#!/usr/bin/env python3
"""
Generate GST boundary golden files using Armitage-McPherson-Rowe recursive
integration with Gauss-Legendre quadrature — the same algorithm R's gsDesign
package and our Rust implementation use.

Mathematical formulation:
  - Lan-DeMets alpha spending: OBF and Pocock
  - Recursive GL quadrature for multivariate normal continuation probabilities
  - Correlation structure: Corr(Z_i, Z_j) = sqrt(t_i / t_j) for i < j
  - Transition density: Z_k | Z_{k-1}=w ~ N(w·sqrt(t_{k-1}/t_k), 1 - t_{k-1}/t_k)

Usage:
    python3 scripts/generate_gst_golden.py              # compare with existing
    python3 scripts/generate_gst_golden.py --update      # overwrite golden files

Reference:
    R: gsDesign(k=K, test.type=2, alpha=α, sfu=sfLDOF|sfLDPocock)
"""

import json
import sys
from pathlib import Path

import numpy as np
from scipy import stats

GOLDEN_DIR = Path(__file__).parent.parent / "crates" / "experimentation-stats" / "tests" / "golden"

# Number of Gauss-Legendre quadrature nodes
N_NODES = 201


def obf_spending(t: float, alpha: float) -> float:
    """Lan-DeMets O'Brien-Fleming: α*(t) = 2·[1 - Φ(z_{α/2}/√t)]"""
    if t <= 0:
        return 0.0
    if t >= 1:
        return alpha
    z = stats.norm.ppf(1 - alpha / 2)
    return float(min(alpha, 2.0 * (1.0 - stats.norm.cdf(z / np.sqrt(t)))))


def pocock_spending(t: float, alpha: float) -> float:
    """Lan-DeMets Pocock: α*(t) = α·ln(1 + (e-1)·t)"""
    if t <= 0:
        return 0.0
    if t >= 1:
        return alpha
    return float(min(alpha, alpha * np.log(1.0 + (np.e - 1.0) * t)))


def gl_on_interval(nodes_ref, weights_ref, lo, hi):
    """Map GL nodes from [-1,1] to [lo, hi]."""
    half_len = (hi - lo) / 2.0
    mid = (hi + lo) / 2.0
    return half_len * nodes_ref + mid, half_len * weights_ref


def gst_boundaries_reference(planned_looks, overall_alpha, spending_fn):
    """Compute GST boundaries via Armitage-McPherson-Rowe recursive integration.

    Uses Gauss-Legendre quadrature (N_NODES points). The GL representation
    of the continuation density is stored as (nodes, weights, values) arrays,
    and the transition convolution is computed via matrix multiply.

    IMPORTANT: When updating the GL representation for the next look, we must
    evaluate g_k at the new grid points BEFORE rebinding the old arrays.
    """
    info_fractions = [(k + 1) / planned_looks for k in range(planned_looks)]
    cum_alphas = [spending_fn(t, overall_alpha) for t in info_fractions]

    gl_ref_nodes, gl_ref_weights = np.polynomial.legendre.leggauss(N_NODES)

    results = []
    prev_cum_alpha = 0.0

    # GL quadrature representation of continuation density from previous look
    prev_nodes = None
    prev_weights = None
    prev_densities = None
    prev_t = None

    for k in range(planned_looks):
        t = info_fractions[k]
        target_cum = cum_alphas[k]
        incremental = target_cum - prev_cum_alpha

        if k == 0:
            # Look 1: c_1 = Φ^{-1}(1 - α*(t_1)/2)
            c = float(stats.norm.ppf(1 - target_cum / 2))

            # g_1(z) = φ(z) on [-c, c]
            prev_nodes, prev_weights = gl_on_interval(gl_ref_nodes, gl_ref_weights, -c, c)
            prev_densities = stats.norm.pdf(prev_nodes)

        else:
            # Transition parameters
            ratio = prev_t / t
            r = np.sqrt(ratio)
            sigma_t = np.sqrt(1.0 - ratio)

            # Snapshot the previous look's representation (avoid late-binding issues)
            w_nodes = prev_nodes
            w_weights = prev_weights
            w_densities = prev_densities

            def eval_gk_vec(z_arr, w_n=w_nodes, w_w=w_weights, w_d=w_densities,
                            r_val=r, sig=sigma_t):
                """Evaluate g_k(z) = ∫ g_{k-1}(w) · f(z|w) dw via GL quadrature.

                All dependencies passed as default args to avoid closure capture issues.
                """
                # z_arr shape: (M,), w_n shape: (N,)
                z_col = z_arr[:, np.newaxis]     # (M, 1)
                w_row = w_n[np.newaxis, :]       # (1, N)
                args = (z_col - w_row * r_val) / sig
                transition = stats.norm.pdf(args) / sig  # (M, N)
                wv = (w_w * w_d)[np.newaxis, :]          # (1, N)
                return np.sum(wv * transition, axis=1)    # (M,)

            def continuation_prob(c_val, eval_fn=eval_gk_vec):
                """∫_{-c}^{c} g_k(z) dz via GL quadrature."""
                z_nodes, z_weights = gl_on_interval(gl_ref_nodes, gl_ref_weights,
                                                    -c_val, c_val)
                gk_at_z = eval_fn(z_nodes)
                return float(np.sum(z_weights * gk_at_z))

            # Bisection: find c_k such that continuation_prob(c_k) = 1 - target_cum
            target_cont = 1.0 - target_cum
            lo, hi = 0.5, 8.0
            for _ in range(200):
                mid = (lo + hi) / 2.0
                prob = continuation_prob(mid)
                if prob > target_cont:
                    hi = mid
                else:
                    lo = mid
                if hi - lo < 1e-12:
                    break
            c = (lo + hi) / 2.0

            # Store g_k representation on [-c, c] for next iteration.
            # Evaluate BEFORE updating prev_nodes (avoids late-binding bug).
            new_nodes, new_weights = gl_on_interval(gl_ref_nodes, gl_ref_weights, -c, c)
            new_densities = eval_gk_vec(new_nodes)
            prev_nodes = new_nodes
            prev_weights = new_weights
            prev_densities = new_densities

        prev_t = t
        prev_cum_alpha = target_cum

        results.append({
            "look": k + 1,
            "information_fraction": t,
            "cumulative_alpha": target_cum,
            "incremental_alpha": incremental,
            "critical_value": c,
        })

    return results


# All golden file configurations
CONFIGS = [
    ("gst_obf_2_looks.json", "OBrienFleming", obf_spending, 2, 0.05),
    ("gst_obf_3_looks_alpha01.json", "OBrienFleming", obf_spending, 3, 0.01),
    ("gst_obf_4_looks.json", "OBrienFleming", obf_spending, 4, 0.05),
    ("gst_obf_5_looks_alpha10.json", "OBrienFleming", obf_spending, 5, 0.10),
    ("gst_obf_6_looks.json", "OBrienFleming", obf_spending, 6, 0.05),
    ("gst_pocock_2_looks.json", "Pocock", pocock_spending, 2, 0.05),
    ("gst_pocock_3_looks.json", "Pocock", pocock_spending, 3, 0.05),
    ("gst_pocock_4_looks.json", "Pocock", pocock_spending, 4, 0.05),
    ("gst_pocock_5_looks_alpha10.json", "Pocock", pocock_spending, 5, 0.10),
    ("gst_pocock_6_looks.json", "Pocock", pocock_spending, 6, 0.05),
]


def compare_with_existing():
    """Compare newly generated values against existing golden files."""
    print("=== GST Boundary Validation (scipy GL quadrature vs existing golden files) ===\n")

    all_pass = True
    for filename, spending_name, spending_fn, k, alpha in CONFIGS:
        path = GOLDEN_DIR / filename

        if not path.exists():
            print(f"  {filename}: MISSING")
            continue

        with open(path) as f:
            existing = json.load(f)

        new_boundaries = gst_boundaries_reference(k, alpha, spending_fn)

        print(f"  {filename} (K={k}, α={alpha}, {spending_name}):")

        max_diff = 0.0
        for old_b, new_b in zip(existing["boundaries"], new_boundaries):
            look = old_b["look"]
            old_c = old_b["critical_value"]
            new_c = new_b["critical_value"]
            diff = abs(old_c - new_c)
            max_diff = max(max_diff, diff)
            status = "OK" if diff < 1e-4 else "MISMATCH"
            print(f"    Look {look}: existing={old_c:.8f}, scipy={new_c:.8f}, diff={diff:.2e} [{status}]")

        overall = "PASS" if max_diff < 1e-4 else "FAIL"
        if overall == "FAIL":
            all_pass = False
        print(f"    Max diff: {max_diff:.2e} [{overall}]\n")

    return all_pass


def generate_all():
    """Generate and write all golden files."""
    print("Generating GST boundary golden files via scipy GL recursive integration")
    print(f"Output directory: {GOLDEN_DIR}\n")

    for filename, spending_name, spending_fn, k, alpha in CONFIGS:
        boundaries = gst_boundaries_reference(k, alpha, spending_fn)

        golden = {
            "test_name": filename.replace(".json", ""),
            "spending_function": spending_name,
            "planned_looks": k,
            "overall_alpha": alpha,
            "source": "scipy_gauss_legendre_201_nodes",
            "r_command": (
                f"gsDesign(k={k}, test.type=2, alpha={alpha}, "
                f"sfu={'sfLDOF' if spending_name == 'OBrienFleming' else 'sfLDPocock'})"
            ),
            "boundaries": boundaries,
        }

        path = GOLDEN_DIR / filename
        with open(path, "w") as f:
            json.dump(golden, f, indent=2)
            f.write("\n")

        crits = [f"{b['critical_value']:.6f}" for b in boundaries]
        print(f"  {filename}: K={k}, α={alpha}, {spending_name}")
        print(f"    Boundaries: [{', '.join(crits)}]\n")


if __name__ == "__main__":
    if "--update" in sys.argv:
        generate_all()
        print("Golden files updated.")
    else:
        ok = compare_with_existing()
        sys.exit(0 if ok else 1)
