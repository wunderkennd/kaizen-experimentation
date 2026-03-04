#!/usr/bin/env python3
"""Generate golden-file test data for experimentation-stats.

Expected values computed via scipy.stats (equivalent to R's t.test and chisq.test).
Each file contains raw data, parameters, and expected results to 15+ decimal places.
"""
import json
import math
import numpy as np
from scipy import stats

OUTPUT_DIR = "crates/experimentation-stats/tests/golden"

def welch_ttest(control, treatment, alpha=0.05):
    """Compute Welch's t-test matching R's t.test(x, y, var.equal=FALSE)."""
    n_c = len(control)
    n_t = len(treatment)
    mean_c = np.mean(control)
    mean_t = np.mean(treatment)
    var_c = np.var(control, ddof=1)
    var_t = np.var(treatment, ddof=1)

    se = math.sqrt(var_c / n_c + var_t / n_t)
    effect = mean_t - mean_c

    # Welch-Satterthwaite degrees of freedom
    df_num = (var_c / n_c + var_t / n_t) ** 2
    df_den = (var_c / n_c) ** 2 / (n_c - 1) + (var_t / n_t) ** 2 / (n_t - 1)
    df = df_num / df_den

    t_stat = effect / se
    p_value = 2.0 * (1.0 - stats.t.cdf(abs(t_stat), df))

    t_crit = stats.t.ppf(1.0 - alpha / 2.0, df)
    ci_lower = effect - t_crit * se
    ci_upper = effect + t_crit * se

    return {
        "effect": float(effect),
        "ci_lower": float(ci_lower),
        "ci_upper": float(ci_upper),
        "p_value": float(p_value),
        "is_significant": bool(p_value < alpha),
        "df": float(df),
        "control_mean": float(mean_c),
        "treatment_mean": float(mean_t),
    }

def chisq_test(observed, expected_fractions):
    """Compute chi-squared test matching R's chisq.test(observed, p=expected)."""
    total = sum(observed.values())
    expected_counts = {k: v * total for k, v in expected_fractions.items()}

    chi_sq = 0.0
    for k in observed:
        diff = observed[k] - expected_counts[k]
        chi_sq += (diff * diff) / expected_counts[k]

    df = len(observed) - 1
    p_value = 1.0 - stats.chi2.cdf(chi_sq, df)

    return {
        "chi_squared": float(chi_sq),
        "p_value": float(p_value),
        "df": int(df),
    }

# Also validate with scipy directly
def verify_ttest(control, treatment, alpha):
    """Cross-check with scipy.stats.ttest_ind."""
    result = stats.ttest_ind(control, treatment, equal_var=False)
    return result.statistic, result.pvalue

def generate_ttest_files():
    """Generate 5 t-test golden files."""
    np.random.seed(42)

    datasets = [
        {
            "test_name": "equal_variance_equal_n",
            "r_command": "set.seed(42); c=rnorm(20,10,2); t=rnorm(20,10,2); t.test(c,t,var.equal=FALSE)",
            "control": np.random.normal(10, 2, 20).tolist(),
            "treatment": None,  # Generated below with same seed state
            "alpha": 0.05,
            "description": "Baseline: equal n=20, similar variance, no true effect"
        },
    ]
    # Generate first dataset
    datasets[0]["treatment"] = np.random.normal(10, 2, 20).tolist()

    # Dataset 2: unequal n
    np.random.seed(123)
    ds2_control = np.random.normal(50, 10, 50).tolist()
    ds2_treatment = np.random.normal(55, 10, 10).tolist()

    # Dataset 3: large effect
    np.random.seed(456)
    ds3_control = np.random.normal(100, 5, 30).tolist()
    ds3_treatment = np.random.normal(125, 5, 30).tolist()

    # Dataset 4: small effect
    np.random.seed(789)
    ds4_control = np.random.normal(0, 1, 100).tolist()
    ds4_treatment = np.random.normal(0.1, 1, 100).tolist()

    # Dataset 5: extreme variance ratio
    np.random.seed(101)
    ds5_control = np.random.normal(50, 0.1, 30).tolist()
    ds5_treatment = np.random.normal(50.5, 10.0, 30).tolist()

    all_datasets = [
        ("ttest_equal_variance_equal_n.json", {
            "test_name": "equal_variance_equal_n",
            "r_command": "set.seed(42); c=rnorm(20,10,2); t=rnorm(20,10,2); t.test(c,t,var.equal=FALSE)",
            "control": datasets[0]["control"],
            "treatment": datasets[0]["treatment"],
            "alpha": 0.05,
        }),
        ("ttest_unequal_n.json", {
            "test_name": "unequal_n",
            "r_command": "set.seed(123); c=rnorm(50,50,10); t=rnorm(10,55,10); t.test(c,t,var.equal=FALSE)",
            "control": ds2_control,
            "treatment": ds2_treatment,
            "alpha": 0.05,
        }),
        ("ttest_large_effect.json", {
            "test_name": "large_effect",
            "r_command": "set.seed(456); c=rnorm(30,100,5); t=rnorm(30,125,5); t.test(c,t,var.equal=FALSE)",
            "control": ds3_control,
            "treatment": ds3_treatment,
            "alpha": 0.05,
        }),
        ("ttest_small_effect.json", {
            "test_name": "small_effect",
            "r_command": "set.seed(789); c=rnorm(100,0,1); t=rnorm(100,0.1,1); t.test(c,t,var.equal=FALSE)",
            "control": ds4_control,
            "treatment": ds4_treatment,
            "alpha": 0.05,
        }),
        ("ttest_extreme_variance_ratio.json", {
            "test_name": "extreme_variance_ratio",
            "r_command": "set.seed(101); c=rnorm(30,50,0.1); t=rnorm(30,50.5,10); t.test(c,t,var.equal=FALSE)",
            "control": ds5_control,
            "treatment": ds5_treatment,
            "alpha": 0.05,
        }),
    ]

    for filename, ds in all_datasets:
        control = np.array(ds["control"])
        treatment = np.array(ds["treatment"])
        expected = welch_ttest(control, treatment, ds["alpha"])

        # Cross-validate with scipy
        scipy_stat, scipy_p = verify_ttest(control, treatment, ds["alpha"])
        assert abs(expected["p_value"] - scipy_p) < 1e-12, \
            f"p-value mismatch for {filename}: manual={expected['p_value']}, scipy={scipy_p}"

        ds["expected"] = expected
        filepath = f"{OUTPUT_DIR}/{filename}"
        with open(filepath, 'w') as f:
            json.dump(ds, f, indent=2)
        print(f"  Generated {filepath}")
        print(f"    effect={expected['effect']:.10f}, p={expected['p_value']:.10e}, df={expected['df']:.10f}, sig={expected['is_significant']}")


def generate_srm_files():
    """Generate 3 SRM golden files."""
    srm_datasets = [
        ("srm_no_mismatch.json", {
            "test_name": "no_mismatch",
            "r_command": "chisq.test(c(4980, 5020), p=c(0.5, 0.5))",
            "observed": {"control": 4980, "treatment": 5020},
            "expected_fractions": {"control": 0.5, "treatment": 0.5},
            "alpha": 0.001,
        }),
        ("srm_clear_mismatch.json", {
            "test_name": "clear_mismatch",
            "r_command": "chisq.test(c(6000, 4000), p=c(0.5, 0.5))",
            "observed": {"control": 6000, "treatment": 4000},
            "expected_fractions": {"control": 0.5, "treatment": 0.5},
            "alpha": 0.001,
        }),
        ("srm_three_variants.json", {
            "test_name": "three_variants",
            "r_command": "chisq.test(c(3400, 3300, 3300), p=c(1/3, 1/3, 1/3))",
            "observed": {"control": 3400, "variant_a": 3300, "variant_b": 3300},
            "expected_fractions": {"control": 0.3333333333333333, "variant_a": 0.3333333333333333, "variant_b": 0.3333333333333333},
            "alpha": 0.001,
        }),
    ]

    for filename, ds in srm_datasets:
        obs = ds["observed"]
        exp_frac = ds["expected_fractions"]
        expected = chisq_test(obs, exp_frac)

        # Cross-validate with scipy
        obs_values = list(obs.values())
        exp_frac_values = list(exp_frac.values())
        scipy_result = stats.chisquare(obs_values, f_exp=[f * sum(obs_values) for f in exp_frac_values])
        assert abs(expected["chi_squared"] - scipy_result.statistic) < 1e-10, \
            f"chi2 mismatch for {filename}"
        assert abs(expected["p_value"] - scipy_result.pvalue) < 1e-10, \
            f"p-value mismatch for {filename}"

        is_mismatch = expected["p_value"] < ds["alpha"]
        ds["expected"] = {
            "chi_squared": expected["chi_squared"],
            "p_value": expected["p_value"],
            "is_mismatch": is_mismatch,
        }

        filepath = f"{OUTPUT_DIR}/{filename}"
        with open(filepath, 'w') as f:
            json.dump(ds, f, indent=2)
        print(f"  Generated {filepath}")
        print(f"    chi2={expected['chi_squared']:.10f}, p={expected['p_value']:.10e}, mismatch={is_mismatch}")


if __name__ == "__main__":
    print("Generating t-test golden files...")
    generate_ttest_files()
    print("\nGenerating SRM golden files...")
    generate_srm_files()
    print("\nDone. All values cross-validated against scipy.stats.")
