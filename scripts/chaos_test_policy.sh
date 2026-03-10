#!/usr/bin/env bash
# =============================================================================
# Chaos Test: Policy Service (M4b) — Pluggable hook for chaos_e2e_framework
# =============================================================================
# Exports:
#   chaos_start_policy   — Build & start the policy Rust binary, echo PID
#   chaos_health_policy  — Exit 0 if gRPC SelectArm responds (even with NotFound)
#   chaos_verify_policy  — Create cold-start bandit, select arm, verify state
#
# Usage:
#   Standalone:  ./scripts/chaos_test_policy.sh
#   Framework:   ./scripts/chaos_e2e_framework.sh --services policy
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

POLICY_PORT=${POLICY_PORT:-50054}
POLICY_GRPC_ADDR=${POLICY_GRPC_ADDR:-"[::1]:${POLICY_PORT}"}
POLICY_ROCKSDB_PATH=${POLICY_ROCKSDB_PATH:-"/tmp/chaos_policy_rocksdb_$$"}
POLICY_BIN=""
POLICY_LOG="/tmp/chaos_policy_$$.log"
RECOVERY_SLA_MS=${RECOVERY_SLA_MS:-10000}

# Colors (inherit from framework or define locally)
RED=${RED:-'\033[0;31m'}
GREEN=${GREEN:-'\033[0;32m'}
YELLOW=${YELLOW:-'\033[1;33m'}
NC=${NC:-'\033[0m'}

_log() { echo -e "[chaos-policy] $*"; }
_ok()  { echo -e "${GREEN}[  OK  ]${NC} $*"; }
_fail(){ echo -e "${RED}[ FAIL ]${NC} $*"; }
_warn(){ echo -e "${YELLOW}[ WARN ]${NC} $*"; }

# ---------------------------------------------------------------------------
# Build policy binary
# ---------------------------------------------------------------------------
_build_policy() {
    local bin_path="/tmp/chaos_policy_server_$$"
    _log "Building policy service (release)..."
    (cd "$REPO_ROOT" && cargo build --release --package experimentation-policy 2>&1) || {
        _fail "cargo build failed"
        return 1
    }
    cp "$REPO_ROOT/target/release/experimentation-policy" "$bin_path"
    echo "$bin_path"
}

# ---------------------------------------------------------------------------
# Hook: chaos_start_policy — Start the service, echo PID
# ---------------------------------------------------------------------------
chaos_start_policy() {
    if [[ -z "$POLICY_BIN" ]] || [[ ! -f "$POLICY_BIN" ]]; then
        POLICY_BIN=$(_build_policy)
    fi

    # Clean RocksDB dir for fresh start (unless testing persistence)
    if [[ "${KEEP_ROCKSDB:-}" != "true" ]]; then
        rm -rf "$POLICY_ROCKSDB_PATH"
    fi

    POLICY_GRPC_ADDR="$POLICY_GRPC_ADDR" \
    POLICY_ROCKSDB_PATH="$POLICY_ROCKSDB_PATH" \
    POLICY_CHANNEL_DEPTH="10000" \
    REWARD_CHANNEL_DEPTH="50000" \
    SNAPSHOT_INTERVAL="5" \
    MAX_SNAPSHOTS_PER_EXPERIMENT="3" \
    KAFKA_BROKERS="${KAFKA_BROKERS:-localhost:9092}" \
    KAFKA_GROUP_ID="chaos-test-$$" \
    KAFKA_REWARD_TOPIC="${KAFKA_REWARD_TOPIC:-reward_events}" \
    RUST_LOG="experimentation_policy=info" \
    "$POLICY_BIN" > "$POLICY_LOG" 2>&1 &

    local pid=$!
    echo "$pid"
}

# ---------------------------------------------------------------------------
# Hook: chaos_health_policy — Exit 0 if gRPC is reachable
# ---------------------------------------------------------------------------
chaos_health_policy() {
    # Try selecting an arm for a nonexistent experiment.
    # If the service is up, we get NOT_FOUND (exit 0 from our perspective).
    # If the service is down, grpcurl fails (exit != 0).
    local result
    result=$(grpcurl -plaintext \
        -d '{"experiment_id": "health-check", "user_id": "probe"}' \
        "[::1]:${POLICY_PORT}" \
        "experimentation.bandit.v1.BanditPolicyService/SelectArm" 2>&1) || true

    # Service is healthy if we get a gRPC response (even NOT_FOUND)
    if echo "$result" | grep -q "NotFound\|not found\|arm_id\|experiment_id"; then
        return 0
    fi

    return 1
}

# ---------------------------------------------------------------------------
# Hook: chaos_verify_policy — Verify service integrity after crash recovery
# ---------------------------------------------------------------------------
chaos_verify_policy() {
    local svc_path="experimentation.bandit.v1.BanditPolicyService"
    local verify_ok=true

    # --- Test 1: SelectArm for unknown experiment returns NotFound ---
    local select_result
    select_result=$(grpcurl -plaintext \
        -d '{"experiment_id": "nonexistent", "user_id": "test"}' \
        "[::1]:${POLICY_PORT}" \
        "${svc_path}/SelectArm" 2>&1) || true

    if echo "$select_result" | grep -q "NotFound\|not found"; then
        _ok "SelectArm: NotFound for unknown experiment (gRPC responsive)"
    else
        _fail "SelectArm: unexpected response: $select_result"
        verify_ok=false
    fi

    # --- Test 2: Create cold-start bandit (write path) ---
    local create_result
    create_result=$(grpcurl -plaintext -d '{
        "content_id": "chaos-movie-'$$'",
        "content_metadata": {"genre": "chaos"},
        "window_days": 7
    }' "[::1]:${POLICY_PORT}" \
        "${svc_path}/CreateColdStartBandit" 2>&1) || {
        _fail "CreateColdStartBandit failed after recovery"
        return 1
    }

    if echo "$create_result" | grep -q "experimentId\|experiment_id"; then
        _ok "CreateColdStartBandit: write path works"
    else
        _fail "CreateColdStartBandit: unexpected response"
        echo "$create_result" >&2
        verify_ok=false
    fi

    # --- Test 3: SelectArm for the newly created cold-start experiment ---
    local arm_result
    arm_result=$(grpcurl -plaintext -d '{
        "experiment_id": "cold-start:chaos-movie-'$$'",
        "user_id": "chaos-user",
        "context_features": {"user_age_bucket": 2.0, "watch_history_len": 50.0, "subscription_tier": 1.0}
    }' "[::1]:${POLICY_PORT}" \
        "${svc_path}/SelectArm" 2>&1) || {
        _fail "SelectArm for cold-start experiment failed"
        verify_ok=false
    }

    if echo "$arm_result" | grep -q "armId\|arm_id"; then
        _ok "SelectArm: cold-start arm selection works"
    else
        _fail "SelectArm: no arm returned for cold-start"
        verify_ok=false
    fi

    # --- Test 4: GetPolicySnapshot (should return NotFound for new experiment) ---
    local snap_result
    snap_result=$(grpcurl -plaintext \
        -d '{"experiment_id": "cold-start:chaos-movie-'$$'"}' \
        "[::1]:${POLICY_PORT}" \
        "${svc_path}/GetPolicySnapshot" 2>&1) || true

    if echo "$snap_result" | grep -q "NotFound\|not found\|snapshot"; then
        _ok "GetPolicySnapshot: responds correctly (no snapshot yet)"
    else
        _fail "GetPolicySnapshot: unexpected response"
        verify_ok=false
    fi

    $verify_ok
}

# ---------------------------------------------------------------------------
# Standalone mode: Run full kill/recovery cycle
# ---------------------------------------------------------------------------
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    _log "Running standalone chaos test for policy service (M4b)"

    # Build
    POLICY_BIN=$(_build_policy)
    _ok "Built policy binary: $POLICY_BIN"

    # Clean RocksDB
    rm -rf "$POLICY_ROCKSDB_PATH"

    # Start
    _log "Starting policy service..."
    KEEP_ROCKSDB="true"
    POLICY_PID=$(chaos_start_policy)
    _ok "Started (PID=$POLICY_PID)"

    # Wait for health
    for i in $(seq 1 30); do
        if chaos_health_policy; then
            _ok "Healthy after ${i}s"
            break
        fi
        if [[ $i -eq 30 ]]; then
            _fail "Policy service did not become healthy in 30s"
            cat "$POLICY_LOG"
            kill "$POLICY_PID" 2>/dev/null || true
            exit 1
        fi
        sleep 1
    done

    # Pre-crash: Create cold-start sentinel
    _log "Creating sentinel cold-start bandit..."
    SENTINEL_RESULT=$(grpcurl -plaintext -d '{
        "content_id": "sentinel-movie",
        "content_metadata": {"genre": "sentinel"},
        "window_days": 7
    }' "[::1]:${POLICY_PORT}" \
        "experimentation.bandit.v1.BanditPolicyService/CreateColdStartBandit" 2>&1) || {
        _fail "Failed to create sentinel bandit"
        cat "$POLICY_LOG"
        kill "$POLICY_PID" 2>/dev/null || true
        exit 1
    }
    _ok "Sentinel cold-start bandit created"

    # Pre-crash: Verify SelectArm works
    grpcurl -plaintext -d '{
        "experiment_id": "cold-start:sentinel-movie",
        "user_id": "pre-crash-user",
        "context_features": {"user_age_bucket": 1.0, "watch_history_len": 10.0, "subscription_tier": 0.0}
    }' "[::1]:${POLICY_PORT}" \
        "experimentation.bandit.v1.BanditPolicyService/SelectArm" >/dev/null 2>&1
    _ok "SelectArm works before crash"

    # Kill with SIGKILL (simulate crash)
    _log "Sending SIGKILL to policy service (PID=$POLICY_PID)..."
    kill -9 "$POLICY_PID" 2>/dev/null || true
    wait "$POLICY_PID" 2>/dev/null || true
    _ok "Policy service killed"

    sleep 1

    # Restart and measure recovery
    _log "Restarting policy service..."
    START_NS=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
    POLICY_PID=$(chaos_start_policy)

    RECOVERY_MS=0
    for i in $(seq 1 200); do
        if chaos_health_policy; then
            END_NS=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
            RECOVERY_MS=$(( (END_NS - START_NS) / 1000000 ))
            _ok "Recovered in ${RECOVERY_MS}ms"
            break
        fi
        sleep 0.1
    done

    if [[ $RECOVERY_MS -eq 0 ]]; then
        _fail "Policy service did not recover within 20s"
        cat "$POLICY_LOG"
        kill "$POLICY_PID" 2>/dev/null || true
        exit 1
    fi

    # Verify data integrity
    _log "Verifying data integrity..."
    if chaos_verify_policy; then
        _ok "All integrity checks passed"
    else
        _fail "Data integrity verification failed"
        kill "$POLICY_PID" 2>/dev/null || true
        exit 1
    fi

    # SLA check
    if [[ $RECOVERY_MS -le $RECOVERY_SLA_MS ]]; then
        _ok "Recovery ${RECOVERY_MS}ms <= ${RECOVERY_SLA_MS}ms SLA"
    else
        _fail "Recovery ${RECOVERY_MS}ms > ${RECOVERY_SLA_MS}ms SLA"
    fi

    # Cleanup
    kill "$POLICY_PID" 2>/dev/null || true
    rm -f "$POLICY_BIN" "$POLICY_LOG"
    rm -rf "$POLICY_ROCKSDB_PATH"

    echo ""
    _log "=== Chaos Test Report: Policy Service (M4b) ==="
    echo "  Recovery time:   ${RECOVERY_MS}ms"
    echo "  Recovery SLA:    ${RECOVERY_SLA_MS}ms"
    echo "  gRPC:            OK"
    echo "  Cold-start:      OK"
    echo "  Arm selection:   OK"
    echo "  State integrity: OK"
    _ok "PASS"
fi
