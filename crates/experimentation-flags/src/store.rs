//! PostgreSQL store for feature flags — sqlx async, runtime queries.
//!
//! Schema: sql/migrations/002_feature_flags.sql, 004_flag_experiment_linkage.sql,
//!         005_flag_resolved_at.sql.
//!
//! Uses sqlx::query / sqlx::query_as (no compile-time macro checking) to match
//! the project's existing pattern (see experimentation-analysis/src/store.rs).

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Domain model for a feature flag, mirroring the Go `store.Flag`.
#[derive(Debug, Clone)]
pub struct Flag {
    pub flag_id: Uuid,
    pub name: String,
    pub description: String,
    /// "BOOLEAN" | "STRING" | "NUMERIC" | "JSON"
    pub flag_type: String,
    pub default_value: String,
    pub enabled: bool,
    pub rollout_percentage: f64,
    /// Per-flag hash salt for deterministic bucketing.
    pub salt: String,
    pub targeting_rule_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub promoted_experiment_id: Option<Uuid>,
    pub promoted_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub variants: Vec<FlagVariant>,
}

#[derive(Debug, Clone)]
pub struct FlagVariant {
    pub variant_id: Uuid,
    pub flag_id: Uuid,
    pub value: String,
    pub traffic_fraction: f64,
    pub ordinal: i32,
}

// ---------------------------------------------------------------------------
// Internal row types for sqlx::query_as
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct FlagRow {
    flag_id: Uuid,
    name: String,
    description: String,
    flag_type: String,
    default_value: String,
    enabled: bool,
    rollout_percentage: f64,
    salt: String,
    targeting_rule_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    promoted_experiment_id: Option<Uuid>,
    promoted_at: Option<DateTime<Utc>>,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct VariantRow {
    variant_id: Uuid,
    flag_id: Uuid,
    value: String,
    traffic_fraction: f64,
    ordinal: i32,
}

impl From<FlagRow> for Flag {
    fn from(r: FlagRow) -> Self {
        Flag {
            flag_id: r.flag_id,
            name: r.name,
            description: r.description,
            flag_type: r.flag_type,
            default_value: r.default_value,
            enabled: r.enabled,
            rollout_percentage: r.rollout_percentage,
            salt: r.salt,
            targeting_rule_id: r.targeting_rule_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
            promoted_experiment_id: r.promoted_experiment_id,
            promoted_at: r.promoted_at,
            resolved_at: r.resolved_at,
            variants: Vec::new(),
        }
    }
}

impl From<VariantRow> for FlagVariant {
    fn from(r: VariantRow) -> Self {
        FlagVariant {
            variant_id: r.variant_id,
            flag_id: r.flag_id,
            value: r.value,
            traffic_fraction: r.traffic_fraction,
            ordinal: r.ordinal,
        }
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FlagStore {
    pool: PgPool,
}

impl FlagStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .connect(database_url)
            .await
            .context("connect to PostgreSQL")?;
        Ok(Self { pool })
    }

    /// Return the underlying pool (needed to share it with AuditStore and gRPC serve).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Build a FlagStore from an existing pool (no new connection, clones the Arc).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    // --- CRUD ---

    pub async fn create_flag(&self, f: &Flag) -> Result<Flag, StoreError> {
        let mut tx = self.pool.begin().await.map_err(StoreError::Db)?;

        let row: FlagRow = sqlx::query_as(
            r#"INSERT INTO feature_flags
                   (name, description, type, default_value, enabled, rollout_percentage, targeting_rule_id)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING
                   flag_id, name, description,
                   type AS flag_type,
                   default_value, enabled, rollout_percentage, salt, targeting_rule_id,
                   created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at"#,
        )
        .bind(&f.name)
        .bind(&f.description)
        .bind(&f.flag_type)
        .bind(&f.default_value)
        .bind(f.enabled)
        .bind(f.rollout_percentage)
        .bind(f.targeting_rule_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                StoreError::AlreadyExists(f.name.clone())
            } else {
                StoreError::Db(e)
            }
        })?;

        let created_id = row.flag_id;

        for (ordinal, v) in f.variants.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO flag_variants (flag_id, value, traffic_fraction, ordinal)
                   VALUES ($1, $2, $3, $4)"#,
            )
            .bind(created_id)
            .bind(&v.value)
            .bind(v.traffic_fraction)
            .bind(ordinal as i32)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Db)?;
        }

        tx.commit().await.map_err(StoreError::Db)?;
        self.get_flag(created_id).await
    }

    pub async fn get_flag(&self, flag_id: Uuid) -> Result<Flag, StoreError> {
        let row: Option<FlagRow> = sqlx::query_as(
            r#"SELECT flag_id, name, description,
                      type AS flag_type,
                      default_value, enabled, rollout_percentage, salt, targeting_rule_id,
                      created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
               FROM feature_flags WHERE flag_id = $1"#,
        )
        .bind(flag_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        let row = row.ok_or_else(|| StoreError::NotFound(flag_id.to_string()))?;
        let mut flag = Flag::from(row);
        flag.variants = self.get_variants(flag_id).await?;
        Ok(flag)
    }

    pub async fn update_flag(&self, f: &Flag) -> Result<Flag, StoreError> {
        let mut tx = self.pool.begin().await.map_err(StoreError::Db)?;

        let rows_affected = sqlx::query(
            r#"UPDATE feature_flags
               SET name = $2, description = $3, type = $4, default_value = $5,
                   enabled = $6, rollout_percentage = $7, targeting_rule_id = $8,
                   updated_at = NOW()
               WHERE flag_id = $1"#,
        )
        .bind(f.flag_id)
        .bind(&f.name)
        .bind(&f.description)
        .bind(&f.flag_type)
        .bind(&f.default_value)
        .bind(f.enabled)
        .bind(f.rollout_percentage)
        .bind(f.targeting_rule_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                StoreError::AlreadyExists(f.name.clone())
            } else {
                StoreError::Db(e)
            }
        })?
        .rows_affected();

        if rows_affected == 0 {
            return Err(StoreError::NotFound(f.flag_id.to_string()));
        }

        sqlx::query("DELETE FROM flag_variants WHERE flag_id = $1")
            .bind(f.flag_id)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Db)?;

        for (ordinal, v) in f.variants.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO flag_variants (flag_id, value, traffic_fraction, ordinal)
                   VALUES ($1, $2, $3, $4)"#,
            )
            .bind(f.flag_id)
            .bind(&v.value)
            .bind(v.traffic_fraction)
            .bind(ordinal as i32)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::Db)?;
        }

        tx.commit().await.map_err(StoreError::Db)?;
        self.get_flag(f.flag_id).await
    }

    pub async fn delete_flag(&self, flag_id: Uuid) -> Result<(), StoreError> {
        let rows_affected = sqlx::query("DELETE FROM feature_flags WHERE flag_id = $1")
            .bind(flag_id)
            .execute(&self.pool)
            .await
            .map_err(StoreError::Db)?
            .rows_affected();

        if rows_affected == 0 {
            return Err(StoreError::NotFound(flag_id.to_string()));
        }
        Ok(())
    }

    /// Cursor-based pagination — page_token is base64(flag_id UUID string).
    pub async fn list_flags(
        &self,
        page_size: i64,
        page_token: &str,
    ) -> Result<(Vec<Flag>, String), StoreError> {
        let page_size = if page_size <= 0 || page_size > 100 {
            50i64
        } else {
            page_size
        };

        let col = r#"flag_id, name, description,
                     type AS flag_type,
                     default_value, enabled, rollout_percentage, salt, targeting_rule_id,
                     created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at"#;

        let rows: Vec<FlagRow> = if page_token.is_empty() {
            sqlx::query_as(&format!(
                "SELECT {col} FROM feature_flags ORDER BY flag_id LIMIT $1"
            ))
            .bind(page_size + 1)
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::Db)?
        } else {
            let cursor = B64
                .decode(page_token)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .and_then(|s| Uuid::parse_str(&s).ok())
                .ok_or(StoreError::InvalidPageToken)?;

            sqlx::query_as(&format!(
                "SELECT {col} FROM feature_flags WHERE flag_id > $1 ORDER BY flag_id LIMIT $2"
            ))
            .bind(cursor)
            .bind(page_size + 1)
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::Db)?
        };

        let mut flags: Vec<Flag> = rows.into_iter().map(Flag::from).collect();

        let next_token = if flags.len() as i64 > page_size {
            let last_id = flags[(page_size - 1) as usize].flag_id;
            flags.truncate(page_size as usize);
            B64.encode(last_id.to_string().as_bytes())
        } else {
            String::new()
        };

        for flag in &mut flags {
            flag.variants = self.get_variants(flag.flag_id).await?;
        }

        Ok((flags, next_token))
    }

    pub async fn get_all_enabled_flags(&self) -> Result<Vec<Flag>, StoreError> {
        let rows: Vec<FlagRow> = sqlx::query_as(
            r#"SELECT flag_id, name, description,
                      type AS flag_type,
                      default_value, enabled, rollout_percentage, salt, targeting_rule_id,
                      created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
               FROM feature_flags WHERE enabled = TRUE ORDER BY flag_id"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        let mut flags: Vec<Flag> = rows.into_iter().map(Flag::from).collect();
        for flag in &mut flags {
            flag.variants = self.get_variants(flag.flag_id).await?;
        }
        Ok(flags)
    }

    // --- Flag-experiment linkage ---

    pub async fn link_flag_to_experiment(
        &self,
        flag_id: Uuid,
        experiment_id: Uuid,
    ) -> Result<(), StoreError> {
        let rows_affected = sqlx::query(
            r#"UPDATE feature_flags
               SET promoted_experiment_id = $2, promoted_at = NOW(), updated_at = NOW()
               WHERE flag_id = $1"#,
        )
        .bind(flag_id)
        .bind(experiment_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::Db)?
        .rows_affected();

        if rows_affected == 0 {
            return Err(StoreError::NotFound(flag_id.to_string()));
        }
        Ok(())
    }

    /// Return the flag promoted to a given experiment, if any.
    pub async fn get_flag_by_experiment(
        &self,
        experiment_id: Uuid,
    ) -> Result<Flag, StoreError> {
        let row: Option<FlagRow> = sqlx::query_as(
            r#"SELECT flag_id, name, description,
                      type AS flag_type,
                      default_value, enabled, rollout_percentage, salt, targeting_rule_id,
                      created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
               FROM feature_flags WHERE promoted_experiment_id = $1"#,
        )
        .bind(experiment_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        let row = row.ok_or_else(|| {
            StoreError::NotFound(format!("no flag for experiment {experiment_id}"))
        })?;
        let mut flag = Flag::from(row);
        flag.variants = self.get_variants(flag.flag_id).await?;
        Ok(flag)
    }

    /// Return all flags that reference a given targeting rule.
    pub async fn get_flags_by_targeting_rule(
        &self,
        targeting_rule_id: Uuid,
    ) -> Result<Vec<Flag>, StoreError> {
        let rows: Vec<FlagRow> = sqlx::query_as(
            r#"SELECT flag_id, name, description,
                      type AS flag_type,
                      default_value, enabled, rollout_percentage, salt, targeting_rule_id,
                      created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
               FROM feature_flags WHERE targeting_rule_id = $1 ORDER BY name"#,
        )
        .bind(targeting_rule_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        let mut flags: Vec<Flag> = rows.into_iter().map(Flag::from).collect();
        for flag in &mut flags {
            flag.variants = self.get_variants(flag.flag_id).await?;
        }
        Ok(flags)
    }

    /// Return all flags that have been promoted to experiments (promoted_experiment_id IS NOT NULL).
    pub async fn get_promoted_flags(&self) -> Result<Vec<Flag>, StoreError> {
        let rows: Vec<FlagRow> = sqlx::query_as(
            r#"SELECT flag_id, name, description,
                      type AS flag_type,
                      default_value, enabled, rollout_percentage, salt, targeting_rule_id,
                      created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
               FROM feature_flags
               WHERE promoted_experiment_id IS NOT NULL
               ORDER BY promoted_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        let mut flags: Vec<Flag> = rows.into_iter().map(Flag::from).collect();
        for flag in &mut flags {
            flag.variants = self.get_variants(flag.flag_id).await?;
        }
        Ok(flags)
    }

    /// Resolve a promoted flag: apply action, set resolved_at = NOW().
    pub async fn resolve_flag(
        &self,
        flag_id: Uuid,
        action: crate::reconciler::ResolutionAction,
    ) -> Result<(), StoreError> {
        use crate::reconciler::ResolutionAction;
        let sql = match action {
            ResolutionAction::RolloutFull => {
                r#"UPDATE feature_flags
                   SET rollout_percentage = 1.0, enabled = TRUE,
                       resolved_at = NOW(), updated_at = NOW()
                   WHERE flag_id = $1"#
            }
            ResolutionAction::Rollback => {
                r#"UPDATE feature_flags
                   SET rollout_percentage = 0.0, enabled = FALSE,
                       resolved_at = NOW(), updated_at = NOW()
                   WHERE flag_id = $1"#
            }
            ResolutionAction::Keep => {
                r#"UPDATE feature_flags
                   SET resolved_at = NOW(), updated_at = NOW()
                   WHERE flag_id = $1"#
            }
        };

        let rows_affected = sqlx::query(sql)
            .bind(flag_id)
            .execute(&self.pool)
            .await
            .map_err(StoreError::Db)?
            .rows_affected();

        if rows_affected == 0 {
            return Err(StoreError::NotFound(flag_id.to_string()));
        }
        Ok(())
    }

    // --- Private helpers ---

    async fn get_variants(&self, flag_id: Uuid) -> Result<Vec<FlagVariant>, StoreError> {
        let rows: Vec<VariantRow> = sqlx::query_as(
            r#"SELECT variant_id, flag_id, value, traffic_fraction, ordinal
               FROM flag_variants WHERE flag_id = $1 ORDER BY ordinal"#,
        )
        .bind(flag_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::Db)?;

        Ok(rows.into_iter().map(FlagVariant::from).collect())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("flag not found: {0}")]
    NotFound(String),
    #[error("flag already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid page token")]
    InvalidPageToken,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}
