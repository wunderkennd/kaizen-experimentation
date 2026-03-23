//! Polling reconciler for promoted feature flags (ADR-024 Phase 3).
//!
//! Periodically scans flags with `promoted_experiment_id IS NOT NULL` and
//! `resolved_at IS NULL`, checks each experiment's state via M5's
//! `GetExperiment` RPC, and auto-resolves flags whose experiments have
//! reached CONCLUDED or ARCHIVED.
//!
//! Resolution actions (port from Go `ResolutionAction`):
//! - `RolloutFull` — set rollout_percentage = 1.0, enabled = true
//! - `Rollback`    — set rollout_percentage = 0.0, enabled = false
//! - `Keep`        — mark resolved_at only (manual follow-up)

use std::sync::Arc;
use std::time::Duration;

use tonic::transport::Channel;
use tracing::{error, info, warn};
use uuid::Uuid;

use experimentation_proto::experimentation::common::v1::ExperimentState;
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_client::ExperimentManagementServiceClient,
    GetExperimentRequest,
};

use crate::audit::AuditStore;
use crate::store::FlagStore;

// ---------------------------------------------------------------------------
// Resolution action
// ---------------------------------------------------------------------------

/// How the reconciler updates a flag when its experiment concludes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResolutionAction {
    /// Set rollout to 100% and enable the flag (treatment won).
    #[default]
    RolloutFull,
    /// Set rollout to 0% and disable the flag (control won / experiment failed).
    Rollback,
    /// Mark resolved_at only — do not change flag config (manual decision needed).
    Keep,
}

impl ResolutionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            ResolutionAction::RolloutFull => "rollout_full",
            ResolutionAction::Rollback => "rollback",
            ResolutionAction::Keep => "keep",
        }
    }

    pub fn parse_action(s: &str) -> Option<Self> {
        match s {
            "rollout_full" => Some(ResolutionAction::RolloutFull),
            "rollback" => Some(ResolutionAction::Rollback),
            "keep" => Some(ResolutionAction::Keep),
            _ => None,
        }
    }
}


// ---------------------------------------------------------------------------
// Reconciler
// ---------------------------------------------------------------------------

pub struct Reconciler {
    pub store: Arc<FlagStore>,
    pub audit: Option<Arc<AuditStore>>,
    pub m5_client: ExperimentManagementServiceClient<Channel>,
    pub interval: Duration,
    pub default_action: ResolutionAction,
}

impl Reconciler {
    pub fn new(
        store: Arc<FlagStore>,
        audit: Option<Arc<AuditStore>>,
        m5_client: ExperimentManagementServiceClient<Channel>,
        interval: Duration,
        default_action: ResolutionAction,
    ) -> Self {
        let interval = if interval.is_zero() {
            Duration::from_secs(60)
        } else {
            interval
        };
        Self {
            store,
            audit,
            m5_client,
            interval,
            default_action,
        }
    }

    /// Run the reconciliation loop. Blocks until the runtime shuts down.
    pub async fn run(mut self) {
        info!(
            interval_secs = self.interval.as_secs(),
            default_action = self.default_action.as_str(),
            "reconciler: starting"
        );

        let mut ticker = tokio::time::interval(self.interval);
        // Skip the first immediate tick so the service has time to fully start.
        ticker.tick().await;

        loop {
            ticker.tick().await;
            self.reconcile_once().await;
        }
    }

    /// Perform one reconciliation pass (also called by the Kafka consumer path).
    pub async fn reconcile_once(&mut self) {
        let flags = match self.store.get_promoted_flags().await {
            Ok(f) => f,
            Err(e) => {
                error!(error = %e, "reconciler: failed to list promoted flags");
                return;
            }
        };

        for flag in flags {
            if flag.resolved_at.is_some() {
                continue; // Already resolved.
            }

            let experiment_id = match flag.promoted_experiment_id {
                Some(eid) => eid,
                None => continue,
            };

            self.reconcile_flag(flag.flag_id, experiment_id).await;
        }
    }

    /// Resolve a single flag if its experiment has concluded.
    ///
    /// Exposed as `pub` so the Kafka consumer path can trigger ad-hoc resolution.
    pub async fn reconcile_flag(&mut self, flag_id: Uuid, experiment_id: Uuid) {
        let resp = match self
            .m5_client
            .get_experiment(GetExperimentRequest {
                experiment_id: experiment_id.to_string(),
            })
            .await
        {
            Ok(r) => r.into_inner(),
            Err(e) => {
                warn!(
                    error = %e,
                    %flag_id,
                    %experiment_id,
                    "reconciler: GetExperiment from M5 failed"
                );
                return;
            }
        };

        let state = resp.state;
        let is_terminal = state == ExperimentState::Concluded as i32
            || state == ExperimentState::Archived as i32;

        if !is_terminal {
            return;
        }

        info!(
            %flag_id,
            %experiment_id,
            state,
            action = self.default_action.as_str(),
            "reconciler: auto-resolving flag"
        );

        if let Err(e) = self.store.resolve_flag(flag_id, self.default_action).await {
            error!(error = %e, %flag_id, "reconciler: resolve_flag failed");
            return;
        }

        if let Some(audit) = &self.audit {
            let action_label = format!("auto_resolve_{}", self.default_action.as_str());
            if let Err(e) = audit
                .record_audit(
                    flag_id,
                    &action_label,
                    "system/reconciler",
                    &serde_json::json!({
                        "experiment_id": experiment_id.to_string(),
                        "experiment_state": state,
                    }),
                    &serde_json::json!({
                        "action": self.default_action.as_str(),
                        "resolved_at": chrono::Utc::now().to_rfc3339(),
                    }),
                )
                .await
            {
                warn!(error = %e, %flag_id, "reconciler: audit write failed (non-fatal)");
            }
        }
    }
}
