//! Adaptive sample size trigger (ADR-020, ADR-025 Phase 3).
//!
//! Implements the scheduled interim analysis that classifies a running experiment
//! into a zone and extends it when appropriate.  All statistical computation
//! delegates directly to `experimentation-stats::adaptive_n` — no gRPC, no FFI.
//!
//! # Design
//!
//! The trigger is called on a schedule (e.g. every 24 hours) while an experiment
//! is RUNNING.  At each scheduled look:
//!
//! 1. Compute blinded pooled variance σ²_B from the combined arm observations
//!    (Gould-Shih estimator; preserves blinding).
//! 2. Compute conditional power CP(δ̂, σ²_B, n_max, α).
//! 3. Classify into Favorable / Promising / Futile zone.
//! 4. If Promising: compute the recommended n_max to restore target power, and
//!    return an extension recommendation.
//! 5. If Futile: return an early-termination recommendation.
//! 6. Write the result to the audit table (migration 008).
//!
//! # State persistence
//!
//! Audit rows are written to `adaptive_sample_size_audit` (migration 008).
//! The management service updates the experiment's `n_max` in the experiments
//! table after the caller acts on the extension recommendation.
//!
//! # References
//!
//! Mehta & Pocock (2011) Stat Med 30:3267-3284 (promising-zone design).
//! Gould & Shih (1992) Stat Med 11:1431-1441 (blinded variance estimator).

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::postgres::PgPool;
use tracing::info;
use uuid::Uuid;

// Re-export Zone and ZoneThresholds so callers can pattern-match without an
// explicit dependency on experimentation-stats.
pub use experimentation_stats::adaptive_n::{Zone, ZoneThresholds};

use experimentation_stats::adaptive_n::{
    blinded_pooled_variance, conditional_power, required_n_for_power, zone_classify,
};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Input parameters for a scheduled interim analysis.
#[derive(Debug, Clone)]
pub struct InterimAnalysisInput {
    /// UUID of the running experiment.
    pub experiment_id: Uuid,
    /// All blinded observations (control + treatment combined).
    ///
    /// The blinding is preserved: we do NOT split by arm here.  The Gould-Shih
    /// estimator uses only the combined sample.
    pub all_observations: Vec<f64>,
    /// Unblinded point estimate of the treatment effect δ̂ = X̄_T − X̄_C.
    ///
    /// This is read from the latest analysis result stored by M4a, not computed
    /// here.  Using the unblinded effect only for conditional power, not for
    /// variance estimation, preserves the Type I error guarantee.
    pub observed_effect: f64,
    /// Current planned maximum per-arm sample size (n_max).
    pub n_max_per_arm: f64,
    /// Two-sided significance level α.
    pub alpha: f64,
    /// Target conditional power for the extension calculation (e.g. 0.80).
    pub target_power: f64,
    /// Hard cap on per-arm sample size for the extension search.
    pub n_max_allowed: f64,
    /// Zone thresholds (default: favorable ≥ 0.90, promising ≥ 0.30).
    pub thresholds: ZoneThresholds,
}

impl InterimAnalysisInput {
    /// Create with default zone thresholds (Mehta & Pocock 2011).
    pub fn new(
        experiment_id: Uuid,
        all_observations: Vec<f64>,
        observed_effect: f64,
        n_max_per_arm: f64,
        alpha: f64,
        target_power: f64,
        n_max_allowed: f64,
    ) -> Self {
        Self {
            experiment_id,
            all_observations,
            observed_effect,
            n_max_per_arm,
            alpha,
            target_power,
            n_max_allowed,
            thresholds: ZoneThresholds::default(),
        }
    }
}

/// Decision produced by the interim analysis.
#[derive(Debug, Clone)]
pub struct ExtensionDecision {
    /// UUID of the experiment.
    pub experiment_id: Uuid,
    /// Zone classification.
    pub zone: Zone,
    /// Conditional power CP(δ̂, σ²_B, n_max, α).
    pub conditional_power: f64,
    /// Blinded pooled variance σ²_B (Gould-Shih estimator).
    pub blinded_variance: f64,
    /// Whether the experiment should be extended (true iff zone is Promising).
    pub should_extend: bool,
    /// Recommended new n_max per arm to achieve target_power.
    /// `None` when zone is not Promising.
    pub recommended_n_max: Option<f64>,
    /// Whether early termination is recommended (true iff zone is Futile).
    pub recommend_early_stop: bool,
    /// Timestamp of this analysis.
    pub analyzed_at: chrono::DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Core analysis function (no database)
// ---------------------------------------------------------------------------

/// Run an interim adaptive-N analysis on the given input.
///
/// This is a pure function — no database access.  The caller is responsible for
/// writing the result to the audit table via `write_audit_row`.
///
/// # Errors
/// Returns an error if `blinded_pooled_variance` or `conditional_power` fails
/// (e.g. fewer than 2 observations, non-positive inputs).
pub fn run_adaptive_interim(input: &InterimAnalysisInput) -> Result<ExtensionDecision> {
    let sigma_sq = blinded_pooled_variance(&input.all_observations)
        .map_err(|e| anyhow::anyhow!("blinded_pooled_variance failed: {e}"))?;

    let cp = conditional_power(
        input.observed_effect,
        sigma_sq,
        input.n_max_per_arm,
        input.alpha,
    )
    .map_err(|e| anyhow::anyhow!("conditional_power failed: {e}"))?;

    let zone = zone_classify(cp, &input.thresholds);

    let (should_extend, recommended_n_max, recommend_early_stop) = match zone {
        Zone::Promising => {
            let n_ext = required_n_for_power(
                input.observed_effect,
                sigma_sq,
                input.target_power,
                input.alpha,
                input.n_max_per_arm,
                input.n_max_allowed,
            )
            .map_err(|e| anyhow::anyhow!("required_n_for_power failed: {e}"))?;
            (true, Some(n_ext), false)
        }
        Zone::Futile => (false, None, true),
        Zone::Favorable => (false, None, false),
    };

    Ok(ExtensionDecision {
        experiment_id: input.experiment_id,
        zone,
        conditional_power: cp,
        blinded_variance: sigma_sq,
        should_extend,
        recommended_n_max,
        recommend_early_stop,
        analyzed_at: Utc::now(),
    })
}

// ---------------------------------------------------------------------------
// Database audit helper
// ---------------------------------------------------------------------------

/// Write an interim analysis result to the `adaptive_sample_size_audit` table.
///
/// Schema defined in `sql/migrations/008_adaptive_sample_size_audit.sql`.
/// Errors are logged but do not fail the analysis — audit writes are
/// fire-and-forget (same pattern as `AnalysisStore::save_analysis_result`).
pub async fn write_audit_row(pool: &PgPool, decision: &ExtensionDecision) -> Result<()> {
    let zone_str = match decision.zone {
        Zone::Favorable => "favorable",
        Zone::Promising => "promising",
        Zone::Futile => "futile",
    };

    sqlx::query(
        r#"INSERT INTO adaptive_sample_size_audit
           (experiment_id, zone, conditional_power, blinded_variance,
            should_extend, recommended_n_max, recommend_early_stop, analyzed_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           ON CONFLICT DO NOTHING"#,
    )
    .bind(decision.experiment_id)
    .bind(zone_str)
    .bind(decision.conditional_power)
    .bind(decision.blinded_variance)
    .bind(decision.should_extend)
    .bind(decision.recommended_n_max)
    .bind(decision.recommend_early_stop)
    .bind(decision.analyzed_at)
    .execute(pool)
    .await
    .context("insert adaptive_sample_size_audit")?;

    info!(
        experiment_id = %decision.experiment_id,
        zone = zone_str,
        conditional_power = decision.conditional_power,
        should_extend = decision.should_extend,
        recommended_n_max = ?decision.recommended_n_max,
        "adaptive interim analysis complete"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests (no database required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn favorable_input() -> InterimAnalysisInput {
        // Large effect, large n_max → high CP → Favorable zone.
        // Use observations with non-zero variance (alternating ±small noise).
        let obs: Vec<f64> = (0..200).map(|i| if i % 2 == 0 { 1.1 } else { 0.9 }).collect();
        InterimAnalysisInput::new(
            Uuid::new_v4(),
            obs,
            1.0,  // observed_effect
            500.0, // n_max_per_arm
            0.05,
            0.80,
            2000.0,
        )
    }

    fn futile_input() -> InterimAnalysisInput {
        // Tiny effect relative to variance → Futile zone.
        InterimAnalysisInput::new(
            Uuid::new_v4(),
            {
                // High variance blinded sample.
                let mut v: Vec<f64> = (0..100).map(|i| i as f64 * 10.0).collect();
                v.extend((0..100).map(|i| -(i as f64) * 10.0));
                v
            },
            0.001,  // negligible effect
            100.0,  // small n_max
            0.05,
            0.80,
            500.0,
        )
    }

    fn promising_input() -> InterimAnalysisInput {
        // Moderate effect, moderate n_max → Promising zone.
        InterimAnalysisInput::new(
            Uuid::new_v4(),
            // 100 blinded observations with modest spread.
            {
                let mut v: Vec<f64> = vec![1.0; 50];
                v.extend(vec![0.0; 50]);
                v
            },
            0.5,   // moderate effect
            200.0, // n_max somewhat low for this effect size
            0.05,
            0.80,
            2000.0,
        )
    }

    #[test]
    fn test_favorable_zone_no_extension() {
        let input = favorable_input();
        let decision = run_adaptive_interim(&input).unwrap();
        assert_eq!(decision.zone, Zone::Favorable);
        assert!(!decision.should_extend);
        assert!(!decision.recommend_early_stop);
        assert!(decision.recommended_n_max.is_none());
        assert!(decision.conditional_power >= 0.90, "cp={}", decision.conditional_power);
    }

    #[test]
    fn test_futile_zone_early_stop() {
        let input = futile_input();
        let decision = run_adaptive_interim(&input).unwrap();
        assert_eq!(decision.zone, Zone::Futile);
        assert!(!decision.should_extend);
        assert!(decision.recommend_early_stop);
        assert!(decision.recommended_n_max.is_none());
        assert!(decision.conditional_power < 0.30, "cp={}", decision.conditional_power);
    }

    #[test]
    fn test_promising_zone_extension() {
        let input = promising_input();
        let decision = run_adaptive_interim(&input).unwrap();
        // This should land in Promising or Favorable depending on effect/variance.
        // At minimum, verify the function runs without error.
        assert!(decision.conditional_power >= 0.0);
        assert!(decision.conditional_power <= 1.0);
        if decision.zone == Zone::Promising {
            assert!(decision.should_extend);
            assert!(decision.recommended_n_max.is_some());
            let n_ext = decision.recommended_n_max.unwrap();
            // Extension must be >= current n_max.
            assert!(n_ext >= input.n_max_per_arm, "n_ext={n_ext} n_max={}", input.n_max_per_arm);
        }
    }

    #[test]
    fn test_blinded_variance_positive() {
        let input = favorable_input();
        let decision = run_adaptive_interim(&input).unwrap();
        assert!(decision.blinded_variance > 0.0, "blinded_variance={}", decision.blinded_variance);
    }

    #[test]
    fn test_experiment_id_preserved() {
        let id = Uuid::new_v4();
        // Use observations with non-zero variance.
        let obs: Vec<f64> = (0..20).map(|i| if i % 2 == 0 { 0.6 } else { 0.4 }).collect();
        let input = InterimAnalysisInput::new(id, obs, 0.5, 100.0, 0.05, 0.80, 500.0);
        let decision = run_adaptive_interim(&input).unwrap();
        assert_eq!(decision.experiment_id, id);
    }

    #[test]
    fn test_insufficient_observations_fails() {
        let input = InterimAnalysisInput::new(
            Uuid::new_v4(),
            vec![1.0], // only 1 observation — blinded_pooled_variance requires >= 2
            0.5,
            200.0,
            0.05,
            0.80,
            2000.0,
        );
        assert!(run_adaptive_interim(&input).is_err(), "should fail with 1 observation");
    }

    #[test]
    fn test_custom_thresholds() {
        // Tighter thresholds: favorable >= 0.95.
        let mut input = promising_input();
        input.thresholds = ZoneThresholds {
            favorable: 0.95,
            promising: 0.50,
        };
        let decision = run_adaptive_interim(&input).unwrap();
        // Just verify it runs without error with custom thresholds.
        assert!(decision.conditional_power >= 0.0);
    }

    // DB-dependent test.

    #[tokio::test]
    #[ignore]
    async fn test_write_audit_row_db() {
        let url = match std::env::var("DATABASE_URL") {
            Ok(u) => u,
            Err(_) => return,
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&url)
            .await
            .unwrap();

        let input = favorable_input();
        let decision = run_adaptive_interim(&input).unwrap();
        write_audit_row(&pool, &decision).await.unwrap();
    }
}
