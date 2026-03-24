//! PostgreSQL store for the Management Service.
//!
//! TOCTOU-safe state transitions use:
//! ```sql
//! UPDATE experiments SET state=$new WHERE experiment_id=$id AND state=$expected
//! ```
//! then check `rows_affected() == 1`.
//!
//! Audit trail is append-only: INSERT only, never UPDATE or DELETE.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ExperimentRow {
    pub experiment_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub owner_email: String,
    pub experiment_type: String,
    pub state: String,
    pub layer_id: Uuid,
    pub primary_metric_id: String,
    pub secondary_metric_ids: Vec<String>,
    pub guardrail_action: String,
    pub hash_salt: String,
    pub targeting_rule_id: Option<Uuid>,
    pub is_cumulative_holdout: bool,
    pub type_config: serde_json::Value,
    pub sequential_method: Option<String>,
    pub planned_looks: Option<i32>,
    pub overall_alpha: Option<f64>,
    pub surrogate_model_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub concluded_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub resumed_at: Option<DateTime<Utc>>,
    pub pause_reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
struct ExperimentRowSql {
    experiment_id: Uuid,
    name: String,
    description: Option<String>,
    owner_email: String,
    #[sqlx(rename = "type")]
    experiment_type: String,
    state: String,
    layer_id: Uuid,
    primary_metric_id: String,
    #[sqlx(default)]
    secondary_metric_ids: Vec<String>,
    guardrail_action: String,
    hash_salt: String,
    targeting_rule_id: Option<Uuid>,
    is_cumulative_holdout: bool,
    type_config: serde_json::Value,
    sequential_method: Option<String>,
    planned_looks: Option<i32>,
    overall_alpha: Option<f64>,
    surrogate_model_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    concluded_at: Option<DateTime<Utc>>,
    archived_at: Option<DateTime<Utc>>,
    // These columns added by migration 009 — default to None if absent.
    #[sqlx(default)]
    paused_at: Option<DateTime<Utc>>,
    #[sqlx(default)]
    resumed_at: Option<DateTime<Utc>>,
    #[sqlx(default)]
    pause_reason: Option<String>,
    updated_at: DateTime<Utc>,
}

impl From<ExperimentRowSql> for ExperimentRow {
    fn from(r: ExperimentRowSql) -> Self {
        ExperimentRow {
            experiment_id: r.experiment_id,
            name: r.name,
            description: r.description,
            owner_email: r.owner_email,
            experiment_type: r.experiment_type,
            state: r.state,
            layer_id: r.layer_id,
            primary_metric_id: r.primary_metric_id,
            secondary_metric_ids: r.secondary_metric_ids,
            guardrail_action: r.guardrail_action,
            hash_salt: r.hash_salt,
            targeting_rule_id: r.targeting_rule_id,
            is_cumulative_holdout: r.is_cumulative_holdout,
            type_config: r.type_config,
            sequential_method: r.sequential_method,
            planned_looks: r.planned_looks,
            overall_alpha: r.overall_alpha,
            surrogate_model_id: r.surrogate_model_id,
            created_at: r.created_at,
            started_at: r.started_at,
            concluded_at: r.concluded_at,
            archived_at: r.archived_at,
            paused_at: r.paused_at,
            resumed_at: r.resumed_at,
            pause_reason: r.pause_reason,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VariantRow {
    pub variant_id: Uuid,
    pub experiment_id: Uuid,
    pub name: String,
    pub traffic_fraction: f64,
    pub is_control: bool,
    pub payload_json: serde_json::Value,
    pub ordinal: i32,
}

#[derive(sqlx::FromRow, Debug, Clone)]
struct VariantRowSql {
    variant_id: Uuid,
    experiment_id: Uuid,
    name: String,
    traffic_fraction: f64,
    is_control: bool,
    payload_json: serde_json::Value,
    ordinal: i32,
}

impl From<VariantRowSql> for VariantRow {
    fn from(r: VariantRowSql) -> Self {
        VariantRow {
            variant_id: r.variant_id,
            experiment_id: r.experiment_id,
            name: r.name,
            traffic_fraction: r.traffic_fraction,
            is_control: r.is_control,
            payload_json: r.payload_json,
            ordinal: r.ordinal,
        }
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ManagementStore {
    pub pool: PgPool,
}

impl ManagementStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .connect(database_url)
            .await
            .context("connect to PostgreSQL")?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ---------------------------------------------------------------------------
    // Experiment CRUD
    // ---------------------------------------------------------------------------

    pub async fn create_experiment(
        &self,
        name: &str,
        description: Option<&str>,
        owner_email: &str,
        experiment_type: &str,
        layer_id: Uuid,
        primary_metric_id: &str,
        secondary_metric_ids: &[String],
        guardrail_action: &str,
        targeting_rule_id: Option<Uuid>,
        is_cumulative_holdout: bool,
        type_config: &serde_json::Value,
        sequential_method: Option<&str>,
        planned_looks: Option<i32>,
        overall_alpha: Option<f64>,
        surrogate_model_id: Option<Uuid>,
        variants: &[(String, f64, bool, serde_json::Value)], // (name, fraction, is_control, payload)
    ) -> Result<ExperimentRow, StoreError> {
        let mut tx = self.pool.begin().await.map_err(StoreError::Db)?;

        let row: ExperimentRowSql = sqlx::query_as(
            r#"INSERT INTO experiments (
                   name, description, owner_email, type, layer_id,
                   primary_metric_id, secondary_metric_ids, guardrail_action,
                   targeting_rule_id, is_cumulative_holdout, type_config,
                   sequential_method, planned_looks, overall_alpha, surrogate_model_id
               ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
               RETURNING
                   experiment_id, name, description, owner_email,
                   type, state, layer_id, primary_metric_id, secondary_metric_ids,
                   guardrail_action, hash_salt, targeting_rule_id, is_cumulative_holdout,
                   type_config, sequential_method, planned_looks, overall_alpha,
                   surrogate_model_id, created_at, started_at, concluded_at,
                   archived_at, updated_at,
                   NULL::TIMESTAMPTZ AS paused_at,
                   NULL::TIMESTAMPTZ AS resumed_at,
                   NULL::TEXT AS pause_reason"#,
        )
        .bind(name)
        .bind(description)
        .bind(owner_email)
        .bind(experiment_type)
        .bind(layer_id)
        .bind(primary_metric_id)
        .bind(secondary_metric_ids)
        .bind(guardrail_action)
        .bind(targeting_rule_id)
        .bind(is_cumulative_holdout)
        .bind(type_config)
        .bind(sequential_method)
        .bind(planned_looks)
        .bind(overall_alpha)
        .bind(surrogate_model_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                StoreError::AlreadyExists(name.to_string())
            } else {
                StoreError::Db(e)
            }
        })?;

        let experiment_id = row.experiment_id;

        for (ordinal, (vname, fraction, is_control, payload)) in variants.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO variants (experiment_id, name, traffic_fraction, is_control, payload_json, ordinal)
                   VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(experiment_id)
            .bind(vname)
            .bind(fraction)
            .bind(is_control)
            .bind(payload)
            .bind(ordinal as i32)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Db)?;
        }

        tx.commit().await.map_err(StoreError::Db)?;
        self.get_experiment(experiment_id).await
    }

    pub async fn get_experiment(&self, experiment_id: Uuid) -> Result<ExperimentRow, StoreError> {
        let row: Option<ExperimentRowSql> = sqlx::query_as(
            r#"SELECT
                   experiment_id, name, description, owner_email,
                   type, state, layer_id, primary_metric_id, secondary_metric_ids,
                   guardrail_action, hash_salt, targeting_rule_id, is_cumulative_holdout,
                   type_config, sequential_method, planned_looks, overall_alpha,
                   surrogate_model_id, created_at, started_at, concluded_at,
                   archived_at, updated_at,
                   paused_at, resumed_at, pause_reason
               FROM experiments WHERE experiment_id = $1"#,
        )
        .bind(experiment_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        row.map(ExperimentRow::from)
            .ok_or_else(|| StoreError::NotFound(experiment_id.to_string()))
    }

    pub async fn list_experiments(
        &self,
        state_filter: Option<&str>,
        type_filter: Option<&str>,
        owner_filter: Option<&str>,
        page_size: i64,
        page_token: Option<Uuid>,
    ) -> Result<(Vec<ExperimentRow>, Option<Uuid>), StoreError> {
        let page_size = if page_size <= 0 || page_size > 100 { 50 } else { page_size };
        let fetch_size = page_size + 1;

        let rows: Vec<ExperimentRowSql> = if let Some(cursor) = page_token {
            sqlx::query_as(
                r#"SELECT
                       experiment_id, name, description, owner_email,
                       type, state, layer_id, primary_metric_id, secondary_metric_ids,
                       guardrail_action, hash_salt, targeting_rule_id, is_cumulative_holdout,
                       type_config, sequential_method, planned_looks, overall_alpha,
                       surrogate_model_id, created_at, started_at, concluded_at,
                       archived_at, updated_at,
                       paused_at, resumed_at, pause_reason
                   FROM experiments
                   WHERE ($1::TEXT IS NULL OR state = $1)
                     AND ($2::TEXT IS NULL OR type = $2)
                     AND ($3::TEXT IS NULL OR owner_email = $3)
                     AND experiment_id > $4
                   ORDER BY experiment_id LIMIT $5"#,
            )
            .bind(state_filter)
            .bind(type_filter)
            .bind(owner_filter)
            .bind(cursor)
            .bind(fetch_size)
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::Db)?
        } else {
            sqlx::query_as(
                r#"SELECT
                       experiment_id, name, description, owner_email,
                       type, state, layer_id, primary_metric_id, secondary_metric_ids,
                       guardrail_action, hash_salt, targeting_rule_id, is_cumulative_holdout,
                       type_config, sequential_method, planned_looks, overall_alpha,
                       surrogate_model_id, created_at, started_at, concluded_at,
                       archived_at, updated_at,
                       paused_at, resumed_at, pause_reason
                   FROM experiments
                   WHERE ($1::TEXT IS NULL OR state = $1)
                     AND ($2::TEXT IS NULL OR type = $2)
                     AND ($3::TEXT IS NULL OR owner_email = $3)
                   ORDER BY experiment_id LIMIT $4"#,
            )
            .bind(state_filter)
            .bind(type_filter)
            .bind(owner_filter)
            .bind(fetch_size)
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::Db)?
        };

        let mut experiments: Vec<ExperimentRow> = rows.into_iter().map(ExperimentRow::from).collect();

        let next_token = if experiments.len() as i64 > page_size {
            let last_id = experiments[(page_size - 1) as usize].experiment_id;
            experiments.truncate(page_size as usize);
            Some(last_id)
        } else {
            None
        };

        Ok((experiments, next_token))
    }

    /// Return all experiments in RUNNING or PAUSED states (for StreamConfigUpdates backfill).
    pub async fn list_active_experiments(&self) -> Result<Vec<ExperimentRow>, StoreError> {
        let rows: Vec<ExperimentRowSql> = sqlx::query_as(
            r#"SELECT
                   experiment_id, name, description, owner_email,
                   type, state, layer_id, primary_metric_id, secondary_metric_ids,
                   guardrail_action, hash_salt, targeting_rule_id, is_cumulative_holdout,
                   type_config, sequential_method, planned_looks, overall_alpha,
                   surrogate_model_id, created_at, started_at, concluded_at,
                   archived_at, updated_at,
                   paused_at, resumed_at, pause_reason
               FROM experiments
               WHERE state IN ('RUNNING', 'PAUSED')
               ORDER BY experiment_id"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(rows.into_iter().map(ExperimentRow::from).collect())
    }

    pub async fn get_variants(&self, experiment_id: Uuid) -> Result<Vec<VariantRow>, StoreError> {
        let rows: Vec<VariantRowSql> = sqlx::query_as(
            r#"SELECT variant_id, experiment_id, name, traffic_fraction, is_control, payload_json, ordinal
               FROM variants WHERE experiment_id = $1 ORDER BY ordinal"#,
        )
        .bind(experiment_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(rows.into_iter().map(VariantRow::from).collect())
    }

    // ---------------------------------------------------------------------------
    // Lifecycle transitions (TOCTOU-safe)
    // ---------------------------------------------------------------------------

    /// TOCTOU-safe: update state only if the current state equals `from`.
    /// Returns rows_affected (0 = conflict or not found, 1 = success).
    pub async fn transition_state_raw(
        &self,
        experiment_id: Uuid,
        from: &str,
        to: &str,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = $2, updated_at = NOW()
               WHERE experiment_id = $1 AND state = $3"#,
        )
        .bind(experiment_id)
        .bind(to)
        .bind(from)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Transition to STARTING (DRAFT→STARTING) and record started_at.
    pub async fn start_transition(
        &self,
        experiment_id: Uuid,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'STARTING', started_at = NOW(), updated_at = NOW()
               WHERE experiment_id = $1 AND state = 'DRAFT'"#,
        )
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Transition STARTING→RUNNING after successful validation.
    pub async fn run_transition(&self, experiment_id: Uuid) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'RUNNING', updated_at = NOW()
               WHERE experiment_id = $1 AND state = 'STARTING'"#,
        )
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Transition STARTING→DRAFT on validation failure.
    pub async fn revert_to_draft(&self, experiment_id: Uuid) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'DRAFT', started_at = NULL, updated_at = NOW()
               WHERE experiment_id = $1 AND state = 'STARTING'"#,
        )
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Pause: RUNNING→PAUSED. Records pause reason and timestamp.
    pub async fn pause_transition(
        &self,
        experiment_id: Uuid,
        reason: &str,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'PAUSED', paused_at = NOW(),
                   pause_reason = $2, updated_at = NOW()
               WHERE experiment_id = $1 AND state = 'RUNNING'"#,
        )
        .bind(experiment_id)
        .bind(reason)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Resume: PAUSED→RUNNING. Records resumed_at.
    pub async fn resume_transition(&self, experiment_id: Uuid) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'RUNNING', resumed_at = NOW(),
                   pause_reason = NULL, updated_at = NOW()
               WHERE experiment_id = $1 AND state = 'PAUSED'"#,
        )
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Conclude: RUNNING or PAUSED → CONCLUDING.
    pub async fn conclude_transition(&self, experiment_id: Uuid) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'CONCLUDING', updated_at = NOW()
               WHERE experiment_id = $1 AND state IN ('RUNNING', 'PAUSED')"#,
        )
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Mark CONCLUDING→CONCLUDED with concluded_at timestamp.
    pub async fn mark_concluded(&self, experiment_id: Uuid) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'CONCLUDED', concluded_at = NOW(), updated_at = NOW()
               WHERE experiment_id = $1 AND state = 'CONCLUDING'"#,
        )
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    /// Archive: CONCLUDED→ARCHIVED.
    pub async fn archive_transition(&self, experiment_id: Uuid) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"UPDATE experiments
               SET state = 'ARCHIVED', archived_at = NOW(), updated_at = NOW()
               WHERE experiment_id = $1 AND state = 'CONCLUDED'"#,
        )
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(result.rows_affected())
    }

    // ---------------------------------------------------------------------------
    // Audit trail
    // ---------------------------------------------------------------------------

    /// Append an immutable audit entry. Never updates or deletes existing entries.
    pub async fn record_audit(
        &self,
        experiment_id: Uuid,
        action: &str,
        actor_email: &str,
        previous_state: Option<&str>,
        new_state: Option<&str>,
        details: &serde_json::Value,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"INSERT INTO audit_trail
                   (experiment_id, action, actor_email, previous_state, new_state, details_json)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(experiment_id)
        .bind(action)
        .bind(actor_email)
        .bind(previous_state)
        .bind(new_state)
        .bind(details)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("experiment not found: {0}")]
    NotFound(String),
    #[error("experiment already exists: {0}")]
    AlreadyExists(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Convenience: check experiment existence.
pub async fn experiment_exists(pool: &PgPool, experiment_id: Uuid) -> Result<bool, StoreError> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT experiment_id FROM experiments WHERE experiment_id = $1",
    )
    .bind(experiment_id)
    .fetch_optional(pool)
    .await
    .map_err(StoreError::Db)?;
    Ok(row.is_some())
}
