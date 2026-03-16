#!/usr/bin/env bash
# =============================================================================
# Load Test: M4b Bandit Policy Service — p99 < 15ms at 10K rps
# =============================================================================
# Phase 4 SLA validation. Builds the release binary, starts the policy server
# (Kafka disabled), seeds LinUCB cold-start experiments via grpcurl, runs k6
# gRPC load at 10K rps for 60s, and validates:
#   - SelectArm p99 < 15ms
#   - Error rate < 0.1%
#
# All experiments use LinUCB (created via CreateColdStartBandit), which requires
# context_features on every SelectArm call — the O(d^2) matrix path per arm.
#
# Prerequisites:
#   - k6 installed (brew install k6)
#   - grpcurl installed (brew install grpcurl)
#   - Rust toolchain (cargo build)
#
# Usage:
#   ./scripts/loadtest_policy.sh
#   TARGET_RPS=5000 ./scripts/loadtest_policy.sh
#   DURATION=30s NUM_EXPERIMENTS=5 ./scripts/loadtest_policy.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PORT="${POLICY_PORT:-50054}"
TARGET_RPS="${TARGET_RPS:-10000}"
DURATION="${DURATION:-60s}"
NUM_EXPERIMENTS="${NUM_EXPERIMENTS:-10}"
POLICY_BIN="$REPO_ROOT/target/release/experimentation-policy"
RESULTS_FILE="$REPO_ROOT/loadtest_policy_results.json"
ROCKSDB_PATH="/tmp/loadtest-policy-rocksdb-$$"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[loadtest-m4b]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

POLICY_PID=""
cleanup() {
    if [[ -n "$POLICY_PID" ]]; then
        kill "$POLICY_PID" 2>/dev/null || true
        wait "$POLICY_PID" 2>/dev/null || true
    fi
    rm -rf "$ROCKSDB_PATH"
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

if ! command -v grpcurl &>/dev/null; then
    fail "grpcurl not found. Install: brew install grpcurl"
    exit 1
fi

# ---------------------------------------------------------------------------
# Build release binary
# ---------------------------------------------------------------------------

if [[ ! -f "$POLICY_BIN" ]]; then
    log "Building policy binary (release mode)..."
    cd "$REPO_ROOT"
    cargo build --release --package experimentation-policy 2>&1 | tail -3
fi
ok "Release binary ready: $POLICY_BIN"

# ---------------------------------------------------------------------------
# Start policy server (Kafka disabled — gRPC only)
# ---------------------------------------------------------------------------

log "Starting policy service on port $PORT (Kafka disabled)..."
POLICY_GRPC_ADDR="0.0.0.0:${PORT}" \
POLICY_ROCKSDB_PATH="$ROCKSDB_PATH" \
KAFKA_BROKERS="localhost:1" \
RUST_LOG=warn \
    "$POLICY_BIN" &
POLICY_PID=$!

# Wait for gRPC health
for i in $(seq 1 30); do
    if grpcurl -plaintext "localhost:${PORT}" list >/dev/null 2>&1; then
        ok "Policy service ready (PID=$POLICY_PID)"
        break
    fi
    if [[ $i -eq 30 ]]; then
        fail "Policy service failed to start within 30s"
        exit 1
    fi
    sleep 1
done

# ---------------------------------------------------------------------------
# Seed LinUCB experiments via CreateColdStartBandit
# ---------------------------------------------------------------------------

log "Seeding $NUM_EXPERIMENTS LinUCB cold-start experiments..."
EXPERIMENT_IDS=""
for i in $(seq 0 $((NUM_EXPERIMENTS - 1))); do
    RESP=$(grpcurl -plaintext \
        -import-path "$REPO_ROOT/proto" \
        -proto "experimentation/bandit/v1/bandit_service.proto" \
        -d "{\"content_id\": \"loadtest-content-${i}\", \"content_metadata\": {\"genre\": \"action\"}, \"window_days\": 30}" \
        "localhost:${PORT}" \
        experimentation.bandit.v1.BanditPolicyService/CreateColdStartBandit 2>/dev/null || echo "")

    if [[ -n "$RESP" ]]; then
        EXP_ID=$(echo "$RESP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('experimentId',''))" 2>/dev/null || echo "")
        if [[ -n "$EXP_ID" ]]; then
            if [[ -n "$EXPERIMENT_IDS" ]]; then
                EXPERIMENT_IDS="${EXPERIMENT_IDS},${EXP_ID}"
            else
                EXPERIMENT_IDS="$EXP_ID"
            fi
        fi
    fi
done

SEEDED=$(echo "$EXPERIMENT_IDS" | tr ',' '\n' | grep -c . || echo "0")
ok "Seeded $SEEDED LinUCB experiments"

if [[ "$SEEDED" -eq 0 ]]; then
    fail "No experiments seeded — cannot run load test"
    exit 1
fi

# ---------------------------------------------------------------------------
# Run k6 load test
# ---------------------------------------------------------------------------

log "Running k6 load test: ${TARGET_RPS} rps for ${DURATION}..."
echo ""

cd "$REPO_ROOT"
k6 run \
    --env "POLICY_ADDR=localhost:${PORT}" \
    --env "TARGET_RPS=${TARGET_RPS}" \
    --env "DURATION=${DURATION}" \
    --env "EXPERIMENT_IDS=${EXPERIMENT_IDS}" \
    scripts/loadtest_policy.js \
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
print(f\"  SelectArm p99:       {r.get('grpc_p99_ms', 'N/A')} ms  (SLA: < 15ms)\")
print(f\"  Total rps achieved:  {r.get('total_rps', 0):.0f}\")
print(f\"  Error rate:          {r.get('select_arm_error_rate', 0)*100:.3f}%\")
" 2>/dev/null || echo "false")

    PASS_LINE=$(echo "$ALL_PASS" | head -1)
    echo "$ALL_PASS" | tail -n +2

    echo ""
    if [[ "$PASS_LINE" == "true" ]]; then
        ok "ALL SLAs MET — p99 < 15ms at ${TARGET_RPS} rps"
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
