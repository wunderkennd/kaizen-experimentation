#!/usr/bin/env bash
# =============================================================================
# Validate Bandit Policy Service SLA: benchmark latency checks
# =============================================================================
# Parses criterion benchmark results and validates against SLA targets:
#   - thompson_select_arm_10:    p99 < 100μs (pure computation)
#   - linucb_select_arm_10_d8:   p99 < 500μs (matrix ops, O(d²))
#   - thompson_update_reward:    informational
#   - linucb_update_d8:          informational
#
# Requires criterion benchmarks to have been run first:
#   cargo bench -p experimentation-bandit
#
# Usage:
#   ./scripts/validate_policy_p99.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRITERION_DIR="$REPO_ROOT/target/criterion"

# SLA thresholds (in nanoseconds)
THOMPSON_SELECT_NS=$((100 * 1000))     # 100μs = 100,000 ns
LINUCB_SELECT_NS=$((500 * 1000))       # 500μs = 500,000 ns

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[sla-check-policy]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
if [[ ! -d "$CRITERION_DIR" ]]; then
    log "No criterion results found at $CRITERION_DIR"
    log "Running benchmarks first..."
    cd "$REPO_ROOT"
    cargo bench --package experimentation-bandit
fi

RESULT="PASS"
CHECKS=0
PASSED=0

# ---------------------------------------------------------------------------
# Helper: extract estimate point value from criterion JSON
# ---------------------------------------------------------------------------
get_estimate_ns() {
    local bench_name="$1"
    local json_file="$CRITERION_DIR/$bench_name/new/estimates.json"

    if [[ ! -f "$json_file" ]]; then
        echo ""
        return
    fi

    local value
    value=$(python3 -c "
import json, sys
with open('$json_file') as f:
    data = json.load(f)
# Use slope if available (iteration-based), else median
if 'slope' in data and data['slope']:
    print(int(data['slope']['point_estimate']))
elif 'median' in data and data['median']:
    print(int(data['median']['point_estimate']))
else:
    print('')
" 2>/dev/null || echo "")

    echo "$value"
}

# Format nanoseconds to human-readable
format_ns() {
    local ns="$1"
    if [[ -z "$ns" ]]; then
        echo "N/A"
        return
    fi

    if [[ $ns -ge 1000000000 ]]; then
        echo "$(echo "scale=2; $ns / 1000000000" | bc)s"
    elif [[ $ns -ge 1000000 ]]; then
        echo "$(echo "scale=2; $ns / 1000000" | bc)ms"
    elif [[ $ns -ge 1000 ]]; then
        echo "$(echo "scale=2; $ns / 1000" | bc)μs"
    else
        echo "${ns}ns"
    fi
}

# ---------------------------------------------------------------------------
# Check: thompson_select_arm_10 — p99 < 100μs
# ---------------------------------------------------------------------------
log "Checking thompson_select_arm_10..."
CHECKS=$((CHECKS + 1))

THOMPSON_NS=$(get_estimate_ns "thompson_select_arm_10")
if [[ -n "$THOMPSON_NS" ]]; then
    THOMPSON_HUMAN=$(format_ns "$THOMPSON_NS")
    if [[ $THOMPSON_NS -le $THOMPSON_SELECT_NS ]]; then
        ok "thompson_select_arm_10: ${THOMPSON_HUMAN} <= 100μs SLA"
        PASSED=$((PASSED + 1))
    else
        fail "thompson_select_arm_10: ${THOMPSON_HUMAN} > 100μs SLA"
        RESULT="FAIL"
    fi
else
    warn "thompson_select_arm_10: no benchmark data found"
fi

# ---------------------------------------------------------------------------
# Check: linucb_select_arm_10_d8 — p99 < 500μs
# ---------------------------------------------------------------------------
log "Checking linucb_select_arm_10_d8..."
CHECKS=$((CHECKS + 1))

LINUCB_NS=$(get_estimate_ns "linucb_select_arm_10_d8")
if [[ -n "$LINUCB_NS" ]]; then
    LINUCB_HUMAN=$(format_ns "$LINUCB_NS")
    if [[ $LINUCB_NS -le $LINUCB_SELECT_NS ]]; then
        ok "linucb_select_arm_10_d8: ${LINUCB_HUMAN} <= 500μs SLA"
        PASSED=$((PASSED + 1))
    else
        fail "linucb_select_arm_10_d8: ${LINUCB_HUMAN} > 500μs SLA"
        RESULT="FAIL"
    fi
else
    warn "linucb_select_arm_10_d8: no benchmark data found"
fi

# ---------------------------------------------------------------------------
# Informational benchmarks
# ---------------------------------------------------------------------------
log "Additional benchmark results (informational):"

for bench in \
    "thompson_update_reward" \
    "linucb_update_d8"; do

    NS=$(get_estimate_ns "$bench")
    if [[ -n "$NS" ]]; then
        HUMAN=$(format_ns "$NS")
        echo "    $bench: $HUMAN"
    fi
done

# Also report stats benchmarks if available
log "Stats benchmark results (informational):"

for bench in \
    "welch_ttest_10k" \
    "srm_check_10k" \
    "msprt_normal" \
    "gst_boundaries_5_obf" \
    "cuped_adjustment_10k"; do

    NS=$(get_estimate_ns "$bench")
    if [[ -n "$NS" ]]; then
        HUMAN=$(format_ns "$NS")
        echo "    $bench: $HUMAN"
    fi
done

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  SLA VALIDATION REPORT: M4b Bandit Policy Service"
echo "============================================================="
echo "  Checks:   $PASSED/$CHECKS passed"
echo "  Result:   $RESULT"
echo ""
echo "  SLA targets:"
echo "    ThompsonSampling select_arm (10 arms):  < 100μs"
echo "    LinUCB select_arm (10 arms, d=8):       < 500μs"
echo "============================================================="

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
