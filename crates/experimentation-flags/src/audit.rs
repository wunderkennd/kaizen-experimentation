//! Audit trail store for feature flags (ADR-024 Phase 2).
//!
//! Records all mutations to feature flags in the `flag_audit_trail` table.
//! Provides stale flag detection by querying `feature_flags` directly
//! (mirrors the `stale_flags` SQL view logic from migration 003).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub audit_id: Uuid,
    pub flag_id: Uuid,
    pub action: String,
    pub actor_email: String,
    pub previous_value: serde_json::Value,
    pub new_value: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaleFlagEntry {
    pub flag_id: Uuid,
    pub name: String,
    pub description: String,
    pub flag_type: String,
    pub enabled: bool,
    pub rollout_percentage: f64,
    pub updated_at: DateTime<Utc>,
    /// Seconds since last update, from EXTRACT(EPOCH …).
    pub stale_seconds: f64,
}

impl StaleFlagEntry {
    pub fn days_since_update(&self) -> i64 {
        (self.stale_seconds / 86_400.0) as i64
    }

    pub fn suggestion(&self) -> String {
        let days = self.days_since_update();
        if days >= 365 {
            format!(
                "Critical: flag '{}' appears abandoned ({days} days unchanged) — remove to reduce technical debt.",
                self.name
            )
        } else if days >= 180 {
            format!(
                "Strongly recommend removing flag '{}' — flag has been unchanged for {days} days.",
                self.name
            )
        } else {
            format!(
                "Flag '{}' has been at 100% rollout for {days} days. Consider removing the flag and making the behavior permanent.",
                self.name
            )
        }
    }
}

// ---------------------------------------------------------------------------
// AuditStore
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AuditStore {
    pool: PgPool,
}

impl AuditStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Record a mutation audit entry. Errors are non-fatal for callers — log and continue.
    pub async fn record_audit(
        &self,
        flag_id: Uuid,
        action: &str,
        actor_email: &str,
        previous_value: &serde_json::Value,
        new_value: &serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO flag_audit_trail (flag_id, action, actor_email, previous_value, new_value)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(flag_id)
        .bind(action)
        .bind(actor_email)
        .bind(previous_value)
        .bind(new_value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Retrieve the audit log for a flag, most recent first. Clamps limit to [1, 1000].
    pub async fn get_audit_log(
        &self,
        flag_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AuditEntry>, sqlx::Error> {
        let limit = limit.clamp(1, 1000);
        let rows: Vec<AuditRow> = sqlx::query_as(
            r#"SELECT audit_id, flag_id, action, actor_email, previous_value, new_value, created_at
               FROM flag_audit_trail
               WHERE flag_id = $1
               ORDER BY created_at DESC
               LIMIT $2"#,
        )
        .bind(flag_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(AuditEntry::from).collect())
    }

    /// Return flags at 100% rollout unchanged for more than `threshold_days` days.
    /// Mirrors the `stale_flags` SQL view from migration 003.
    pub async fn get_stale_flags(
        &self,
        threshold_days: i64,
    ) -> Result<Vec<StaleFlagEntry>, sqlx::Error> {
        let threshold = threshold_days.max(1);
        let rows: Vec<StaleFlagRow> = sqlx::query_as(
            r#"SELECT flag_id, name, description,
                      type AS flag_type,
                      enabled, rollout_percentage, updated_at,
                      EXTRACT(EPOCH FROM (NOW() - updated_at)) AS stale_seconds
               FROM feature_flags
               WHERE enabled = TRUE
                 AND rollout_percentage >= 1.0
                 AND promoted_experiment_id IS NULL
                 AND updated_at < NOW() - ($1::bigint * interval '1 day')
               ORDER BY updated_at ASC"#,
        )
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(StaleFlagEntry::from).collect())
    }
}

// ---------------------------------------------------------------------------
// sqlx row types
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct AuditRow {
    audit_id: Uuid,
    flag_id: Uuid,
    action: String,
    actor_email: String,
    previous_value: serde_json::Value,
    new_value: serde_json::Value,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct StaleFlagRow {
    flag_id: Uuid,
    name: String,
    description: String,
    flag_type: String,
    enabled: bool,
    rollout_percentage: f64,
    updated_at: DateTime<Utc>,
    stale_seconds: f64,
}

impl From<AuditRow> for AuditEntry {
    fn from(r: AuditRow) -> Self {
        AuditEntry {
            audit_id: r.audit_id,
            flag_id: r.flag_id,
            action: r.action,
            actor_email: r.actor_email,
            previous_value: r.previous_value,
            new_value: r.new_value,
            created_at: r.created_at,
        }
    }
}

impl From<StaleFlagRow> for StaleFlagEntry {
    fn from(r: StaleFlagRow) -> Self {
        StaleFlagEntry {
            flag_id: r.flag_id,
            name: r.name,
            description: r.description,
            flag_type: r.flag_type,
            enabled: r.enabled,
            rollout_percentage: r.rollout_percentage,
            updated_at: r.updated_at,
            stale_seconds: r.stale_seconds,
        }
    }
}
