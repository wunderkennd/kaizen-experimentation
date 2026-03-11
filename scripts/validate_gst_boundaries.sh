#!/usr/bin/env bash
# Validate GST boundaries against reference implementations.
#
# 1. Runs the Python validator (spending function + provenance checks)
# 2. If R is available, regenerates golden files from gsDesign and diffs
#
# Usage: ./scripts/validate_gst_boundaries.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GOLDEN_DIR="crates/experimentation-stats/tests/golden"

echo "=== GST Boundary Validation ==="
echo

# Step 1: Python validation
echo "--- Python validation ---"
python3 "$SCRIPT_DIR/validate_gst_boundaries.py"
echo

# Step 2: R validation (if available)
if command -v Rscript &>/dev/null; then
    echo "--- R/gsDesign validation ---"
    TMPDIR=$(mktemp -d)
    trap "rm -rf $TMPDIR" EXIT

    # Generate reference files to a temp directory, then diff
    R_SCRIPT="$SCRIPT_DIR/generate_gst_golden.R"
    if [ -f "$R_SCRIPT" ]; then
        # Temporarily redirect output to temp dir
        sed "s|crates/experimentation-stats/tests/golden|$TMPDIR|g" "$R_SCRIPT" > "$TMPDIR/gen.R"
        Rscript "$TMPDIR/gen.R"

        echo
        echo "Comparing R output with existing golden files:"
        DIFFS=0
        for f in "$TMPDIR"/gst_*.json; do
            base=$(basename "$f")
            existing="$GOLDEN_DIR/$base"
            if [ -f "$existing" ]; then
                if diff -q "$f" "$existing" >/dev/null 2>&1; then
                    echo "  MATCH $base"
                else
                    echo "  DIFF  $base"
                    DIFFS=$((DIFFS + 1))
                fi
            else
                echo "  NEW   $base (no existing file)"
            fi
        done

        if [ "$DIFFS" -gt 0 ]; then
            echo
            echo "WARNING: $DIFFS file(s) differ from R output."
            echo "Run 'Rscript $R_SCRIPT' to update golden files from gsDesign."
        fi
    else
        echo "R script not found: $R_SCRIPT"
    fi
else
    echo "--- R not available, skipping gsDesign cross-check ---"
    echo "Install R + gsDesign to enable: install.packages('gsDesign')"
fi

echo
echo "=== Done ==="
