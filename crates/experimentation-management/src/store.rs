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
    metric_definition::TypeConfig as MetricTypeConfig, CompositeConfig, CompositeOperand,
    FilteredMeanConfig, MetricAggregationLevel, MetricDefinition, MetricStakeholder, MetricType,
    WindowedCountConfig,
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
/// migration 007 (stakeholder/aggregation_level), 011 (type_config JSONB), and
/// 013 (metricql_expression TEXT, ADR-026 Phase 2).
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
    /// MetricQL source text (migration 013, ADR-026 Phase 2 / #435). None for
    /// legacy 6 types and Phase 1 structured types; populated only for METRICQL.
    pub metricql_expression: Option<String>,
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
    #[sqlx(default)]
    metricql_expression: Option<String>,
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
            metricql_expression: r.metricql_expression,
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
        MetricType::Metricql => "METRICQL",
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

// Inverse of `metric_type_to_sql`: rebuild the proto enum from the PG `type`
// column string. Unknown strings fall back to `Unspecified`.
fn metric_type_from_sql(s: &str) -> MetricType {
    match s {
        "MEAN" => MetricType::Mean,
        "PROPORTION" => MetricType::Proportion,
        "RATIO" => MetricType::Ratio,
        "COUNT" => MetricType::Count,
        "PERCENTILE" => MetricType::Percentile,
        "CUSTOM" => MetricType::Custom,
        "FILTERED_MEAN" => MetricType::FilteredMean,
        "COMPOSITE" => MetricType::Composite,
        "WINDOWED_COUNT" => MetricType::WindowedCount,
        "METRICQL" => MetricType::Metricql,
        _ => MetricType::Unspecified,
    }
}

fn stakeholder_from_sql(s: &str) -> MetricStakeholder {
    match s {
        "USER" => MetricStakeholder::User,
        "PROVIDER" => MetricStakeholder::Provider,
        "PLATFORM" => MetricStakeholder::Platform,
        _ => MetricStakeholder::Unspecified,
    }
}

fn aggregation_level_from_sql(s: &str) -> MetricAggregationLevel {
    match s {
        "USER" => MetricAggregationLevel::User,
        "EXPERIMENT" => MetricAggregationLevel::Experiment,
        "PROVIDER" => MetricAggregationLevel::Provider,
        _ => MetricAggregationLevel::Unspecified,
    }
}

/// Inverse of `build_metric_type_config`: rebuild the proto `TypeConfig`
/// oneof arm from the JSONB stored in `metric_definitions.type_config`.
/// Returns `None` for legacy 6 types or rows without a structured payload.
fn metric_type_config_from_json(
    ty: MetricType,
    json: Option<&serde_json::Value>,
) -> Option<MetricTypeConfig> {
    let v = json?;
    match ty {
        MetricType::FilteredMean => {
            Some(MetricTypeConfig::FilteredMean(FilteredMeanConfig {
                filter_sql: v
                    .get("filter_sql")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                value_column: v
                    .get("value_column")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
            }))
        }
        MetricType::Composite => {
            let operands = v
                .get("operands")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|op| CompositeOperand {
                            metric_id: op
                                .get("metric_id")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string(),
                            weight: op
                                .get("weight")
                                .and_then(|n| n.as_f64())
                                .unwrap_or(0.0),
                        })
                        .collect()
                })
                .unwrap_or_default();
            // operator is stored as the i32 enum value
            let operator = v.get("operator").and_then(|n| n.as_i64()).unwrap_or(0) as i32;
            Some(MetricTypeConfig::Composite(CompositeConfig {
                operands,
                operator,
            }))
        }
        MetricType::WindowedCount => {
            Some(MetricTypeConfig::WindowedCount(WindowedCountConfig {
                event_type: v
                    .get("event_type")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                filter_sql: v
                    .get("filter_sql")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                window_hours: v
                    .get("window_hours")
                    .and_then(|n| n.as_i64())
                    .unwrap_or(0) as i32,
            }))
        }
        _ => None,
    }
}

impl MetricRow {
    /// Round-trip a stored row back into the proto `MetricDefinition`. Mirrors
    /// the inverse of `create_metric`'s serialisation: flat columns map to
    /// their proto siblings, and `type_config` JSONB is rehydrated into the
    /// oneof arm for the 3 Phase 1 types.
    pub fn into_proto(self) -> MetricDefinition {
        let ty = metric_type_from_sql(&self.r#type);
        let stakeholder = stakeholder_from_sql(&self.stakeholder);
        let aggregation_level = aggregation_level_from_sql(&self.aggregation_level);
        let type_config =
            metric_type_config_from_json(ty, self.type_config.as_ref().map(|j| &j.0));

        MetricDefinition {
            metric_id: self.metric_id,
            name: self.name,
            description: self.description.unwrap_or_default(),
            r#type: ty as i32,
            source_event_type: self.source_event_type.unwrap_or_default(),
            numerator_event_type: self.numerator_event_type.unwrap_or_default(),
            denominator_event_type: self.denominator_event_type.unwrap_or_default(),
            percentile: self.percentile.unwrap_or(0.0),
            custom_sql: self.custom_sql.unwrap_or_default(),
            lower_is_better: self.lower_is_better,
            surrogate_target_metric_id: String::new(),
            is_qoe_metric: self.is_qoe_metric,
            cuped_covariate_metric_id: self.cuped_covariate_metric_id.unwrap_or_default(),
            minimum_detectable_effect: self.minimum_detectable_effect.unwrap_or(0.0),
            stakeholder: stakeholder as i32,
            aggregation_level: aggregation_level as i32,
            type_config,
            // Lock 3 from #559 / migration 013: proto3 string default is "" so
            // legacy rows (NULL column) round-trip as empty — `unwrap_or_default`
            // matches the `custom_sql` pattern above.
            metricql_expression: self.metricql_expression.unwrap_or_default(),
        }
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
                   stakeholder, aggregation_level, type_config, metricql_expression
               ) VALUES (
                   $1, $2, $3, $4,
                   $5, $6, $7,
                   $8, $9,
                   $10, $11,
                   $12, $13,
                   $14, $15, $16, $17
               )
               RETURNING
                   metric_id, name, description, type,
                   source_event_type, numerator_event_type, denominator_event_type,
                   percentile, custom_sql,
                   lower_is_better, is_qoe_metric,
                   cuped_covariate_metric_id, minimum_detectable_effect,
                   stakeholder, aggregation_level, type_config, metricql_expression, created_at"#,
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
        // Migration 013: empty proto string → NULL in DB to satisfy the
        // metric_definitions_single_definition_source CHECK (custom_sql,
        // type_config, metricql_expression mutually exclusive).
        .bind(opt_str(&metric.metricql_expression))
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
                   stakeholder, aggregation_level, type_config, metricql_expression, created_at
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
                   stakeholder, aggregation_level, type_config, metricql_expression, created_at
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

// ---------------------------------------------------------------------------
// Tests — pure round-trip serialisation (no DB required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn synth_row(ty: &str, type_config: Option<serde_json::Value>) -> MetricRow {
        MetricRow {
            metric_id: "m1".into(),
            name: "M1".into(),
            description: None,
            r#type: ty.into(),
            source_event_type: None,
            numerator_event_type: None,
            denominator_event_type: None,
            percentile: None,
            custom_sql: None,
            lower_is_better: false,
            is_qoe_metric: false,
            cuped_covariate_metric_id: None,
            minimum_detectable_effect: None,
            stakeholder: String::new(),
            aggregation_level: String::new(),
            type_config: type_config.map(sqlx::types::Json),
            metricql_expression: None,
            created_at: Utc.timestamp_opt(0, 0).unwrap(),
        }
    }

    #[test]
    fn metric_type_to_sql_round_trip_covers_all_variants() {
        for t in [
            MetricType::Mean,
            MetricType::Proportion,
            MetricType::Ratio,
            MetricType::Count,
            MetricType::Percentile,
            MetricType::Custom,
            MetricType::FilteredMean,
            MetricType::Composite,
            MetricType::WindowedCount,
            MetricType::Metricql,
        ] {
            let s = metric_type_to_sql(t);
            assert_eq!(metric_type_from_sql(s), t, "{:?} did not round-trip via {}", t, s);
        }
    }

    #[test]
    fn filtered_mean_json_round_trip() {
        let original = MetricDefinition {
            metric_id: "fm1".into(),
            name: "FM1".into(),
            r#type: MetricType::FilteredMean as i32,
            type_config: Some(MetricTypeConfig::FilteredMean(FilteredMeanConfig {
                filter_sql: "platform = 'mobile' AND duration_ms > 5000".into(),
                value_column: "duration_ms".into(),
            })),
            ..Default::default()
        };
        let json = build_metric_type_config(&original).unwrap();
        let row = synth_row("FILTERED_MEAN", Some(json));
        let mut rebuilt = row.into_proto();
        // Fields not present in synth_row default to "M1"/"m1"; compare just type_config.
        rebuilt.metric_id = original.metric_id.clone();
        rebuilt.name = original.name.clone();
        assert_eq!(rebuilt.type_config, original.type_config);
        assert_eq!(rebuilt.r#type, original.r#type);
    }

    #[test]
    fn composite_json_round_trip() {
        let original = MetricDefinition {
            metric_id: "c1".into(),
            name: "C1".into(),
            r#type: MetricType::Composite as i32,
            type_config: Some(MetricTypeConfig::Composite(CompositeConfig {
                operator: CompositeOperator::WeightedSum as i32,
                operands: vec![
                    CompositeOperand { metric_id: "a".into(), weight: 0.7 },
                    CompositeOperand { metric_id: "b".into(), weight: 0.3 },
                ],
            })),
            ..Default::default()
        };
        let json = build_metric_type_config(&original).unwrap();
        let row = synth_row("COMPOSITE", Some(json));
        let mut rebuilt = row.into_proto();
        rebuilt.metric_id = original.metric_id.clone();
        rebuilt.name = original.name.clone();
        assert_eq!(rebuilt.type_config, original.type_config);
    }

    #[test]
    fn windowed_count_json_round_trip() {
        let original = MetricDefinition {
            metric_id: "wc1".into(),
            name: "WC1".into(),
            r#type: MetricType::WindowedCount as i32,
            type_config: Some(MetricTypeConfig::WindowedCount(WindowedCountConfig {
                event_type: "signup_completed".into(),
                filter_sql: "country = 'US'".into(),
                window_hours: 168,
            })),
            ..Default::default()
        };
        let json = build_metric_type_config(&original).unwrap();
        let row = synth_row("WINDOWED_COUNT", Some(json));
        let mut rebuilt = row.into_proto();
        rebuilt.metric_id = original.metric_id.clone();
        rebuilt.name = original.name.clone();
        assert_eq!(rebuilt.type_config, original.type_config);
    }

    #[test]
    fn legacy_mean_has_no_type_config_after_round_trip() {
        let row = synth_row("MEAN", None);
        let proto = row.into_proto();
        assert_eq!(proto.r#type, MetricType::Mean as i32);
        assert!(proto.type_config.is_none());
    }
}

// Needed only for tests' CompositeOperator reference.
#[cfg(test)]
use experimentation_proto::experimentation::common::v1::CompositeOperator;
