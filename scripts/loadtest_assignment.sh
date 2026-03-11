#!/usr/bin/env bash
# =============================================================================
# Load Test: M1 Assignment Service — p99 < 5ms at 10K rps
# =============================================================================
# Phase 1 SLA validation. Builds the release binary, starts the server,
# runs k6 gRPC load at 10K rps for 60s, and validates:
#   - GetAssignment p99 < 5ms
#   - GetInterleavedList p99 < 15ms
#   - Error rate < 0.1%
#
# Prerequisites:
#   - k6 installed (brew install k6)
#   - Rust toolchain (cargo build)
#
# Usage:
#   ./scripts/loadtest_assignment.sh
#   TARGET_RPS=5000 ./scripts/loadtest_assignment.sh
#   DURATION=30s ./scripts/loadtest_assignment.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PORT="${ASSIGNMENT_PORT:-50051}"
TARGET_RPS="${TARGET_RPS:-10000}"
DURATION="${DURATION:-60s}"
CONFIG_PATH="${CONFIG_PATH:-$REPO_ROOT/dev/config.json}"
ASSIGNMENT_BIN="$REPO_ROOT/target/release/experimentation-assignment"
RESULTS_FILE="$REPO_ROOT/loadtest_assignment_results.json"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[loadtest-m1]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

ASSIGNMENT_PID=""
cleanup() {
    if [[ -n "$ASSIGNMENT_PID" ]]; then
        kill "$ASSIGNMENT_PID" 2>/dev/null || true
        wait "$ASSIGNMENT_PID" 2>/dev/null || true
    fi
    rm -f "$RESULTS_FILE"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

if ! command -v k6 &>/dev/null; then
    fail "k6 not found. Install: brew install k6"
    exit 1
fi

if [[ ! -f "$CONFIG_PATH" ]]; then
    fail "Config file not found: $CONFIG_PATH"
    exit 1
fi

# ---------------------------------------------------------------------------
# Build release binary
# ---------------------------------------------------------------------------

if [[ ! -f "$ASSIGNMENT_BIN" ]]; then
    log "Building assignment binary (release mode)..."
    cd "$REPO_ROOT"
    cargo build --release --package experimentation-assignment 2>&1 | tail -3
fi
ok "Release binary ready: $ASSIGNMENT_BIN"

# ---------------------------------------------------------------------------
# Start assignment server
# ---------------------------------------------------------------------------

log "Starting assignment service on port $PORT..."
CONFIG_PATH="$CONFIG_PATH" \
GRPC_ADDR="0.0.0.0:${PORT}" \
RUST_LOG=warn \
    "$ASSIGNMENT_BIN" &
ASSIGNMENT_PID=$!

# Wait for gRPC health
for i in $(seq 1 30); do
    if grpcurl -plaintext "localhost:${PORT}" list >/dev/null 2>&1; then
        ok "Assignment service ready (PID=$ASSIGNMENT_PID)"
        break
    fi
    if [[ $i -eq 30 ]]; then
        fail "Assignment service failed to start within 30s"
        exit 1
    fi
    sleep 1
done

# ---------------------------------------------------------------------------
# Run k6 load test
# ---------------------------------------------------------------------------

log "Running k6 load test: ${TARGET_RPS} rps for ${DURATION}..."
echo ""

cd "$REPO_ROOT"
k6 run \
    --env "ASSIGNMENT_ADDR=localhost:${PORT}" \
    --env "TARGET_RPS=${TARGET_RPS}" \
    --env "DURATION=${DURATION}" \
    scripts/loadtest_assignment.js \
    2>&1

echo ""

# ---------------------------------------------------------------------------
# Validate results
# ---------------------------------------------------------------------------

if [[ -f "$RESULTS_FILE" ]]; then
    ALL_PASS=$(python3 -c "
import json, sys
with open('$RESULTS_FILE') as f:
    r = json.load(f)
print('true' if r.get('all_pass', False) else 'false')
print(f\"  GetAssignment p99:      {r.get('assign_p99_ms', 'N/A')} ms\")
print(f\"  GetInterleavedList p99: {r.get('interleave_p99_ms', 'N/A')} ms\")
print(f\"  Total rps achieved:     {r.get('total_rps', 0):.0f}\")
print(f\"  Error rate:             {r.get('assign_error_rate', 0)*100:.3f}%\")
" 2>/dev/null || echo "false")

    PASS_LINE=$(echo "$ALL_PASS" | head -1)
    echo "$ALL_PASS" | tail -n +2

    echo ""
    if [[ "$PASS_LINE" == "true" ]]; then
        ok "ALL SLAs MET — p99 < 5ms at ${TARGET_RPS} rps"
        exit 0
    else
        fail "SLA VIOLATION — see report above"
        exit 1
    fi
else
    warn "No results file found — k6 may have reported inline"
    # k6 thresholds cause exit code 99 on failure
    exit 0
fi
