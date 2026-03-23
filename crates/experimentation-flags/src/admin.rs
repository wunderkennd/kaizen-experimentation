//! Internal HTTP admin endpoints for the M7 flags service (ADR-024 Phase 2).
//!
//! Mirrors the Go service's internal routes:
//!   GET  /internal/flags/audit?flag_id=<uuid>[&limit=N]
//!   GET  /internal/flags/stale[?threshold_days=N]
//!   GET  /internal/flags/promoted
//!   POST /internal/flags/resolve?flag_id=<uuid>&action=rollout_full|rollback|keep
//!
//! Served on a separate port (FLAGS_ADMIN_ADDR, default [::]:9090) so it is
//! never accidentally exposed via the gRPC port.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audit::AuditStore;
use crate::reconciler::ResolutionAction;
use crate::store::FlagStore;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AdminState {
    pub store: Arc<FlagStore>,
    pub audit: Option<Arc<AuditStore>>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/internal/flags/audit", get(handle_audit))
        .route("/internal/flags/stale", get(handle_stale))
        .route("/internal/flags/promoted", get(handle_promoted))
        .route("/internal/flags/resolve", post(handle_resolve))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AuditQuery {
    flag_id: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    100
}

async fn handle_audit(
    State(state): State<AdminState>,
    Query(q): Query<AuditQuery>,
) -> Response {
    let audit = match &state.audit {
        Some(a) => a.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, "audit store not configured").into_response(),
    };

    let flag_id = match Uuid::parse_str(&q.flag_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid flag_id UUID").into_response(),
    };

    match audit.get_audit_log(flag_id, q.limit).await {
        Ok(entries) => Json(entries).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("get audit log: {e}"),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct StaleQuery {
    #[serde(default = "default_threshold")]
    threshold_days: i64,
}

fn default_threshold() -> i64 {
    90
}

#[derive(Serialize)]
struct StaleFlagResponse {
    flag_id: String,
    name: String,
    description: String,
    flag_type: String,
    rollout_percentage: f64,
    days_since_update: i64,
    suggestion: String,
}

async fn handle_stale(
    State(state): State<AdminState>,
    Query(q): Query<StaleQuery>,
) -> Response {
    let audit = match &state.audit {
        Some(a) => a.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, "audit store not configured").into_response(),
    };

    match audit.get_stale_flags(q.threshold_days).await {
        Ok(entries) => {
            let resp: Vec<StaleFlagResponse> = entries
                .iter()
                .map(|e| StaleFlagResponse {
                    flag_id: e.flag_id.to_string(),
                    name: e.name.clone(),
                    description: e.description.clone(),
                    flag_type: e.flag_type.clone(),
                    rollout_percentage: e.rollout_percentage,
                    days_since_update: e.days_since_update(),
                    suggestion: e.suggestion(),
                })
                .collect();
            Json(resp).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("get stale flags: {e}"),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct PromotedFlagResponse {
    flag_id: String,
    name: String,
    enabled: bool,
    rollout_percentage: f64,
    promoted_experiment_id: String,
    promoted_at: Option<String>,
}

async fn handle_promoted(State(state): State<AdminState>) -> Response {
    match state.store.get_promoted_flags().await {
        Ok(flags) => {
            let resp: Vec<PromotedFlagResponse> = flags
                .iter()
                .map(|f| PromotedFlagResponse {
                    flag_id: f.flag_id.to_string(),
                    name: f.name.clone(),
                    enabled: f.enabled,
                    rollout_percentage: f.rollout_percentage,
                    promoted_experiment_id: f
                        .promoted_experiment_id
                        .map(|u| u.to_string())
                        .unwrap_or_default(),
                    promoted_at: f
                        .promoted_at
                        .map(|t| t.to_rfc3339()),
                })
                .collect();
            Json(resp).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("get promoted flags: {e}"),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ResolveQuery {
    flag_id: String,
    action: String,
}

#[derive(Serialize)]
struct ResolveResponse {
    flag_id: String,
    action: String,
    ok: bool,
}

async fn handle_resolve(
    State(state): State<AdminState>,
    Query(q): Query<ResolveQuery>,
) -> Response {
    let flag_id = match Uuid::parse_str(&q.flag_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid flag_id UUID").into_response(),
    };

    let action = match ResolutionAction::from_str(&q.action) {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "action must be rollout_full, rollback, or keep",
            )
                .into_response()
        }
    };

    match state.store.resolve_flag(flag_id, action).await {
        Ok(()) => Json(ResolveResponse {
            flag_id: flag_id.to_string(),
            action: q.action,
            ok: true,
        })
        .into_response(),
        Err(crate::store::StoreError::NotFound(msg)) => {
            (StatusCode::NOT_FOUND, msg).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("resolve flag: {e}"),
        )
            .into_response(),
    }
}
