#!/usr/bin/env python3
"""
Independent validation of bootstrap BCa coverage using Python/numpy.

Runs the same Monte Carlo simulation as the Rust bootstrap_coverage test:
generate 1000 synthetic datasets with known treatment effects, compute BCa
bootstrap CIs, and verify empirical coverage is 93-97%.

This script uses a from-scratch BCa implementation (not scipy.stats.bootstrap)
so the algorithm exactly mirrors our Rust code, providing a true independent
cross-language validation.

Usage:
    python3 scripts/generate_bootstrap_coverage.py
"""

import sys
import time

import numpy as np
from scipy import stats

# ---------------------------------------------------------------------------
# BCa bootstrap (mirrors Rust implementation in bootstrap.rs)
# ---------------------------------------------------------------------------


def bootstrap_bca(control, treatment, alpha, n_resamples, rng):
    """BCa bootstrap CI for difference in means.

    Algorithm matches Rust's experimentation_stats::bootstrap::bootstrap_bca:
    1. Generate replicates by resampling each group independently
    2. Bias correction z0 from proportion of replicates below observed
    3. Acceleration from delete-one jackknife
    4. Adjusted quantiles via BCa formula
    """
    n_c, n_t = len(control), len(treatment)
    observed = np.mean(treatment) - np.mean(control)

    # Generate replicates
    replicates = np.empty(n_resamples)
    for i in range(n_resamples):
        c_idx = rng.integers(0, n_c, size=n_c)
        t_idx = rng.integers(0, n_t, size=n_t)
        replicates[i] = np.mean(treatment[t_idx]) - np.mean(control[c_idx])

    # Bias correction z0
    prop_below = np.mean(replicates < observed)
    prop_below = np.clip(prop_below, 1e-10, 1 - 1e-10)
    z0 = stats.norm.ppf(prop_below)

    # Acceleration via jackknife (matches Rust jackknife_acceleration)
    c_sum, t_sum = np.sum(control), np.sum(treatment)
    c_mean, t_mean = np.mean(control), np.mean(treatment)

    jk = np.empty(n_c + n_t)
    for j, x in enumerate(control):
        jk[j] = t_mean - (c_sum - x) / (n_c - 1)
    for j, x in enumerate(treatment):
        jk[n_c + j] = (t_sum - x) / (n_t - 1) - c_mean

    theta_dot = np.mean(jk)
    d = theta_dot - jk
    sum_sq = np.sum(d**2)
    a = np.sum(d**3) / (6 * sum_sq**1.5) if sum_sq > 1e-15 else 0.0

    # Adjusted quantiles
    z_lo = stats.norm.ppf(alpha / 2)
    z_hi = stats.norm.ppf(1 - alpha / 2)

    def adj_q(z):
        num = z0 + z
        den = 1 - a * num
        if abs(den) < 1e-15:
            return stats.norm.cdf(z)
        return stats.norm.cdf(z0 + num / den)

    q_lo, q_hi = adj_q(z_lo), adj_q(z_hi)

    replicates.sort()
    lo_idx = min(int(np.floor(q_lo * n_resamples)), n_resamples - 1)
    hi_idx = min(int(np.floor(q_hi * n_resamples)), n_resamples - 1)

    return replicates[lo_idx], replicates[hi_idx]


def bootstrap_percentile(control, treatment, alpha, n_resamples, rng):
    """Percentile bootstrap CI for difference in means."""
    n_c, n_t = len(control), len(treatment)

    replicates = np.empty(n_resamples)
    for i in range(n_resamples):
        c_idx = rng.integers(0, n_c, size=n_c)
        t_idx = rng.integers(0, n_t, size=n_t)
        replicates[i] = np.mean(treatment[t_idx]) - np.mean(control[c_idx])

    replicates.sort()
    lo_idx = int(np.floor(alpha / 2 * n_resamples))
    hi_idx = min(int(np.floor((1 - alpha / 2) * n_resamples)), n_resamples - 1)

    return replicates[lo_idx], replicates[hi_idx]


# ---------------------------------------------------------------------------
# Coverage simulation
# ---------------------------------------------------------------------------

N_DATASETS = 1000
N_RESAMPLES = 1999
ALPHA = 0.05


def run_coverage(scenario_name, scenario_seed, n_per_group, true_effect, gen_fn):
    """Run coverage simulation for a single scenario."""
    master_rng = np.random.default_rng(scenario_seed)
    bca_covered = 0
    pct_covered = 0

    t0 = time.time()

    for i in range(N_DATASETS):
        control, treatment = gen_fn(n_per_group, master_rng)

        # Deterministic bootstrap RNG per dataset
        boot_seed = int(scenario_seed * 1_000_003 + i) % (2**63)
        boot_rng = np.random.default_rng(boot_seed)

        lo, hi = bootstrap_bca(control, treatment, ALPHA, N_RESAMPLES, boot_rng)
        if lo <= true_effect <= hi:
            bca_covered += 1

        boot_rng2 = np.random.default_rng(boot_seed)
        lo2, hi2 = bootstrap_percentile(control, treatment, ALPHA, N_RESAMPLES, boot_rng2)
        if lo2 <= true_effect <= hi2:
            pct_covered += 1

        if (i + 1) % 200 == 0:
            elapsed = time.time() - t0
            print(f"    {scenario_name}: {i+1}/{N_DATASETS} ({elapsed:.1f}s)")

    bca_cov = bca_covered / N_DATASETS
    pct_cov = pct_covered / N_DATASETS
    elapsed = time.time() - t0

    return bca_cov, pct_cov, elapsed


def gen_normal(mean_c, mean_t, std):
    """Factory for normal data generator."""
    def generate(n, rng):
        control = rng.normal(mean_c, std, size=n)
        treatment = rng.normal(mean_t, std, size=n)
        return control, treatment
    return generate


def gen_lognormal_shifted(mu, sigma, shift):
    """Factory for lognormal + constant shift data generator."""
    def generate(n, rng):
        control = rng.lognormal(mu, sigma, size=n)
        treatment = rng.lognormal(mu, sigma, size=n) + shift
        return control, treatment
    return generate


SCENARIOS = [
    ("Normal medium effect (delta=0.5, n=50)", 20260312, 50, 0.5,
     gen_normal(0.0, 0.5, 1.0), (0.93, 0.97), (0.92, 0.98)),

    ("Normal null effect (delta=0.0, n=50)", 20260313, 50, 0.0,
     gen_normal(0.0, 0.0, 1.0), (0.93, 0.97), (0.92, 0.98)),

    ("Skewed medium effect (lognormal, delta=0.5, n=50)", 20260314, 50, 0.5,
     gen_lognormal_shifted(0.0, 0.5, 0.5), (0.93, 0.97), (0.90, 0.98)),

    ("Small sample (delta=0.5, n=15)", 20260315, 15, 0.5,
     gen_normal(0.0, 0.5, 1.0), (0.91, 0.98), (0.90, 0.98)),
]


def main():
    print("=" * 72)
    print("Bootstrap BCa/Percentile Coverage Validation (Python reference)")
    print(f"  {N_DATASETS} datasets x {N_RESAMPLES} resamples, alpha={ALPHA}")
    print("=" * 72)
    print()

    all_pass = True

    for name, seed, n, effect, gen, bca_range, pct_range in SCENARIOS:
        print(f"  Scenario: {name}")
        bca_cov, pct_cov, elapsed = run_coverage(name, seed, n, effect, gen)

        bca_ok = bca_range[0] <= bca_cov <= bca_range[1]
        pct_ok = pct_range[0] <= pct_cov <= pct_range[1]

        bca_status = "PASS" if bca_ok else "FAIL"
        pct_status = "PASS" if pct_ok else "FAIL"

        print(f"    BCa coverage:        {bca_cov:.3f}  expected [{bca_range[0]}, {bca_range[1]}]  [{bca_status}]")
        print(f"    Percentile coverage: {pct_cov:.3f}  expected [{pct_range[0]}, {pct_range[1]}]  [{pct_status}]")
        print(f"    Time: {elapsed:.1f}s")
        print()

        if not bca_ok or not pct_ok:
            all_pass = False

    if all_pass:
        print("All scenarios PASSED.")
    else:
        print("Some scenarios FAILED.")
        sys.exit(1)


if __name__ == "__main__":
    main()
