#!/usr/bin/env bash
# =============================================================================
# Load Test: M7 Feature Flag Service — p99 < 10ms at 20K rps
# =============================================================================
# Phase 4 SLA validation. Builds the Go binary, starts the flags server with
# in-memory mock store (no database required), seeds test flags, runs k6 at
# 20K rps for 60s, and validates:
#   - EvaluateFlag p99 < 10ms
#   - EvaluateFlags (bulk) p99 < 50ms
#   - Error rate < 0.1%
#
# Prerequisites:
#   - k6 installed (brew install k6)
#   - Go toolchain
#   - curl
#
# Usage:
#   ./scripts/loadtest_flags.sh
#   TARGET_RPS=10000 ./scripts/loadtest_flags.sh
#   DURATION=30s ./scripts/loadtest_flags.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PORT="${FLAGS_PORT:-50057}"
TARGET_RPS="${TARGET_RPS:-20000}"
DURATION="${DURATION:-60s}"
NUM_FLAGS="${NUM_FLAGS:-20}"
FLAGS_URL="http://localhost:${PORT}"
FLAGS_BIN=""
RESULTS_FILE="$REPO_ROOT/loadtest_flags_results.json"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[loadtest-m7]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

FLAGS_PID=""
cleanup() {
    if [[ -n "$FLAGS_PID" ]]; then
        kill "$FLAGS_PID" 2>/dev/null || true
        wait "$FLAGS_PID" 2>/dev/null || true
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

if ! command -v go &>/dev/null; then
    fail "Go not found. Install Go toolchain."
    exit 1
fi

# ---------------------------------------------------------------------------
# Build Go binary
# ---------------------------------------------------------------------------

log "Building flags service binary..."
cd "$REPO_ROOT/services"
go build -o "$REPO_ROOT/target/flags-loadtest" ./flags/cmd/
FLAGS_BIN="$REPO_ROOT/target/flags-loadtest"
ok "Binary ready: $FLAGS_BIN"

# ---------------------------------------------------------------------------
# Start flags server (mock store, no database, no auth)
# ---------------------------------------------------------------------------

log "Starting flags service on port $PORT (mock store, RBAC disabled)..."
PORT="$PORT" \
DISABLE_AUTH=true \
    "$FLAGS_BIN" &
FLAGS_PID=$!

# Wait for health check
for i in $(seq 1 30); do
    if curl -sf "http://localhost:${PORT}/healthz" >/dev/null 2>&1; then
        ok "Flags service ready (PID=$FLAGS_PID)"
        break
    fi
    if [[ $i -eq 30 ]]; then
        fail "Flags service failed to start within 30s"
        exit 1
    fi
    sleep 1
done

# ---------------------------------------------------------------------------
# Seed test flags via ConnectRPC
# ---------------------------------------------------------------------------

log "Seeding $NUM_FLAGS test flags..."
FLAG_IDS=""
for i in $(seq 0 $((NUM_FLAGS - 1))); do
    RESP=$(curl -sf -X POST \
        "${FLAGS_URL}/experimentation.flags.v1.FeatureFlagService/CreateFlag" \
        -H "Content-Type: application/json" \
        -d "{
            \"flag\": {
                \"name\": \"loadtest-flag-${i}\",
                \"type\": 1,
                \"default_value\": \"false\",
                \"enabled\": true,
                \"rollout_percentage\": 0.5
            }
        }" 2>/dev/null || echo "")

    if [[ -n "$RESP" ]]; then
        FID=$(echo "$RESP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('flagId',''))" 2>/dev/null || echo "")
        if [[ -n "$FID" ]]; then
            if [[ -n "$FLAG_IDS" ]]; then
                FLAG_IDS="${FLAG_IDS},${FID}"
            else
                FLAG_IDS="$FID"
            fi
        fi
    fi
done

SEEDED=$(echo "$FLAG_IDS" | tr ',' '\n' | grep -c . || echo "0")
ok "Seeded $SEEDED flags"

if [[ "$SEEDED" -eq 0 ]]; then
    fail "No flags seeded — cannot run load test"
    exit 1
fi

# ---------------------------------------------------------------------------
# Run k6 load test
# ---------------------------------------------------------------------------

log "Running k6 load test: ${TARGET_RPS} rps for ${DURATION}..."
echo ""

cd "$REPO_ROOT"
k6 run \
    --env "FLAGS_URL=${FLAGS_URL}" \
    --env "TARGET_RPS=${TARGET_RPS}" \
    --env "DURATION=${DURATION}" \
    --env "FLAG_IDS=${FLAG_IDS}" \
    scripts/loadtest_flags.js \
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
print(f\"  EvaluateFlag p99:    {r.get('eval_p99_ms', 'N/A')} ms  (SLA: < 10ms)\")
print(f\"  EvaluateFlags p99:   {r.get('bulk_p99_ms', 'N/A')} ms  (SLA: < 50ms)\")
print(f\"  Total rps achieved:  {r.get('total_rps', 0):.0f}\")
print(f\"  Error rate:          {r.get('eval_error_rate', 0)*100:.3f}%\")
" 2>/dev/null || echo "false")

    PASS_LINE=$(echo "$ALL_PASS" | head -1)
    echo "$ALL_PASS" | tail -n +2

    echo ""
    if [[ "$PASS_LINE" == "true" ]]; then
        ok "ALL SLAs MET — p99 < 10ms at ${TARGET_RPS} rps"
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
