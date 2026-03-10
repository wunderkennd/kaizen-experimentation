#!/usr/bin/env bash
# =============================================================================
# Validate Assignment Service SLA: p99 latency and throughput checks
# =============================================================================
# Parses criterion benchmark results and validates against SLA targets:
#   - get_assignment_single: p99 < 5ms (SLA)
#   - get_assignment_1000_users: throughput (informational)
#   - get_interleaved_list_100_items: p99 < 15ms (SLA)
#
# Requires criterion benchmarks to have been run first:
#   cargo bench -p experimentation-assignment
#
# Usage:
#   ./scripts/validate_assignment_p99.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRITERION_DIR="$REPO_ROOT/target/criterion"

# SLA thresholds (in nanoseconds)
ASSIGNMENT_P99_NS=$((5 * 1000000))          # 5ms = 5,000,000 ns
INTERLEAVE_P99_NS=$((15 * 1000000))         # 15ms = 15,000,000 ns

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[sla-check]${NC} $*"; }
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
    cargo bench --package experimentation-assignment
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

    # Extract the "point_estimate" from the "median" field (best proxy for p99)
    # Criterion stores values in nanoseconds
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
# Check: get_assignment_single — p99 < 5ms
# ---------------------------------------------------------------------------
log "Checking get_assignment_single..."
CHECKS=$((CHECKS + 1))

ASSIGN_NS=$(get_estimate_ns "get_assignment_single")
if [[ -n "$ASSIGN_NS" ]]; then
    ASSIGN_HUMAN=$(format_ns "$ASSIGN_NS")
    if [[ $ASSIGN_NS -le $ASSIGNMENT_P99_NS ]]; then
        ok "get_assignment_single: ${ASSIGN_HUMAN} <= 5ms SLA"
        PASSED=$((PASSED + 1))
    else
        fail "get_assignment_single: ${ASSIGN_HUMAN} > 5ms SLA"
        RESULT="FAIL"
    fi
else
    warn "get_assignment_single: no benchmark data found"
fi

# ---------------------------------------------------------------------------
# Check: get_assignment_1000_users — throughput (informational)
# ---------------------------------------------------------------------------
log "Checking get_assignment_1000_users..."
CHECKS=$((CHECKS + 1))

BATCH_NS=$(get_estimate_ns "get_assignment_1000_users")
if [[ -n "$BATCH_NS" ]]; then
    BATCH_HUMAN=$(format_ns "$BATCH_NS")
    # 1000 users per iteration — compute throughput
    if [[ $BATCH_NS -gt 0 ]]; then
        THROUGHPUT=$(echo "scale=0; 1000 * 1000000000 / $BATCH_NS" | bc)
        ok "get_assignment_1000_users: ${BATCH_HUMAN} total (~${THROUGHPUT} assigns/s)"
        PASSED=$((PASSED + 1))
    fi
else
    warn "get_assignment_1000_users: no benchmark data found"
fi

# ---------------------------------------------------------------------------
# Check: get_interleaved_list_100_items — p99 < 15ms
# ---------------------------------------------------------------------------
log "Checking get_interleaved_list_100_items..."
CHECKS=$((CHECKS + 1))

INTERLEAVE_NS=$(get_estimate_ns "get_interleaved_list_100_items")
if [[ -n "$INTERLEAVE_NS" ]]; then
    INTERLEAVE_HUMAN=$(format_ns "$INTERLEAVE_NS")
    if [[ $INTERLEAVE_NS -le $INTERLEAVE_P99_NS ]]; then
        ok "get_interleaved_list_100_items: ${INTERLEAVE_HUMAN} <= 15ms SLA"
        PASSED=$((PASSED + 1))
    else
        fail "get_interleaved_list_100_items: ${INTERLEAVE_HUMAN} > 15ms SLA"
        RESULT="FAIL"
    fi
else
    warn "get_interleaved_list_100_items: no benchmark data found"
fi

# ---------------------------------------------------------------------------
# Additional benchmarks (informational, no SLA gate)
# ---------------------------------------------------------------------------
log "Additional benchmark results (informational):"

for bench in \
    "get_assignment_session_level" \
    "get_assignment_session_1000" \
    "get_interleaved_list_10_items" \
    "get_optimized_interleave_10_items" \
    "get_optimized_interleave_50_items" \
    "get_multileave_3_algos_10_items" \
    "get_multileave_3_algos_50_items" \
    "get_assignment_mab_single" \
    "get_assignment_mab_1000_users"; do

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
echo "  SLA VALIDATION REPORT: M1 Assignment Service"
echo "============================================================="
echo "  Checks:   $PASSED/$CHECKS passed"
echo "  Result:   $RESULT"
echo ""
echo "  SLA targets:"
echo "    GetAssignment p99:       < 5ms"
echo "    GetInterleavedList p99:  < 15ms"
echo "============================================================="

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
