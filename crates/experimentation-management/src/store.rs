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
use sqlx::QueryBuilder;
use uuid::Uuid;

use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, MetricAggregationLevel, MetricDefinition,
    MetricStakeholder, MetricType,
};

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
// Metric definitions (ADR-026 Phase 1)
// ---------------------------------------------------------------------------

/// Row materialised from `metric_definitions`. Mirrors the table columns after
/// migration 007 (stakeholder/aggregation_level) and 011 (type_config JSONB).
#[derive(Debug, Clone)]
pub struct MetricRow {
    pub metric_id: String,
    pub name: String,
    pub description: Option<String>,
    pub r#type: String,
    pub source_event_type: Option<String>,
    pub numerator_event_type: Option<String>,
    pub denominator_event_type: Option<String>,
    pub percentile: Option<f64>,
    pub custom_sql: Option<String>,
    pub lower_is_better: bool,
    pub is_qoe_metric: bool,
    pub cuped_covariate_metric_id: Option<String>,
    pub minimum_detectable_effect: Option<f64>,
    pub stakeholder: String,
    pub aggregation_level: String,
    /// Per-type oneof payload persisted as JSONB. None for legacy 6 types.
    pub type_config: Option<sqlx::types::Json<serde_json::Value>>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
struct MetricRowSql {
    metric_id: String,
    name: String,
    description: Option<String>,
    #[sqlx(rename = "type")]
    r#type: String,
    source_event_type: Option<String>,
    numerator_event_type: Option<String>,
    denominator_event_type: Option<String>,
    percentile: Option<f64>,
    custom_sql: Option<String>,
    lower_is_better: bool,
    is_qoe_metric: bool,
    cuped_covariate_metric_id: Option<String>,
    minimum_detectable_effect: Option<f64>,
    #[sqlx(default)]
    stakeholder: String,
    #[sqlx(default)]
    aggregation_level: String,
    #[sqlx(default)]
    type_config: Option<sqlx::types::Json<serde_json::Value>>,
    created_at: DateTime<Utc>,
}

impl From<MetricRowSql> for MetricRow {
    fn from(r: MetricRowSql) -> Self {
        MetricRow {
            metric_id: r.metric_id,
            name: r.name,
            description: r.description,
            r#type: r.r#type,
            source_event_type: r.source_event_type,
            numerator_event_type: r.numerator_event_type,
            denominator_event_type: r.denominator_event_type,
            percentile: r.percentile,
            custom_sql: r.custom_sql,
            lower_is_better: r.lower_is_better,
            is_qoe_metric: r.is_qoe_metric,
            cuped_covariate_metric_id: r.cuped_covariate_metric_id,
            minimum_detectable_effect: r.minimum_detectable_effect,
            stakeholder: r.stakeholder,
            aggregation_level: r.aggregation_level,
            type_config: r.type_config,
            created_at: r.created_at,
        }
    }
}

/// Filter for `ManagementStore::list_metrics`. All fields are optional and
/// AND-combined; unset fields disable that predicate.
#[derive(Debug, Clone, Default)]
pub struct MetricFilter {
    /// SQL string value (e.g. "USER", "PROVIDER", "PLATFORM").
    pub stakeholder: Option<String>,
    /// SQL string value (e.g. "USER", "EXPERIMENT", "PROVIDER").
    pub aggregation_level: Option<String>,
    /// MetricType string value (e.g. "MEAN", "FILTERED_MEAN", "COMPOSITE").
    pub r#type: Option<String>,
}

// Convert a proto `MetricType` enum to its canonical PG `type` column string.
// Mirrors the CHECK constraint admit-list in migration 011.
fn metric_type_to_sql(t: MetricType) -> &'static str {
    match t {
        MetricType::Unspecified => "",
        MetricType::Mean => "MEAN",
        MetricType::Proportion => "PROPORTION",
        MetricType::Ratio => "RATIO",
        MetricType::Count => "COUNT",
        MetricType::Percentile => "PERCENTILE",
        MetricType::Custom => "CUSTOM",
        MetricType::FilteredMean => "FILTERED_MEAN",
        MetricType::Composite => "COMPOSITE",
        MetricType::WindowedCount => "WINDOWED_COUNT",
    }
}

fn stakeholder_to_sql(s: MetricStakeholder) -> &'static str {
    match s {
        MetricStakeholder::Unspecified => "",
        MetricStakeholder::User => "USER",
        MetricStakeholder::Provider => "PROVIDER",
        MetricStakeholder::Platform => "PLATFORM",
    }
}

fn aggregation_level_to_sql(a: MetricAggregationLevel) -> &'static str {
    match a {
        MetricAggregationLevel::Unspecified => "",
        MetricAggregationLevel::User => "USER",
        MetricAggregationLevel::Experiment => "EXPERIMENT",
        MetricAggregationLevel::Provider => "PROVIDER",
    }
}

/// Proto3 strings default to "" when unset; map empty to NULL for the
/// nullable text columns on `metric_definitions`.
fn opt_str(s: &str) -> Option<&str> {
    if s.is_empty() { None } else { Some(s) }
}

/// Proto3 doubles default to 0.0 when unset; map non-positive to NULL for
/// the optional `percentile` / `minimum_detectable_effect` columns.
fn opt_pos_f64(v: f64) -> Option<f64> {
    if v > 0.0 { Some(v) } else { None }
}

/// Convert the proto `MetricDefinition.type_config` oneof to the JSONB value
/// persisted in `metric_definitions.type_config`. Returns `None` for legacy 6
/// types (which carry their config in flat columns).
fn build_metric_type_config(m: &MetricDefinition) -> Option<serde_json::Value> {
    match m.type_config.as_ref()? {
        MetricTypeConfig::FilteredMean(cfg) => Some(serde_json::json!({
            "filter_sql": cfg.filter_sql,
            "value_column": cfg.value_column,
        })),
        MetricTypeConfig::Composite(cfg) => Some(serde_json::json!({
            "operator": cfg.operator,
            "operands": cfg
                .operands
                .iter()
                .map(|op| serde_json::json!({
                    "metric_id": op.metric_id,
                    "weight": op.weight,
                }))
                .collect::<Vec<_>>(),
        })),
        MetricTypeConfig::WindowedCount(cfg) => Some(serde_json::json!({
            "event_type": cfg.event_type,
            "filter_sql": cfg.filter_sql,
            "window_hours": cfg.window_hours,
        })),
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Parameters for `ManagementStore::create_experiment`.
///
/// Groups the 16 creation parameters to avoid the `clippy::too_many_arguments` lint.
pub struct CreateExperimentParams<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub owner_email: &'a str,
    pub experiment_type: &'a str,
    pub layer_id: Uuid,
    pub primary_metric_id: &'a str,
    pub secondary_metric_ids: &'a [String],
    pub guardrail_action: &'a str,
    pub targeting_rule_id: Option<Uuid>,
    pub is_cumulative_holdout: bool,
    pub type_config: &'a serde_json::Value,
    pub sequential_method: Option<&'a str>,
    pub planned_looks: Option<i32>,
    pub overall_alpha: Option<f64>,
    pub surrogate_model_id: Option<Uuid>,
    pub variants: &'a [(String, f64, bool, serde_json::Value)],
}

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
        params: CreateExperimentParams<'_>,
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
        .bind(params.name)
        .bind(params.description)
        .bind(params.owner_email)
        .bind(params.experiment_type)
        .bind(params.layer_id)
        .bind(params.primary_metric_id)
        .bind(params.secondary_metric_ids)
        .bind(params.guardrail_action)
        .bind(params.targeting_rule_id)
        .bind(params.is_cumulative_holdout)
        .bind(params.type_config)
        .bind(params.sequential_method)
        .bind(params.planned_looks)
        .bind(params.overall_alpha)
        .bind(params.surrogate_model_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                StoreError::AlreadyExists(params.name.to_string())
            } else {
                StoreError::Db(e)
            }
        })?;

        let experiment_id = row.experiment_id;

        for (ordinal, (vname, fraction, is_control, payload)) in params.variants.iter().enumerate() {
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
    // Metric definitions CRUD (ADR-026 Phase 1)
    // ---------------------------------------------------------------------------

    /// Insert a metric definition row. The proto `type_config` oneof is
    /// serialised to JSONB; legacy 6 types carry their config in the flat
    /// sibling columns (`source_event_type`, `numerator_event_type`,
    /// `denominator_event_type`, `percentile`, `custom_sql`).
    ///
    /// Returns `StoreError::AlreadyExists` on PK conflict (duplicate
    /// `metric_id`), `StoreError::Db` on other database errors.
    pub async fn create_metric(
        &self,
        metric: &MetricDefinition,
    ) -> Result<MetricRow, StoreError> {
        let type_config_json = build_metric_type_config(metric);

        let row: MetricRowSql = sqlx::query_as(
            r#"INSERT INTO metric_definitions (
                   metric_id, name, description, type,
                   source_event_type, numerator_event_type, denominator_event_type,
                   percentile, custom_sql,
                   lower_is_better, is_qoe_metric,
                   cuped_covariate_metric_id, minimum_detectable_effect,
                   stakeholder, aggregation_level, type_config
               ) VALUES (
                   $1, $2, $3, $4,
                   $5, $6, $7,
                   $8, $9,
                   $10, $11,
                   $12, $13,
                   $14, $15, $16
               )
               RETURNING
                   metric_id, name, description, type,
                   source_event_type, numerator_event_type, denominator_event_type,
                   percentile, custom_sql,
                   lower_is_better, is_qoe_metric,
                   cuped_covariate_metric_id, minimum_detectable_effect,
                   stakeholder, aggregation_level, type_config, created_at"#,
        )
        .bind(&metric.metric_id)
        .bind(&metric.name)
        .bind(opt_str(&metric.description))
        .bind(metric_type_to_sql(metric.r#type()))
        .bind(opt_str(&metric.source_event_type))
        .bind(opt_str(&metric.numerator_event_type))
        .bind(opt_str(&metric.denominator_event_type))
        .bind(opt_pos_f64(metric.percentile))
        .bind(opt_str(&metric.custom_sql))
        .bind(metric.lower_is_better)
        .bind(metric.is_qoe_metric)
        .bind(opt_str(&metric.cuped_covariate_metric_id))
        .bind(opt_pos_f64(metric.minimum_detectable_effect))
        .bind(stakeholder_to_sql(metric.stakeholder()))
        .bind(aggregation_level_to_sql(metric.aggregation_level()))
        .bind(type_config_json.map(sqlx::types::Json))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                StoreError::AlreadyExists(metric.metric_id.clone())
            } else {
                StoreError::Db(e)
            }
        })?;

        Ok(MetricRow::from(row))
    }

    /// Fetch a single metric definition by `metric_id`.
    ///
    /// Returns `StoreError::NotFound` if no row matches.
    pub async fn get_metric(&self, metric_id: &str) -> Result<MetricRow, StoreError> {
        let row: Option<MetricRowSql> = sqlx::query_as(
            r#"SELECT
                   metric_id, name, description, type,
                   source_event_type, numerator_event_type, denominator_event_type,
                   percentile, custom_sql,
                   lower_is_better, is_qoe_metric,
                   cuped_covariate_metric_id, minimum_detectable_effect,
                   stakeholder, aggregation_level, type_config, created_at
               FROM metric_definitions
               WHERE metric_id = $1"#,
        )
        .bind(metric_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        row.map(MetricRow::from)
            .ok_or_else(|| StoreError::NotFound(metric_id.to_string()))
    }

    /// List metric definitions with optional AND-combined predicates.
    /// Empty filter returns all rows. Ordered by `metric_id` for stable
    /// pagination if callers later layer cursors on top.
    pub async fn list_metrics(
        &self,
        filter: MetricFilter,
    ) -> Result<Vec<MetricRow>, StoreError> {
        let mut qb: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            r#"SELECT
                   metric_id, name, description, type,
                   source_event_type, numerator_event_type, denominator_event_type,
                   percentile, custom_sql,
                   lower_is_better, is_qoe_metric,
                   cuped_covariate_metric_id, minimum_detectable_effect,
                   stakeholder, aggregation_level, type_config, created_at
               FROM metric_definitions WHERE 1 = 1"#,
        );

        if let Some(ref s) = filter.stakeholder {
            qb.push(" AND stakeholder = ").push_bind(s.clone());
        }
        if let Some(ref a) = filter.aggregation_level {
            qb.push(" AND aggregation_level = ").push_bind(a.clone());
        }
        if let Some(ref t) = filter.r#type {
            qb.push(" AND type = ").push_bind(t.clone());
        }
        qb.push(" ORDER BY metric_id");

        let rows: Vec<MetricRowSql> = qb
            .build_query_as()
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::Db)?;

        Ok(rows.into_iter().map(MetricRow::from).collect())
    }

    /// True if a metric with the given id exists.
    pub async fn exists_metric(&self, metric_id: &str) -> Result<bool, StoreError> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM metric_definitions WHERE metric_id = $1)",
        )
        .bind(metric_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::Db)?;
        Ok(row.0)
    }

    /// True iff *every* id in `metric_ids` exists in `metric_definitions`.
    /// Duplicates in the input are deduplicated server-side via the COUNT
    /// over the DISTINCT match set.
    pub async fn exists_all_metrics(
        &self,
        metric_ids: &[&str],
    ) -> Result<bool, StoreError> {
        if metric_ids.is_empty() {
            return Ok(true);
        }
        // Deduplicate input so the COUNT comparison is well-defined when the
        // caller passes the same id twice.
        let mut owned: Vec<String> = metric_ids.iter().map(|s| (*s).to_string()).collect();
        owned.sort();
        owned.dedup();
        let expected = owned.len() as i64;

        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM metric_definitions WHERE metric_id = ANY($1)",
        )
        .bind(&owned)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(row.0 == expected)
    }

    /// Return the operand `metric_id`s of a COMPOSITE metric in declaration
    /// order. Used by the cycle detector in B1.
    ///
    /// - Returns `StoreError::NotFound` if `metric_id` does not exist.
    /// - Returns an empty `Vec` if the row exists but is not COMPOSITE or has
    ///   no `type_config.operands` array.
    pub async fn get_composite_operands(
        &self,
        metric_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        let row: Option<(String, Option<sqlx::types::Json<serde_json::Value>>)> = sqlx::query_as(
            r#"SELECT type, type_config
               FROM metric_definitions
               WHERE metric_id = $1"#,
        )
        .bind(metric_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        let (ty, type_config) = row.ok_or_else(|| StoreError::NotFound(metric_id.to_string()))?;

        if ty != "COMPOSITE" {
            return Ok(Vec::new());
        }

        let Some(json) = type_config else {
            return Ok(Vec::new());
        };
        let Some(operands) = json.0.get("operands").and_then(|v| v.as_array()) else {
            return Ok(Vec::new());
        };

        Ok(operands
            .iter()
            .filter_map(|op| op.get("metric_id").and_then(|v| v.as_str()).map(String::from))
            .collect())
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
