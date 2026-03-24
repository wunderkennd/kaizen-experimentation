//! Portfolio allocation enrichment (ADR-019, ADR-025 Phase 3).
//!
//! When `GetPortfolioAllocation` is called, this module enriches the response
//! with per-experiment power recommendations and annualized impact estimates.
//!
//! All statistical computation delegates to `experimentation-stats` directly:
//! - Alpha allocation: `experimentation_stats::portfolio::optimal_alpha`
//! - Annualized impact: `experimentation_stats::portfolio::annualized_impact`
//! - Conditional power: `experimentation_stats::adaptive_n::conditional_power`
//!
//! # Design
//!
//! The `enrich_portfolio_allocation` function takes a list of running experiments
//! with their current statistical state and returns power recommendations and
//! annualized impact projections for each.  This data powers the M6 portfolio
//! dashboard.

use anyhow::Result;
use uuid::Uuid;

use experimentation_stats::adaptive_n::conditional_power;
use experimentation_stats::portfolio::{annualized_impact, optimal_alpha};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Statistical snapshot of one running experiment, required for portfolio analysis.
#[derive(Debug, Clone)]
pub struct ExperimentPortfolioEntry {
    /// Unique identifier for the experiment.
    pub experiment_id: Uuid,
    /// Current point estimate of the treatment effect (per-user-per-day).
    pub effect_estimate: f64,
    /// Blinded pooled variance estimate (σ²_B).
    pub blinded_sigma_sq: f64,
    /// Current per-arm sample size (n_current).
    pub n_per_arm: f64,
    /// Planned maximum per-arm sample size (n_max).
    pub n_max_per_arm: f64,
    /// Number of daily active users in the target population (post-ship).
    pub daily_users: f64,
    /// Experiment duration in calendar days.
    pub duration_days: f64,
}

/// Enriched portfolio allocation result for a single experiment.
#[derive(Debug, Clone)]
pub struct PortfolioAllocationResult {
    /// Unique identifier for the experiment.
    pub experiment_id: Uuid,
    /// Recommended per-experiment significance level (Bonferroni-corrected).
    pub recommended_alpha: f64,
    /// Conditional power at the planned n_max given the current effect estimate.
    pub conditional_power: f64,
    /// Projected annualized business impact if the effect is real and ships.
    pub annualized_impact: f64,
}

// ---------------------------------------------------------------------------
// Core function
// ---------------------------------------------------------------------------

/// Enrich a portfolio allocation response with power recommendations and impact.
///
/// For each experiment in `entries`:
/// 1. Computes the Bonferroni-corrected alpha = alpha_budget / n (where n is
///    the total number of experiments in the portfolio).
/// 2. Computes conditional power at `n_max_per_arm` using the current blinded
///    variance and observed effect estimate.
/// 3. Computes annualized impact = effect * daily_users * 365.
///
/// # Arguments
/// * `entries` — Snapshot of all running experiments in the portfolio.
/// * `alpha_budget` — Platform-wide significance budget (e.g. 0.05).
///
/// # Errors
/// Returns an error if any statistical computation fails (e.g. negative variance,
/// invalid alpha_budget).  Partial failures are not supported — all-or-nothing.
///
/// # Returns
/// One `PortfolioAllocationResult` per input entry, in the same order.
pub fn enrich_portfolio_allocation(
    entries: &[ExperimentPortfolioEntry],
    alpha_budget: f64,
) -> Result<Vec<PortfolioAllocationResult>> {
    let n = entries.len();

    // optimal_alpha requires n >= 1.
    let per_experiment_alpha = optimal_alpha(alpha_budget, n.max(1))
        .map_err(|e| anyhow::anyhow!("optimal_alpha failed: {e}"))?;

    let mut results = Vec::with_capacity(n);

    for entry in entries {
        let cp = conditional_power(
            entry.effect_estimate,
            entry.blinded_sigma_sq,
            entry.n_max_per_arm,
            per_experiment_alpha,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "conditional_power failed for experiment {}: {e}",
                entry.experiment_id
            )
        })?;

        let impact = annualized_impact(entry.effect_estimate, entry.daily_users, entry.duration_days)
            .map_err(|e| {
                anyhow::anyhow!(
                    "annualized_impact failed for experiment {}: {e}",
                    entry.experiment_id
                )
            })?;

        results.push(PortfolioAllocationResult {
            experiment_id: entry.experiment_id,
            recommended_alpha: per_experiment_alpha,
            conditional_power: cp,
            annualized_impact: impact,
        });
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(
        effect: f64,
        sigma_sq: f64,
        n_per_arm: f64,
        n_max: f64,
        daily_users: f64,
    ) -> ExperimentPortfolioEntry {
        ExperimentPortfolioEntry {
            experiment_id: Uuid::new_v4(),
            effect_estimate: effect,
            blinded_sigma_sq: sigma_sq,
            n_per_arm,
            n_max_per_arm: n_max,
            daily_users,
            duration_days: 14.0,
        }
    }

    #[test]
    fn test_enrich_empty_portfolio() {
        // Empty portfolio: n=0 → treated as n=1 to avoid division by zero.
        // optimal_alpha(0.05, 1) = 0.05.
        let results = enrich_portfolio_allocation(&[], 0.05).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_enrich_single_experiment_alpha() {
        let entry = make_entry(0.5, 1.0, 100.0, 200.0, 1_000_000.0);
        let results = enrich_portfolio_allocation(&[entry], 0.05).unwrap();
        assert_eq!(results.len(), 1);
        // Single experiment: alpha = 0.05 / 1 = 0.05.
        assert!((results[0].recommended_alpha - 0.05).abs() < 1e-12);
    }

    #[test]
    fn test_enrich_five_experiments_bonferroni() {
        let entries: Vec<_> = (0..5)
            .map(|_| make_entry(0.5, 1.0, 100.0, 200.0, 1_000_000.0))
            .collect();
        let results = enrich_portfolio_allocation(&entries, 0.05).unwrap();
        assert_eq!(results.len(), 5);
        // Bonferroni: alpha_per = 0.05 / 5 = 0.01.
        for r in &results {
            assert!((r.recommended_alpha - 0.01).abs() < 1e-12);
        }
    }

    #[test]
    fn test_enrich_conditional_power_in_range() {
        let entry = make_entry(0.5, 1.0, 50.0, 200.0, 500_000.0);
        let results = enrich_portfolio_allocation(&[entry], 0.05).unwrap();
        let cp = results[0].conditional_power;
        assert!(
            (0.0..=1.0).contains(&cp),
            "conditional power must be in [0, 1]: cp={cp}"
        );
    }

    #[test]
    fn test_enrich_large_effect_high_power() {
        // Large effect, large n_max → high conditional power.
        let entry = make_entry(5.0, 1.0, 50.0, 500.0, 1_000_000.0);
        let results = enrich_portfolio_allocation(&[entry], 0.05).unwrap();
        assert!(
            results[0].conditional_power > 0.80,
            "cp={}",
            results[0].conditional_power
        );
    }

    #[test]
    fn test_enrich_annualized_impact_positive() {
        let entry = make_entry(0.01, 1.0, 100.0, 200.0, 10_000_000.0);
        let results = enrich_portfolio_allocation(&[entry], 0.05).unwrap();
        let impact = results[0].annualized_impact;
        // 0.01 * 10M * 365 = 36.5M
        assert!(
            (impact - 36_500_000.0).abs() < 1.0,
            "annualized_impact={impact}"
        );
    }

    #[test]
    fn test_enrich_negative_effect_negative_impact() {
        let entry = make_entry(-0.01, 1.0, 100.0, 200.0, 1_000_000.0);
        let results = enrich_portfolio_allocation(&[entry], 0.05).unwrap();
        assert!(
            results[0].annualized_impact < 0.0,
            "negative effect should produce negative impact"
        );
    }

    #[test]
    fn test_enrich_invalid_alpha_budget_fails() {
        let entry = make_entry(0.5, 1.0, 100.0, 200.0, 1_000_000.0);
        assert!(enrich_portfolio_allocation(&[entry], 0.0).is_err());
        let entry2 = make_entry(0.5, 1.0, 100.0, 200.0, 1_000_000.0);
        assert!(enrich_portfolio_allocation(&[entry2], 1.5).is_err());
    }

    #[test]
    fn test_enrich_preserves_order() {
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let entries: Vec<_> = ids
            .iter()
            .map(|&id| ExperimentPortfolioEntry {
                experiment_id: id,
                effect_estimate: 0.1,
                blinded_sigma_sq: 1.0,
                n_per_arm: 100.0,
                n_max_per_arm: 200.0,
                daily_users: 1_000_000.0,
                duration_days: 14.0,
            })
            .collect();
        let results = enrich_portfolio_allocation(&entries, 0.05).unwrap();
        for (i, r) in results.iter().enumerate() {
            assert_eq!(r.experiment_id, ids[i], "order must be preserved");
        }
    }
}
