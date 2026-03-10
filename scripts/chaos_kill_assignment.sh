#!/usr/bin/env bash
# =============================================================================
# Chaos Test: Kill-9 Assignment Service Under Load
# =============================================================================
# Phase 4.5 chaos engineering for M1 Assignment Service.
#
# Verifies crash-only recovery properties:
#   1. Start assignment service with local JSON config
#   2. Record baseline assignments for N users (determinism baseline)
#   3. Send sustained GetAssignment load
#   4. kill -9 the process mid-load
#   5. Restart the process
#   6. Measure recovery time (must be < 2s)
#   7. Verify assignment determinism: same user+experiment → same variant
#   8. Verify all experiment types work (AB, SESSION_LEVEL, MAB)
#
# The assignment service is fully stateless — no disk state, no warm-up.
# Deterministic MurmurHash3 bucketing guarantees identical assignments
# before and after any crash/restart.
#
# Prerequisites:
#   - cargo build --package experimentation-assignment --release
#   - grpcurl installed (brew install grpcurl)
#   - dev/config.json present
#
# Usage:
#   ./scripts/chaos_kill_assignment.sh
#   ./scripts/chaos_kill_assignment.sh --duration 10 --kill-after 5
#   ./scripts/chaos_kill_assignment.sh --port 50052 --requests-per-sec 200
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
ASSIGNMENT_PORT=${ASSIGNMENT_PORT:-50051}
TOTAL_DURATION=${TOTAL_DURATION:-10}
KILL_AFTER_SECS=${KILL_AFTER_SECS:-5}
REQUESTS_PER_SEC=${REQUESTS_PER_SEC:-100}
RECOVERY_SLA_MS=${RECOVERY_SLA_MS:-2000}
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ASSIGNMENT_BIN="$REPO_ROOT/target/release/experimentation-assignment"
CONFIG_PATH="$REPO_ROOT/dev/config.json"
WORK_DIR=$(mktemp -d)

# Number of test users for determinism check
DETERMINISM_USERS=10

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[chaos-assign]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

ASSIGNMENT_PID=""
cleanup() {
    if [[ -n "$ASSIGNMENT_PID" ]]; then
        kill "$ASSIGNMENT_PID" 2>/dev/null || true
        wait "$ASSIGNMENT_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --port)             ASSIGNMENT_PORT="$2"; shift 2 ;;
        --duration)         TOTAL_DURATION="$2"; shift 2 ;;
        --kill-after)       KILL_AFTER_SECS="$2"; shift 2 ;;
        --requests-per-sec) REQUESTS_PER_SEC="$2"; shift 2 ;;
        --recovery-sla)     RECOVERY_SLA_MS="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --port PORT             gRPC port (default: 50051)"
            echo "  --duration SECS         Total load duration (default: 10)"
            echo "  --kill-after SECS       Kill after N seconds of load (default: 5)"
            echo "  --requests-per-sec N    Request rate (default: 100)"
            echo "  --recovery-sla MS       Max recovery time in ms (default: 2000)"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Helper: call GetAssignment
# ---------------------------------------------------------------------------
get_assignment() {
    local user_id="$1"
    local experiment_id="$2"
    local session_id="${3:-}"

    local payload
    if [[ -n "$session_id" ]]; then
        payload="{\"user_id\":\"$user_id\",\"experiment_id\":\"$experiment_id\",\"session_id\":\"$session_id\"}"
    else
        payload="{\"user_id\":\"$user_id\",\"experiment_id\":\"$experiment_id\"}"
    fi

    grpcurl -plaintext -d "$payload" \
        "localhost:${ASSIGNMENT_PORT}" \
        experimentation.assignment.v1.AssignmentService/GetAssignment 2>&1
}

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
log "Chaos test: kill-9 assignment service under ${REQUESTS_PER_SEC} req/s load"
log "Config: duration=${TOTAL_DURATION}s, kill after ${KILL_AFTER_SECS}s, port=${ASSIGNMENT_PORT}"

if ! command -v grpcurl &>/dev/null; then
    fail "grpcurl not found. Install: brew install grpcurl"
    exit 1
fi

if [[ ! -f "$CONFIG_PATH" ]]; then
    fail "Config file not found: $CONFIG_PATH"
    exit 1
fi

if [[ ! -f "$ASSIGNMENT_BIN" ]]; then
    log "Building assignment binary (release mode)..."
    (cd "$REPO_ROOT" && cargo build --package experimentation-assignment --release 2>&1 | tail -5)
fi

mkdir -p "$WORK_DIR"

# ---------------------------------------------------------------------------
# Helper: start assignment service
# ---------------------------------------------------------------------------
start_assignment() {
    CONFIG_PATH="$CONFIG_PATH" \
    GRPC_ADDR="0.0.0.0:${ASSIGNMENT_PORT}" \
    RUST_LOG=warn \
    "$ASSIGNMENT_BIN" > "$WORK_DIR/assignment.log" 2>&1 &
    ASSIGNMENT_PID=$!
}

# Helper: wait for gRPC health
wait_healthy() {
    local max_wait=${1:-30}
    for i in $(seq 1 "$max_wait"); do
        if grpcurl -plaintext "localhost:${ASSIGNMENT_PORT}" list >/dev/null 2>&1; then
            return 0
        fi
        if [[ $i -eq "$max_wait" ]]; then
            return 1
        fi
        sleep 1
    done
}

# ---------------------------------------------------------------------------
# Phase 1: Start assignment service
# ---------------------------------------------------------------------------
log "Starting assignment service on port ${ASSIGNMENT_PORT}..."
start_assignment

if wait_healthy 30; then
    ok "Assignment service ready (PID=$ASSIGNMENT_PID)"
else
    fail "Assignment service failed to start within 30s"
    exit 1
fi

# ---------------------------------------------------------------------------
# Phase 2: Record baseline assignments (determinism baseline)
# ---------------------------------------------------------------------------
log "Recording baseline assignments for $DETERMINISM_USERS users..."
mkdir -p "$WORK_DIR/baseline"

BASELINE_OK=true
for i in $(seq 1 "$DETERMINISM_USERS"); do
    user_id="chaos-determinism-user-$i"

    # AB test (exp_dev_001)
    result=$(get_assignment "$user_id" "exp_dev_001")
    variant=$(echo "$result" | grep '"variantId"' | sed 's/.*"variantId": *"//;s/".*//')
    echo "$variant" > "$WORK_DIR/baseline/ab_user_$i"

    # Session-level test (exp_dev_003) — use a fixed session_id
    result=$(get_assignment "$user_id" "exp_dev_003" "chaos-session-$i")
    variant=$(echo "$result" | grep '"variantId"' | sed 's/.*"variantId": *"//;s/".*//')
    echo "$variant" > "$WORK_DIR/baseline/session_user_$i"

    # MAB test (exp_dev_005) — returns uniform random fallback (no M4b)
    result=$(get_assignment "$user_id" "exp_dev_005")
    if echo "$result" | grep -q '"variantId"'; then
        echo "ok" > "$WORK_DIR/baseline/mab_user_$i"
    else
        echo "fail" > "$WORK_DIR/baseline/mab_user_$i"
        BASELINE_OK=false
    fi
done

if $BASELINE_OK; then
    ok "Baseline assignments recorded for all $DETERMINISM_USERS users (AB + SESSION + MAB)"
else
    warn "Some MAB baseline assignments failed (expected if no M4b)"
fi

# ---------------------------------------------------------------------------
# Phase 3: Send sustained load
# ---------------------------------------------------------------------------
log "Sending load at ~${REQUESTS_PER_SEC} req/s for ${KILL_AFTER_SECS}s before kill..."

LOAD_SENT=0
LOAD_ERRORS=0

send_load() {
    local duration="$1"
    local end_time=$(( $(date +%s) + duration ))
    local sent=0
    local errors=0

    while [[ $(date +%s) -lt $end_time ]]; do
        for _ in $(seq 1 "$REQUESTS_PER_SEC"); do
            local user_id="chaos-load-user-$((RANDOM % 10000))"
            local exp_id="exp_dev_00$((RANDOM % 3 + 1))"

            grpcurl -plaintext -d "{
                \"user_id\": \"$user_id\",
                \"experiment_id\": \"$exp_id\"
            }" "localhost:${ASSIGNMENT_PORT}" \
                experimentation.assignment.v1.AssignmentService/GetAssignment \
                >/dev/null 2>&1 && sent=$((sent + 1)) || errors=$((errors + 1)) &
        done
        wait 2>/dev/null || true
        sleep 1
    done

    echo "$sent" > "$WORK_DIR/load_sent"
    echo "$errors" > "$WORK_DIR/load_errors"
}

# Start background load for full duration
send_load "$TOTAL_DURATION" &
LOAD_PID=$!

# Let load run before kill
sleep "$KILL_AFTER_SECS"

# ---------------------------------------------------------------------------
# Phase 4: KILL -9 (the chaos)
# ---------------------------------------------------------------------------
log "Sending SIGKILL to assignment service (PID=$ASSIGNMENT_PID)..."
kill -9 "$ASSIGNMENT_PID" 2>/dev/null || true
wait "$ASSIGNMENT_PID" 2>/dev/null || true
ok "Assignment service killed"

# Brief pause — load generator will see connection errors during this window
sleep 1

# ---------------------------------------------------------------------------
# Phase 5: Restart and measure recovery time
# ---------------------------------------------------------------------------
log "Restarting assignment service..."
RECOVERY_START=$(date +%s%N)

start_assignment

# Tight polling for recovery measurement
RECOVERY_MS=99999
for i in $(seq 1 200); do
    if grpcurl -plaintext "localhost:${ASSIGNMENT_PORT}" list >/dev/null 2>&1; then
        RECOVERY_END=$(date +%s%N)
        RECOVERY_MS=$(( (RECOVERY_END - RECOVERY_START) / 1000000 ))
        ok "Assignment service recovered in ${RECOVERY_MS}ms (PID=$ASSIGNMENT_PID)"
        break
    fi
    sleep 0.1
done

if [[ $RECOVERY_MS -eq 99999 ]]; then
    fail "Assignment service failed to recover within 20s"
    exit 1
fi

# Wait for remaining load to finish
wait "$LOAD_PID" 2>/dev/null || true

LOAD_SENT=$(cat "$WORK_DIR/load_sent" 2>/dev/null || echo 0)
LOAD_ERRORS=$(cat "$WORK_DIR/load_errors" 2>/dev/null || echo 0)

# ---------------------------------------------------------------------------
# Phase 6: Verify assignment determinism after recovery
# ---------------------------------------------------------------------------
log "Verifying assignment determinism after crash recovery..."

DETERMINISM_PASS=0
DETERMINISM_FAIL=0

for i in $(seq 1 "$DETERMINISM_USERS"); do
    user_id="chaos-determinism-user-$i"

    # AB test determinism
    result=$(get_assignment "$user_id" "exp_dev_001")
    variant=$(echo "$result" | grep '"variantId"' | sed 's/.*"variantId": *"//;s/".*//')
    expected=$(cat "$WORK_DIR/baseline/ab_user_$i")

    if [[ "$variant" == "$expected" ]]; then
        DETERMINISM_PASS=$((DETERMINISM_PASS + 1))
    else
        DETERMINISM_FAIL=$((DETERMINISM_FAIL + 1))
        fail "AB determinism mismatch for user $i: expected=$expected got=$variant"
    fi

    # Session-level determinism
    result=$(get_assignment "$user_id" "exp_dev_003" "chaos-session-$i")
    variant=$(echo "$result" | grep '"variantId"' | sed 's/.*"variantId": *"//;s/".*//')
    expected=$(cat "$WORK_DIR/baseline/session_user_$i")

    if [[ "$variant" == "$expected" ]]; then
        DETERMINISM_PASS=$((DETERMINISM_PASS + 1))
    else
        DETERMINISM_FAIL=$((DETERMINISM_FAIL + 1))
        fail "SESSION determinism mismatch for user $i: expected=$expected got=$variant"
    fi
done

if [[ $DETERMINISM_FAIL -eq 0 ]]; then
    ok "Determinism verified: $DETERMINISM_PASS/$DETERMINISM_PASS assignments match (AB + SESSION)"
else
    fail "Determinism failures: $DETERMINISM_FAIL mismatches"
fi

# ---------------------------------------------------------------------------
# Phase 7: Verify all experiment types work post-recovery
# ---------------------------------------------------------------------------
log "Verifying all experiment types post-recovery..."

TYPES_PASS=0
TYPES_FAIL=0

# AB test
result=$(get_assignment "chaos-type-verify" "exp_dev_001")
if echo "$result" | grep -q '"variantId"'; then
    ok "AB experiment (exp_dev_001) serving assignments"
    TYPES_PASS=$((TYPES_PASS + 1))
else
    fail "AB experiment (exp_dev_001) not returning assignments"
    TYPES_FAIL=$((TYPES_FAIL + 1))
fi

# Session-level test
result=$(get_assignment "chaos-type-verify" "exp_dev_003" "chaos-session-verify")
if echo "$result" | grep -q '"variantId"'; then
    ok "SESSION_LEVEL experiment (exp_dev_003) serving assignments"
    TYPES_PASS=$((TYPES_PASS + 1))
else
    fail "SESSION_LEVEL experiment (exp_dev_003) not returning assignments"
    TYPES_FAIL=$((TYPES_FAIL + 1))
fi

# MAB test (uniform random fallback without M4b)
result=$(get_assignment "chaos-type-verify" "exp_dev_005")
if echo "$result" | grep -q '"variantId"'; then
    ok "MAB experiment (exp_dev_005) serving assignments (uniform fallback)"
    TYPES_PASS=$((TYPES_PASS + 1))
else
    fail "MAB experiment (exp_dev_005) not returning assignments"
    TYPES_FAIL=$((TYPES_FAIL + 1))
fi

# ---------------------------------------------------------------------------
# Phase 8: Report
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  CHAOS TEST REPORT: kill -9 Assignment Service"
echo "============================================================="
echo "  Recovery time:              ${RECOVERY_MS}ms"
echo "  Recovery SLA:               ${RECOVERY_SLA_MS}ms"
echo "  Load requests sent:         ${LOAD_SENT}"
echo "  Load errors (during kill):  ${LOAD_ERRORS}"
echo "  Determinism checks:         ${DETERMINISM_PASS} pass / ${DETERMINISM_FAIL} fail"
echo "  Experiment types verified:  ${TYPES_PASS}/3"
echo ""

RESULT="PASS"

# Recovery SLA check
if [[ $RECOVERY_MS -le $RECOVERY_SLA_MS ]]; then
    ok "PASS: Recovery ${RECOVERY_MS}ms <= ${RECOVERY_SLA_MS}ms SLA"
else
    fail "FAIL: Recovery ${RECOVERY_MS}ms > ${RECOVERY_SLA_MS}ms SLA"
    RESULT="FAIL"
fi

# Determinism check
if [[ $DETERMINISM_FAIL -eq 0 ]]; then
    ok "PASS: All assignments deterministic after crash"
else
    fail "FAIL: ${DETERMINISM_FAIL} determinism mismatches"
    RESULT="FAIL"
fi

# Type check
if [[ $TYPES_FAIL -eq 0 ]]; then
    ok "PASS: All experiment types serving post-recovery"
else
    fail "FAIL: ${TYPES_FAIL}/3 experiment types failed"
    RESULT="FAIL"
fi

echo "============================================================="
echo ""

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
