//! PostgreSQL store for M5 — sqlx async, TOCTOU-safe state transitions.
//!
//! ## TOCTOU Safety
//!
//! State transitions use:
//!   `UPDATE experiments SET state = $new WHERE experiment_id = $id AND state = $expected`
//! `rows_affected() == 1` confirms we won the race. `0` means either the experiment
//! doesn't exist or a concurrent caller already transitioned it away from `$expected`.
//! We distinguish the two cases with a secondary `SELECT EXISTS` check.
//!
//! Schema: sql/migrations/001_schema.sql

use base64::Engine as _;
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

use crate::state::{ExperimentState, TransitionError};

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Shared PostgreSQL store for M5 management operations.
#[derive(Clone, Debug)]
pub struct ManagementStore {
    pool: PgPool,
}

impl ManagementStore {
    /// Connect and build a pool from the given URL.
    pub async fn connect(database_url: &str, max_connections: u32) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Build from an existing pool (test / admin sharing).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Expose the underlying pool for sharing (e.g. audit trail).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

// ---------------------------------------------------------------------------
// Domain row types
// ---------------------------------------------------------------------------

/// Flattened experiment row from the `experiments` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ExperimentRow {
    pub experiment_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub owner_email: String,
    pub r#type: String,
    pub state: String,
    pub layer_id: Uuid,
    pub primary_metric_id: String,
    pub secondary_metric_ids: Vec<String>,
    pub guardrail_action: String,
    pub hash_salt: String,
    pub targeting_rule_id: Option<Uuid>,
    pub is_cumulative_holdout: bool,
    pub sequential_method: Option<String>,
    pub planned_looks: Option<i32>,
    pub overall_alpha: Option<f64>,
    pub surrogate_model_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub concluded_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub type_config: serde_json::Value,
}

/// Flattened variant row from the `variants` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct VariantRow {
    pub variant_id: Uuid,
    pub experiment_id: Uuid,
    pub name: String,
    pub traffic_fraction: f64,
    pub is_control: bool,
    pub payload_json: serde_json::Value,
    pub ordinal: i32,
}

/// Experiment with its associated variants.
#[derive(Debug, Clone)]
pub struct ExperimentWithVariants {
    pub experiment: ExperimentRow,
    pub variants: Vec<VariantRow>,
}

// ---------------------------------------------------------------------------
// Store errors
// ---------------------------------------------------------------------------

/// Errors from store operations distinct from `TransitionError`.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid page token")]
    InvalidPageToken,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

// ---------------------------------------------------------------------------
// Core queries
// ---------------------------------------------------------------------------

const EXPERIMENT_COLUMNS: &str = "experiment_id, name, description, owner_email, \
    type, state, layer_id, primary_metric_id, secondary_metric_ids, guardrail_action, \
    hash_salt, targeting_rule_id, is_cumulative_holdout, sequential_method, \
    planned_looks, overall_alpha, surrogate_model_id, created_at, started_at, \
    concluded_at, archived_at, updated_at, type_config";

impl ManagementStore {
    /// Fetch a single experiment row by ID.
    pub async fn get_experiment_row(&self, experiment_id: Uuid) -> Result<ExperimentRow, StoreError> {
        sqlx::query_as::<_, ExperimentRow>(&format!(
            "SELECT {EXPERIMENT_COLUMNS} FROM experiments WHERE experiment_id = $1"
        ))
        .bind(experiment_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::Db)?
        .ok_or_else(|| StoreError::NotFound(format!("experiment {experiment_id}")))
    }

    /// Fetch variants for an experiment ordered by ordinal.
    pub async fn get_variants(&self, experiment_id: Uuid) -> Result<Vec<VariantRow>, StoreError> {
        sqlx::query_as::<_, VariantRow>(
            "SELECT variant_id, experiment_id, name, traffic_fraction, is_control, \
             payload_json, ordinal \
             FROM variants WHERE experiment_id = $1 ORDER BY ordinal, variant_id",
        )
        .bind(experiment_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::Db)
    }

    /// Fetch an experiment plus its variants.
    pub async fn get_experiment(&self, experiment_id: Uuid) -> Result<ExperimentWithVariants, StoreError> {
        let experiment = self.get_experiment_row(experiment_id).await?;
        let variants = self.get_variants(experiment_id).await?;
        Ok(ExperimentWithVariants { experiment, variants })
    }

    /// List experiments with optional state filter and cursor-based pagination.
    ///
    /// `page_token` is a base64-encoded UUID cursor. Empty string means first page.
    /// Phase 1: only state filter supported; type/owner filters are Phase 2.
    pub async fn list_experiments(
        &self,
        page_size: i64,
        page_token: &str,
        state_filter: Option<ExperimentState>,
        _type_filter: Option<&str>,
        _owner_filter: Option<&str>,
    ) -> Result<(Vec<ExperimentRow>, Option<String>), StoreError> {
        let limit = page_size.clamp(1, 200) + 1; // +1 to detect next page

        // Decode cursor (base64 → UUID string → Uuid).
        let after_id: Option<Uuid> = if page_token.is_empty() {
            None
        } else {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(page_token)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|s| Uuid::parse_str(s.as_str()).ok());
            if decoded.is_none() {
                return Err(StoreError::InvalidPageToken);
            }
            decoded
        };

        // Phase 1: state filter only. Cursor is experiment_id order.
        let rows = match (state_filter, after_id) {
            (None, None) => {
                sqlx::query_as::<_, ExperimentRow>(&format!(
                    "SELECT {EXPERIMENT_COLUMNS} FROM experiments \
                     ORDER BY created_at DESC LIMIT $1"
                ))
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(StoreError::Db)?
            }
            (Some(state), None) => {
                sqlx::query_as::<_, ExperimentRow>(&format!(
                    "SELECT {EXPERIMENT_COLUMNS} FROM experiments \
                     WHERE state = $1 ORDER BY created_at DESC LIMIT $2"
                ))
                .bind(state.as_db_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(StoreError::Db)?
            }
            (None, Some(cursor)) => {
                sqlx::query_as::<_, ExperimentRow>(&format!(
                    "SELECT {EXPERIMENT_COLUMNS} FROM experiments \
                     WHERE created_at < (SELECT created_at FROM experiments WHERE experiment_id = $1) \
                     ORDER BY created_at DESC LIMIT $2"
                ))
                .bind(cursor)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(StoreError::Db)?
            }
            (Some(state), Some(cursor)) => {
                sqlx::query_as::<_, ExperimentRow>(&format!(
                    "SELECT {EXPERIMENT_COLUMNS} FROM experiments \
                     WHERE state = $1 \
                     AND created_at < (SELECT created_at FROM experiments WHERE experiment_id = $2) \
                     ORDER BY created_at DESC LIMIT $3"
                ))
                .bind(state.as_db_str())
                .bind(cursor)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(StoreError::Db)?
            }
        };

        let has_next = rows.len() as i64 == limit;
        let mut rows = rows;
        if has_next {
            rows.pop();
        }

        let next_token = if has_next {
            rows.last().map(|r| {
                base64::engine::general_purpose::STANDARD
                    .encode(r.experiment_id.to_string().as_bytes())
            })
        } else {
            None
        };

        Ok((rows, next_token))
    }
}

// ---------------------------------------------------------------------------
// TOCTOU-safe state transitions
// ---------------------------------------------------------------------------

impl ManagementStore {
    /// Atomically transition `experiment_id` from state `from` to state `to`.
    ///
    /// Uses `UPDATE … WHERE state = $from`. If `rows_affected() == 0`, a concurrent
    /// caller already modified the state and we return `ConcurrentModification`.
    ///
    /// **Precondition**: call `state::validate_transition(from, to)` before this
    /// method — `apply_transition` does not recheck the state graph.
    pub async fn apply_transition(
        &self,
        experiment_id: Uuid,
        from: ExperimentState,
        to: ExperimentState,
    ) -> Result<ExperimentRow, TransitionError> {
        // Build the SET clause: include the timestamp column matching the `to` state.
        let sql = match to {
            ExperimentState::Running => {
                "UPDATE experiments SET state = $1, started_at = NOW(), updated_at = NOW() \
                 WHERE experiment_id = $2 AND state = $3"
            }
            ExperimentState::Concluded => {
                "UPDATE experiments SET state = $1, concluded_at = NOW(), updated_at = NOW() \
                 WHERE experiment_id = $2 AND state = $3"
            }
            ExperimentState::Archived => {
                "UPDATE experiments SET state = $1, archived_at = NOW(), updated_at = NOW() \
                 WHERE experiment_id = $2 AND state = $3"
            }
            _ => {
                "UPDATE experiments SET state = $1, updated_at = NOW() \
                 WHERE experiment_id = $2 AND state = $3"
            }
        };

        let result = sqlx::query(sql)
            .bind(to.as_db_str())
            .bind(experiment_id)
            .bind(from.as_db_str())
            .execute(&self.pool)
            .await
            .map_err(TransitionError::Db)?;

        if result.rows_affected() == 0 {
            // Distinguish "not found" from "concurrent modification".
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM experiments WHERE experiment_id = $1)",
            )
            .bind(experiment_id)
            .fetch_one(&self.pool)
            .await
            .map_err(TransitionError::Db)?;

            return if !exists {
                Err(TransitionError::NotFound(experiment_id))
            } else {
                Err(TransitionError::ConcurrentModification {
                    experiment_id,
                    expected: from,
                })
            };
        }

        // Return the updated row.
        self.get_experiment_row(experiment_id)
            .await
            .map_err(|e| TransitionError::Db(sqlx::Error::Protocol(format!("post-transition fetch: {e}"))))
    }

    /// Insert an audit trail entry (non-fatal — logs on failure, never returns error).
    pub async fn audit(
        &self,
        experiment_id: Uuid,
        action: &str,
        actor_email: &str,
        previous_state: Option<ExperimentState>,
        new_state: Option<ExperimentState>,
    ) {
        let result = sqlx::query(
            "INSERT INTO audit_trail \
             (experiment_id, action, actor_email, previous_state, new_state) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(experiment_id)
        .bind(action)
        .bind(actor_email)
        .bind(previous_state.map(|s| s.as_db_str()))
        .bind(new_state.map(|s| s.as_db_str()))
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            tracing::warn!(
                %experiment_id, %action, %actor_email,
                error = %e, "audit trail write failed (non-fatal)"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests (no real DB required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::state::ExperimentState;

    /// Verify that ExperimentState DB strings match the schema CHECK constraint.
    /// schema: state IN ('DRAFT', 'STARTING', 'RUNNING', 'CONCLUDING', 'CONCLUDED', 'ARCHIVED')
    #[test]
    fn state_db_strings_match_schema() {
        let cases = [
            (ExperimentState::Draft, "DRAFT"),
            (ExperimentState::Starting, "STARTING"),
            (ExperimentState::Running, "RUNNING"),
            (ExperimentState::Concluding, "CONCLUDING"),
            (ExperimentState::Concluded, "CONCLUDED"),
            (ExperimentState::Archived, "ARCHIVED"),
        ];
        for (state, expected) in &cases {
            assert_eq!(state.as_db_str(), *expected, "{state:?} DB string mismatch");
        }
    }
}
