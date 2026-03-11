#!/usr/bin/env python3
"""
Generate GST golden files using recursive numerical integration.

Implements the Armitage-McPherson-Rowe algorithm matching R's gsDesign package.
Uses Gauss-Legendre quadrature with per-step adaptive intervals for high-precision
computation of the multivariate normal continuation probability.

Usage: python3 scripts/generate_gst_golden_py.py

This produces boundaries equivalent to:
  gsDesign(k=K, test.type=2, alpha=alpha, sfu=sfLDOF|sfLDPocock)
"""

import json
import math
from pathlib import Path

import numpy as np
from scipy import stats
from scipy.optimize import brentq

OUTPUT_DIR = Path("crates/experimentation-stats/tests/golden")

# Number of Gauss-Legendre nodes per step (201 is more than sufficient for 1e-6)
N_GL = 201


# ---------------------------------------------------------------------------
# Spending functions (Lan-DeMets approximations)
# ---------------------------------------------------------------------------

def spending_obf(t: float, alpha: float) -> float:
    """Lan-DeMets O'Brien-Fleming: 2*(1 - Phi(z_{alpha/2}/sqrt(t)))"""
    z_alpha_half = stats.norm.ppf(1.0 - alpha / 2.0)
    return float(2.0 * (1.0 - stats.norm.cdf(z_alpha_half / math.sqrt(t))))


def spending_pocock(t: float, alpha: float) -> float:
    """Lan-DeMets Pocock: alpha * ln(1 + (e-1)*t)"""
    return float(alpha * math.log(1.0 + (math.e - 1.0) * t))


# ---------------------------------------------------------------------------
# Gauss-Legendre helpers
# ---------------------------------------------------------------------------

# Pre-compute GL nodes and weights on [-1, 1]
_GL_REF_NODES, _GL_REF_WEIGHTS = np.polynomial.legendre.leggauss(N_GL)


def gl_on_interval(lo: float, hi: float):
    """Map GL nodes and weights from [-1,1] to [lo, hi]."""
    half_len = 0.5 * (hi - lo)
    mid = 0.5 * (lo + hi)
    nodes = half_len * _GL_REF_NODES + mid
    weights = half_len * _GL_REF_WEIGHTS
    return nodes, weights


# ---------------------------------------------------------------------------
# Armitage-McPherson-Rowe recursive integration
# ---------------------------------------------------------------------------

def gst_boundaries_recursive(
    K: int,
    alpha: float,
    spending_fn,
) -> list[dict]:
    """
    Compute GST boundaries via Armitage-McPherson-Rowe recursive integration
    using Gauss-Legendre quadrature with adaptive intervals.

    At each step k:
    1. g_{k-1} is stored at GL nodes on [-c_{k-1}, c_{k-1}]
    2. g_k(z) = sum_i g_{k-1}(w_i) * f(z|w_i) * gl_weight_i
    3. Find c_k: integral_{-c_k}^{c_k} g_k(z) dz = 1 - alpha*(t_k)
       The integral for each candidate c is evaluated using GL on [-c, c].
    """
    info_fracs = [(k + 1) / K for k in range(K)]
    cum_alphas = [max(0.0, min(spending_fn(t, alpha), alpha)) for t in info_fracs]

    boundaries = []
    prev_cum_alpha = 0.0
    prev_t = 0.0

    # Previous step quadrature: (nodes, densities, weights) on [-c_{k-1}, c_{k-1}]
    prev_nodes = None
    prev_dens = None
    prev_wts = None

    for k in range(K):
        t = info_fracs[k]
        cum_alpha = cum_alphas[k]
        inc_alpha = cum_alpha - prev_cum_alpha

        if k == 0:
            # Look 1: simple quantile
            c_k = float(stats.norm.ppf(1.0 - cum_alpha / 2.0))

            # Set up quadrature on [-c_k, c_k]
            nodes, wts = gl_on_interval(-c_k, c_k)
            dens = stats.norm.pdf(nodes)

            prev_nodes = nodes
            prev_dens = dens
            prev_wts = wts
        else:
            # Transition: Z_k | Z_{k-1}=w ~ N(w*r, sigma_t)
            ratio = prev_t / t
            r = math.sqrt(ratio)
            sigma_t = math.sqrt(1.0 - ratio)

            def eval_gk(z_array):
                """Evaluate g_k at an array of z values."""
                # g_k(z) = sum_i prev_dens[i] * f(z | prev_nodes[i]) * prev_wts[i]
                result = np.zeros(len(z_array))
                prev_means = prev_nodes * r
                for j, z_j in enumerate(z_array):
                    trans = stats.norm.pdf(z_j, loc=prev_means, scale=sigma_t)
                    result[j] = np.sum(prev_dens * trans * prev_wts)
                return result

            def continuation_prob(c):
                """Integral of g_k over [-c, c] using GL quadrature."""
                nodes_c, wts_c = gl_on_interval(-c, c)
                gk_vals = eval_gk(nodes_c)
                return float(np.sum(gk_vals * wts_c))

            target = 1.0 - cum_alpha

            def obj(c):
                return continuation_prob(c) - target

            try:
                c_k = float(brentq(obj, 0.5, 7.5, xtol=1e-12, maxiter=500))
            except ValueError:
                if inc_alpha > 0:
                    c_k = float(stats.norm.ppf(1.0 - inc_alpha / 2.0))
                else:
                    c_k = float('inf')

            # Store g_k at GL nodes on [-c_k, c_k] for next step
            nodes, wts = gl_on_interval(-c_k, c_k)
            dens = eval_gk(nodes)

            prev_nodes = nodes
            prev_dens = dens
            prev_wts = wts

        boundaries.append({
            "look": k + 1,
            "information_fraction": t,
            "cumulative_alpha": cum_alpha,
            "incremental_alpha": inc_alpha,
            "critical_value": c_k,
        })

        prev_cum_alpha = cum_alpha
        prev_t = t

    return boundaries


# ---------------------------------------------------------------------------
# Configuration and output
# ---------------------------------------------------------------------------

CONFIGS = [
    ("gst_obf_4_looks",              4, 0.05, spending_obf,    "OBrienFleming", "sfLDOF"),
    ("gst_pocock_4_looks",            4, 0.05, spending_pocock, "Pocock",        "sfLDPocock"),
    ("gst_obf_5_looks_alpha10",       5, 0.10, spending_obf,    "OBrienFleming", "sfLDOF"),
    ("gst_pocock_3_looks",            3, 0.05, spending_pocock, "Pocock",        "sfLDPocock"),
    ("gst_obf_2_looks",              2, 0.05, spending_obf,    "OBrienFleming", "sfLDOF"),
    ("gst_pocock_2_looks",            2, 0.05, spending_pocock, "Pocock",        "sfLDPocock"),
    ("gst_obf_6_looks",              6, 0.05, spending_obf,    "OBrienFleming", "sfLDOF"),
    ("gst_pocock_6_looks",            6, 0.05, spending_pocock, "Pocock",        "sfLDPocock"),
    ("gst_obf_3_looks_alpha01",       3, 0.01, spending_obf,    "OBrienFleming", "sfLDOF"),
    ("gst_pocock_5_looks_alpha10",    5, 0.10, spending_pocock, "Pocock",        "sfLDPocock"),
]


def main():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    for name, k, alpha, sfn, sf_label, r_sfu in CONFIGS:
        r_cmd = f"gsDesign(k={k}, test.type=2, alpha={alpha}, sfu={r_sfu})"
        boundaries = gst_boundaries_recursive(k, alpha, sfn)

        golden = {
            "test_name": name,
            "spending_function": sf_label,
            "planned_looks": k,
            "overall_alpha": alpha,
            "source": "recursive_integration_python",
            "r_command": r_cmd,
            "boundaries": boundaries,
        }

        outfile = OUTPUT_DIR / f"{name}.json"
        with open(outfile, "w") as f:
            json.dump(golden, f, indent=2)
            f.write("\n")

        crit_vals = [f"{b['critical_value']:.6f}" for b in boundaries]
        print(f"Wrote {outfile} ({k} looks, alpha={alpha}, {sf_label})")
        print(f"  Boundaries: {', '.join(crit_vals)}")

    print(f"\nGenerated {len(CONFIGS)} golden files in {OUTPUT_DIR}/")


if __name__ == "__main__":
    main()
