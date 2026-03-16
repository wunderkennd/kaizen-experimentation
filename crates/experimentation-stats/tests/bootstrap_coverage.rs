//! Bootstrap coverage validation via Monte Carlo simulation.
//!
//! Generates 1000 synthetic datasets with known treatment effects, runs BCa and
//! percentile bootstrap on each, and verifies that empirical coverage falls within
//! the expected range for a nominal 95% CI.
//!
//! Acceptance criteria (per onboarding doc):
//!   BCa coverage: 93%–97% on 1000 synthetic datasets (symmetric data)
//!   BCa coverage: 91%–97% on skewed/small-sample data
//!
//! These tests are `#[ignore]` because they take ~30s in release mode (~5min debug).
//! Run with:
//!   cargo test --release -p experimentation-stats --test bootstrap_coverage -- --ignored
//!   just test-bootstrap-coverage

use experimentation_stats::bootstrap::{bootstrap_bca, bootstrap_ci};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand_distr::{Distribution, LogNormal, Normal};

/// Master seed for reproducible dataset generation.
const MASTER_SEED: u64 = 20260312;

/// Number of synthetic datasets per scenario.
const N_DATASETS: usize = 1000;

/// Bootstrap resamples per dataset (odd for clean median quantile).
const N_RESAMPLES: usize = 1999;

/// Nominal significance level (95% CI).
const ALPHA: f64 = 0.05;

/// Run a coverage simulation for BCa and percentile bootstrap.
///
/// Returns (bca_coverage, percentile_coverage) as fractions in [0, 1].
fn run_coverage<F>(
    scenario_seed: u64,
    n_per_group: usize,
    true_effect: f64,
    generate: F,
) -> (f64, f64)
where
    F: Fn(usize, &mut StdRng) -> (Vec<f64>, Vec<f64>),
{
    let mut master_rng = StdRng::seed_from_u64(scenario_seed);
    let mut bca_covered = 0usize;
    let mut pct_covered = 0usize;

    for i in 0..N_DATASETS {
        let (control, treatment) = generate(n_per_group, &mut master_rng);

        // Deterministic bootstrap seed per dataset (different from data generation)
        let bootstrap_seed = scenario_seed.wrapping_mul(1_000_003).wrapping_add(i as u64);

        let bca = bootstrap_bca(&control, &treatment, ALPHA, N_RESAMPLES, bootstrap_seed)
            .unwrap_or_else(|e| panic!("BCa failed on dataset {i}: {e}"));

        if bca.ci_lower <= true_effect && true_effect <= bca.ci_upper {
            bca_covered += 1;
        }

        let pct = bootstrap_ci(&control, &treatment, ALPHA, N_RESAMPLES, bootstrap_seed)
            .unwrap_or_else(|e| panic!("Percentile failed on dataset {i}: {e}"));

        if pct.ci_lower <= true_effect && true_effect <= pct.ci_upper {
            pct_covered += 1;
        }
    }

    let bca_coverage = bca_covered as f64 / N_DATASETS as f64;
    let pct_coverage = pct_covered as f64 / N_DATASETS as f64;

    (bca_coverage, pct_coverage)
}

// ---------------------------------------------------------------------------
// Scenario 1: Normal data, medium treatment effect (delta = 0.5)
// ---------------------------------------------------------------------------

/// Both BCa and percentile should achieve near-nominal coverage on symmetric data.
#[test]
#[ignore] // ~30s release, ~80s debug — run via `just test-bootstrap-coverage`
fn coverage_normal_medium_effect() {
    let (bca, pct) = run_coverage(MASTER_SEED, 50, 0.5, |n, rng| {
        let dist_c = Normal::new(0.0, 1.0).unwrap();
        let dist_t = Normal::new(0.5, 1.0).unwrap();
        let control: Vec<f64> = (0..n).map(|_| dist_c.sample(rng)).collect();
        let treatment: Vec<f64> = (0..n).map(|_| dist_t.sample(rng)).collect();
        (control, treatment)
    });

    eprintln!("Normal medium effect: BCa={bca:.3}, Percentile={pct:.3}");
    assert!(
        bca >= 0.93 && bca <= 0.97,
        "BCa coverage {bca:.3} outside [0.93, 0.97]"
    );
    assert!(
        pct >= 0.92 && pct <= 0.98,
        "Percentile coverage {pct:.3} outside [0.92, 0.98]"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Normal data, null effect (delta = 0.0)
// ---------------------------------------------------------------------------

/// Calibration check: CI should contain 0 at nominal rate.
#[test]
#[ignore]
fn coverage_normal_null_effect() {
    let (bca, pct) = run_coverage(MASTER_SEED + 1, 50, 0.0, |n, rng| {
        let dist = Normal::new(0.0, 1.0).unwrap();
        let control: Vec<f64> = (0..n).map(|_| dist.sample(rng)).collect();
        let treatment: Vec<f64> = (0..n).map(|_| dist.sample(rng)).collect();
        (control, treatment)
    });

    eprintln!("Normal null effect: BCa={bca:.3}, Percentile={pct:.3}");
    assert!(
        bca >= 0.93 && bca <= 0.97,
        "BCa coverage {bca:.3} outside [0.93, 0.97]"
    );
    assert!(
        pct >= 0.92 && pct <= 0.98,
        "Percentile coverage {pct:.3} outside [0.92, 0.98]"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: Skewed (lognormal) data, medium treatment effect
// ---------------------------------------------------------------------------

/// BCa adjusts for skewness but may still slightly undercover on highly skewed
/// data with a location-shift DGP. The percentile method undercoverages more.
#[test]
#[ignore]
fn coverage_skewed_medium_effect() {
    let true_effect = 0.5;
    let (bca, pct) = run_coverage(MASTER_SEED + 2, 50, true_effect, |n, rng| {
        let dist = LogNormal::new(0.0, 0.5).unwrap();
        let control: Vec<f64> = (0..n).map(|_| dist.sample(rng)).collect();
        let treatment: Vec<f64> = (0..n).map(|_| dist.sample(rng) + true_effect).collect();
        (control, treatment)
    });

    eprintln!("Skewed medium effect: BCa={bca:.3}, Percentile={pct:.3}");
    // BCa: wider range for skewed data — known undercoverage with location-shift DGP
    assert!(
        bca >= 0.91 && bca <= 0.97,
        "BCa coverage {bca:.3} outside [0.91, 0.97]"
    );
    // Percentile: wider tolerance for skewed data
    assert!(
        pct >= 0.90 && pct <= 0.98,
        "Percentile coverage {pct:.3} outside [0.90, 0.98]"
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: Small sample (n = 15), medium effect
// ---------------------------------------------------------------------------

/// Coverage may be slightly worse with small n where jackknife estimates
/// are noisier, so we allow a wider acceptable range.
#[test]
#[ignore]
fn coverage_small_sample() {
    let (bca, pct) = run_coverage(MASTER_SEED + 3, 15, 0.5, |n, rng| {
        let dist_c = Normal::new(0.0, 1.0).unwrap();
        let dist_t = Normal::new(0.5, 1.0).unwrap();
        let control: Vec<f64> = (0..n).map(|_| dist_c.sample(rng)).collect();
        let treatment: Vec<f64> = (0..n).map(|_| dist_t.sample(rng)).collect();
        (control, treatment)
    });

    eprintln!("Small sample: BCa={bca:.3}, Percentile={pct:.3}");
    // Wider tolerance for small samples
    assert!(
        bca >= 0.91 && bca <= 0.98,
        "BCa coverage {bca:.3} outside [0.91, 0.98]"
    );
    assert!(
        pct >= 0.90 && pct <= 0.98,
        "Percentile coverage {pct:.3} outside [0.90, 0.98]"
    );
}
