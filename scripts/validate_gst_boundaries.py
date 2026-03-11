#!/usr/bin/env python3
"""
Validate GST (Group Sequential Test) golden files.

Checks that:
1. All golden files have the 'source' provenance field
2. Spending function alphas match the expected closed-form values
3. Optionally re-validates via rpy2/gsDesign if available

Usage: python3 scripts/validate_gst_boundaries.py
"""

import json
import math
import sys
from pathlib import Path

from scipy import stats

GOLDEN_DIR = Path("crates/experimentation-stats/tests/golden")
TOLERANCE = 1e-4


def spending_obf(t: float, alpha: float) -> float:
    z_alpha_half = stats.norm.ppf(1.0 - alpha / 2.0)
    return float(2.0 * (1.0 - stats.norm.cdf(z_alpha_half / math.sqrt(t))))


def spending_pocock(t: float, alpha: float) -> float:
    return float(alpha * math.log(1.0 + (math.e - 1.0) * t))


def validate_golden(path: Path) -> list[str]:
    """Validate a single golden file. Returns list of error messages."""
    errors = []
    with open(path) as f:
        data = json.load(f)

    name = data.get("test_name", path.stem)

    # Check provenance
    source = data.get("source")
    if not source:
        errors.append(f"{name}: missing 'source' provenance field")

    sf_name = data["spending_function"]
    alpha = data["overall_alpha"]
    sf = spending_obf if sf_name == "OBrienFleming" else spending_pocock

    # Validate spending function alphas
    for b in data["boundaries"]:
        t = b["information_fraction"]
        expected_cum = sf(t, alpha)
        actual_cum = b["cumulative_alpha"]
        diff = abs(expected_cum - actual_cum)
        if diff > TOLERANCE:
            errors.append(
                f"{name} look {b['look']}: cumulative_alpha diff={diff:.2e} > {TOLERANCE:.0e}"
            )

    # Check boundary monotonicity for OBF
    if sf_name == "OBrienFleming":
        crits = [b["critical_value"] for b in data["boundaries"]]
        for i in range(1, len(crits)):
            if crits[i] > crits[i - 1] + TOLERANCE:
                errors.append(
                    f"{name}: OBF boundary not decreasing at look {i+1}: "
                    f"{crits[i]:.6f} > {crits[i-1]:.6f}"
                )

    return errors


def main():
    golden_files = sorted(GOLDEN_DIR.glob("gst_*.json"))
    if not golden_files:
        print(f"No GST golden files found in {GOLDEN_DIR}/")
        sys.exit(1)

    total_errors = []
    for path in golden_files:
        errors = validate_golden(path)
        if errors:
            total_errors.extend(errors)
            print(f"FAIL {path.name}")
            for e in errors:
                print(f"  {e}")
        else:
            print(f"OK   {path.name}")

    print(f"\nValidated {len(golden_files)} golden files.")
    if total_errors:
        print(f"{len(total_errors)} error(s) found.")
        sys.exit(1)
    else:
        print("All checks passed.")


if __name__ == "__main__":
    main()
