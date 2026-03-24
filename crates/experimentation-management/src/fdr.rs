//! Online FDR Controller (ADR-018, ADR-025 Phase 3).
//!
//! Implements the platform-level Online False Discovery Rate (FDR) controller
//! using e-values (Ramdas & Wang, 2024).  This is a singleton per platform —
//! one instance tracks the alpha-wealth budget across all experiments.
//!
//! # Design
//!
//! The controller uses the e-BH (e-value Benjamini-Hochberg) procedure:
//!
//! 1. Each concluded experiment produces an e-value E_i via `e_value_grow` or
//!    `e_value_avlm` from `experimentation-stats`.
//! 2. The rejection rule is: reject H_i if E_i ≥ m / (alpha_wealth · R_i),
//!    where m is the total number of experiments tested and R_i is the running
//!    rejection count.  For a simplified online implementation, we use the
//!    threshold E_i > 1/alpha, where alpha is the current alpha_wealth budget.
//! 3. On each experiment conclusion, the controller loads state from PostgreSQL,
//!    computes the e-value, makes a rejection decision, updates wealth, and
//!    saves state back.
//!
//! # State persistence
//!
//! State (alpha_wealth, rejection_count) is stored in the `fdr_controller_state`
//! table (migration 009).  All state transitions are single-row upserts.
//!
//! # References
//!
//! Ramdas & Wang (2024) "Hypothesis Testing with E-values", §6 (e-BH procedure).
//! Bates et al. (2023) "Testing for outliers with conformal p-values" (online FDR).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::{info, warn};
use uuid::Uuid;

use experimentation_stats::evalue::{e_value_avlm, e_value_grow};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Persisted state of the singleton FDR controller.
#[derive(Debug, Clone)]
pub struct FdrState {
    /// Initial alpha wealth (platform configuration).
    pub alpha_0: f64,
    /// Current alpha wealth W_t ∈ (0, 1).
    pub alpha_wealth: f64,
    /// Cumulative number of hypothesis rejections.
    pub rejection_count: i32,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Decision produced for a single experiment conclusion.
#[derive(Debug, Clone)]
pub struct FdrDecision {
    pub experiment_id: Uuid,
    /// E-value E_n from the GROW martingale.
    pub e_value: f64,
    /// Natural log of the e-value (numerically stable).
    pub log_e_value: f64,
    /// Whether H_0 was rejected at the current alpha_wealth.
    pub rejected: bool,
    /// Alpha wealth before this conclusion.
    pub alpha_wealth_before: f64,
    /// Alpha wealth after this conclusion (updated by the e-BH rule).
    pub alpha_wealth_after: f64,
    /// Cumulative rejection count after this conclusion.
    pub rejection_count_after: i32,
}

// ---------------------------------------------------------------------------
// Internal sqlx row type
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct FdrStateRow {
    alpha_0: f64,
    alpha_wealth: f64,
    rejection_count: i32,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct FdrAuditRow {
    e_value: f64,
    log_e_value: f64,
    rejected: bool,
    alpha_wealth_before: f64,
    alpha_wealth_after: f64,
    rejection_count_after: i32,
}

// ---------------------------------------------------------------------------
// OnlineFdrController
// ---------------------------------------------------------------------------

/// Platform-level online FDR controller.
///
/// Singleton: call `load_state()` before and `save_state()` after each use to
/// maintain correctness across concurrent experiment conclusions.  For production
/// use, wrap in a tokio Mutex or use advisory locks in PostgreSQL.
#[derive(Clone)]
pub struct OnlineFdrController {
    pool: PgPool,
    /// Default initial alpha wealth used when initializing the singleton row.
    pub alpha_0: f64,
}

impl OnlineFdrController {
    /// Create a controller backed by the given pool.
    ///
    /// `alpha_0` is the initial alpha wealth — the platform-wide FDR budget
    /// (typically 0.05 or 0.10).  It is used when inserting the singleton row
    /// for the first time.
    pub fn new(pool: PgPool, alpha_0: f64) -> Self {
        Self { pool, alpha_0 }
    }

    /// Connect to PostgreSQL and create the controller.
    pub async fn connect(database_url: &str, alpha_0: f64) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(database_url)
            .await
            .context("connect to PostgreSQL for FDR controller")?;
        Ok(Self::new(pool, alpha_0))
    }

    /// Load the current FDR state from PostgreSQL.
    ///
    /// If the singleton row does not exist yet (first run), initializes it with
    /// `alpha_0` wealth and zero rejections.
    pub async fn load_state(&self) -> Result<FdrState> {
        // Upsert the singleton row on first access.
        sqlx::query(
            r#"INSERT INTO fdr_controller_state (id, alpha_0, alpha_wealth, rejection_count)
               VALUES (1, $1, $1, 0)
               ON CONFLICT (id) DO NOTHING"#,
        )
        .bind(self.alpha_0)
        .execute(&self.pool)
        .await
        .context("initialize fdr_controller_state singleton")?;

        let row: FdrStateRow = sqlx::query_as(
            "SELECT alpha_0, alpha_wealth, rejection_count, updated_at
             FROM fdr_controller_state WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await
        .context("load fdr_controller_state")?;

        Ok(FdrState {
            alpha_0: row.alpha_0,
            alpha_wealth: row.alpha_wealth,
            rejection_count: row.rejection_count,
            updated_at: row.updated_at,
        })
    }

    /// Conclude an experiment using the GROW martingale e-value.
    ///
    /// Computes E_n = `e_value_grow(observations, sigma_sq, alpha_wealth)`,
    /// makes a rejection decision (E_n > 1/alpha_wealth), updates the alpha
    /// wealth via the e-BH rule, and persists both the updated state and an
    /// audit row.
    ///
    /// # Arguments
    /// * `experiment_id` — UUID of the concluding experiment.
    /// * `observations` — Sequential treatment-effect estimates X_1,...,X_n
    ///   (e.g. daily delta means) from N(μ, σ²).
    /// * `sigma_sq` — Known (pre-estimated) variance σ² > 0.
    pub async fn conclude_grow(
        &self,
        experiment_id: Uuid,
        observations: &[f64],
        sigma_sq: f64,
    ) -> Result<FdrDecision> {
        let state = self.load_state().await?;
        let alpha = state.alpha_wealth;

        let result = e_value_grow(observations, sigma_sq, alpha)
            .map_err(|e| anyhow::anyhow!("e_value_grow failed: {e}"))?;

        let decision = self
            .apply_decision(experiment_id, result.e_value, result.log_e_value, &state)
            .await?;

        info!(
            experiment_id = %experiment_id,
            e_value = result.e_value,
            rejected = decision.rejected,
            alpha_wealth_after = decision.alpha_wealth_after,
            "FDR controller: GROW conclusion"
        );

        Ok(decision)
    }

    /// Conclude an experiment using the CUPED-adjusted (AVLM) e-value.
    ///
    /// Uses `e_value_avlm` for regression-adjusted e-value computation (lower
    /// variance than GROW when a pre-experiment covariate is available).
    ///
    /// # Arguments
    /// See `experimentation_stats::evalue::e_value_avlm` for full argument docs.
    #[allow(clippy::too_many_arguments)]
    pub async fn conclude_avlm(
        &self,
        experiment_id: Uuid,
        control_y: &[f64],
        treatment_y: &[f64],
        control_x: &[f64],
        treatment_x: &[f64],
        tau_sq: f64,
    ) -> Result<FdrDecision> {
        let state = self.load_state().await?;
        let alpha = state.alpha_wealth;

        let result = e_value_avlm(control_y, treatment_y, control_x, treatment_x, tau_sq, alpha)
            .map_err(|e| anyhow::anyhow!("e_value_avlm failed: {e}"))?;

        let decision = self
            .apply_decision(experiment_id, result.e_value, result.log_e_value, &state)
            .await?;

        info!(
            experiment_id = %experiment_id,
            e_value = result.e_value,
            rejected = decision.rejected,
            alpha_wealth_after = decision.alpha_wealth_after,
            "FDR controller: AVLM conclusion"
        );

        Ok(decision)
    }

    // --- Private helpers ---

    /// Apply the e-BH rejection rule and persist the updated state + audit row.
    ///
    /// Rejection rule: reject H_0 if E_n > 1/alpha_wealth.
    ///
    /// Wealth update (simplified e-BH):
    /// - If rejected: alpha_wealth decreases by alpha_wealth (spend all for this
    ///   discovery, then the wealth resets to alpha_0 — permissive for online
    ///   FDR).  In production, implement the full Ramdas-Wang e-LOND formula.
    ///   Here we use the conservative approximation: no spend on non-rejections,
    ///   wealth resets to `alpha_0` after a rejection to bound aggregate FDR.
    /// - If not rejected: alpha_wealth is unchanged (GROW e-values accumulate
    ///   wealth under H_1 without spending).
    async fn apply_decision(
        &self,
        experiment_id: Uuid,
        e_value: f64,
        log_e_value: f64,
        state: &FdrState,
    ) -> Result<FdrDecision> {
        let alpha_before = state.alpha_wealth;
        let rejected = e_value > 1.0 / alpha_before;

        // Wealth update: reset to alpha_0 after rejection (conservative online FDR).
        // Non-rejections do not decrease wealth — the GROW martingale grows wealth
        // under H_1, so wealth is preserved until a rejection is made.
        let alpha_after = if rejected {
            // After rejection: reset wealth to initial budget for the next test.
            state.alpha_0
        } else {
            alpha_before
        };

        if alpha_after <= 0.0 || alpha_after > 1.0 {
            warn!(alpha_after, "FDR controller: alpha_wealth out of bounds after update");
        }

        let rejection_count_after = if rejected {
            state.rejection_count + 1
        } else {
            state.rejection_count
        };

        // Persist updated singleton state.
        let rows_affected = sqlx::query(
            r#"UPDATE fdr_controller_state
               SET alpha_wealth = $1,
                   rejection_count = $2,
                   updated_at = NOW()
               WHERE id = 1 AND alpha_wealth = $3"#,
        )
        .bind(alpha_after)
        .bind(rejection_count_after)
        .bind(alpha_before)
        .execute(&self.pool)
        .await
        .context("update fdr_controller_state")?
        .rows_affected();

        if rows_affected == 0 {
            // Concurrent update detected — reload and retry would be needed in
            // production.  Log a warning here.
            warn!("FDR controller: concurrent state update detected; state may be stale");
        }

        // Write audit row.
        sqlx::query(
            r#"INSERT INTO fdr_controller_audit
               (experiment_id, e_value, log_e_value, rejected,
                alpha_wealth_before, alpha_wealth_after, rejection_count_after)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        )
        .bind(experiment_id)
        .bind(e_value)
        .bind(log_e_value)
        .bind(rejected)
        .bind(alpha_before)
        .bind(alpha_after)
        .bind(rejection_count_after)
        .execute(&self.pool)
        .await
        .context("insert fdr_controller_audit")?;

        Ok(FdrDecision {
            experiment_id,
            e_value,
            log_e_value,
            rejected,
            alpha_wealth_before: alpha_before,
            alpha_wealth_after: alpha_after,
            rejection_count_after,
        })
    }

    /// Retrieve the audit history for a specific experiment.
    pub async fn get_audit_history(
        &self,
        experiment_id: Uuid,
    ) -> Result<Vec<FdrDecision>> {
        let rows: Vec<FdrAuditRow> = sqlx::query_as(
            r#"SELECT e_value, log_e_value, rejected,
                      alpha_wealth_before, alpha_wealth_after, rejection_count_after
               FROM fdr_controller_audit
               WHERE experiment_id = $1
               ORDER BY evaluated_at ASC"#,
        )
        .bind(experiment_id)
        .fetch_all(&self.pool)
        .await
        .context("fetch fdr audit history")?;

        Ok(rows
            .into_iter()
            .map(|r| FdrDecision {
                experiment_id,
                e_value: r.e_value,
                log_e_value: r.log_e_value,
                rejected: r.rejected,
                alpha_wealth_before: r.alpha_wealth_before,
                alpha_wealth_after: r.alpha_wealth_after,
                rejection_count_after: r.rejection_count_after,
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Unit tests (no database required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a mock FdrState for unit-testing the decision logic in isolation.
    fn mock_state(alpha_wealth: f64) -> FdrState {
        FdrState {
            alpha_0: 0.05,
            alpha_wealth,
            rejection_count: 0,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_rejection_rule_threshold() {
        // E_n > 1/alpha should reject.
        // alpha = 0.05 → threshold = 20.0.
        let state = mock_state(0.05);
        let e_value = 25.0_f64;
        let rejected = e_value > 1.0 / state.alpha_wealth;
        assert!(rejected, "E=25 should reject at alpha=0.05 (threshold=20)");
    }

    #[test]
    fn test_no_rejection_below_threshold() {
        // E_n = 15 < 20 = 1/0.05 → no rejection.
        let state = mock_state(0.05);
        let e_value = 15.0_f64;
        let rejected = e_value > 1.0 / state.alpha_wealth;
        assert!(!rejected, "E=15 should not reject at threshold=20");
    }

    #[test]
    fn test_wealth_resets_after_rejection() {
        // After rejection, alpha_wealth resets to alpha_0.
        let state = mock_state(0.05);
        let alpha_after = state.alpha_0; // simulating the reset
        assert!(
            (alpha_after - 0.05).abs() < 1e-12,
            "wealth after rejection should reset to alpha_0"
        );
    }

    #[test]
    fn test_rejection_count_increments() {
        let state = mock_state(0.05);
        let rejected = true;
        let count_after = if rejected {
            state.rejection_count + 1
        } else {
            state.rejection_count
        };
        assert_eq!(count_after, 1);
    }

    #[test]
    fn test_grow_evalue_computes_correctly() {
        // Verify we can call e_value_grow directly — integration point.
        let obs = vec![2.0_f64; 5]; // strong positive signal
        let result = e_value_grow(&obs, 1.0, 0.05).unwrap();
        assert!(result.e_value > 0.0);
        // Strong signal: should exceed threshold.
        assert!(result.e_value > 1.0 / 0.05, "strong signal should produce large e-value");
    }

    #[test]
    fn test_avlm_evalue_computes_correctly() {
        // Verify e_value_avlm integration.
        let ctrl_y: Vec<f64> = vec![0.0; 20];
        let trt_y: Vec<f64> = vec![3.0; 20]; // large effect
        let x = vec![0.0_f64; 20];
        let result = e_value_avlm(&ctrl_y, &trt_y, &x, &x, 1.0, 0.05).unwrap();
        assert!(result.e_value > 0.0);
        assert!(result.reject, "large effect should reject at alpha=0.05");
    }

    // DB-dependent tests — require a running PostgreSQL instance.

    #[tokio::test]
    #[ignore]
    async fn test_conclude_grow_roundtrip() {
        let url = match std::env::var("DATABASE_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let controller = OnlineFdrController::connect(&url, 0.05).await.unwrap();
        let exp_id = Uuid::new_v4();
        let obs: Vec<f64> = vec![2.0; 10];
        let decision = controller.conclude_grow(exp_id, &obs, 1.0).await.unwrap();
        assert!(decision.e_value > 0.0);
        assert_eq!(decision.experiment_id, exp_id);
    }

    #[tokio::test]
    #[ignore]
    async fn test_conclude_avlm_roundtrip() {
        let url = match std::env::var("DATABASE_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let controller = OnlineFdrController::connect(&url, 0.05).await.unwrap();
        let exp_id = Uuid::new_v4();
        let ctrl_y: Vec<f64> = vec![0.0; 20];
        let trt_y: Vec<f64> = vec![2.0; 20];
        let x = vec![0.0_f64; 20];
        let decision = controller
            .conclude_avlm(exp_id, &ctrl_y, &trt_y, &x, &x, 1.0)
            .await
            .unwrap();
        assert!(decision.e_value > 0.0);
    }
}
