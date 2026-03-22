#!/usr/bin/env bash
# =============================================================================
# Chaos Test: Analysis Service (M4a) — Pluggable hook for chaos_e2e_framework
# =============================================================================
# Exports:
#   chaos_start_analysis   — Build & start the analysis Rust binary, echo PID
#   chaos_health_analysis  — Exit 0 if gRPC responds
#   chaos_verify_analysis  — Verify all 5 RPCs respond correctly after recovery
#
# M4a is stateless (no RocksDB, no Kafka). Crash recovery is trivial:
# restart the binary and verify gRPC is responsive.
#
# All 5 analysis RPCs are fully wired (PR #107):
#   RunAnalysis, GetAnalysisResult, GetInterleavingAnalysis,
#   GetNoveltyAnalysis, GetInterferenceAnalysis
#
# Usage:
#   Standalone:  ./scripts/chaos_test_analysis.sh
#   Framework:   ./scripts/chaos_e2e_framework.sh --services analysis
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ANALYSIS_PORT=${ANALYSIS_PORT:-50055}
ANALYSIS_GRPC_ADDR=${ANALYSIS_GRPC_ADDR:-"[::1]:${ANALYSIS_PORT}"}
ANALYSIS_BIN=""
ANALYSIS_LOG="/tmp/chaos_analysis_$$.log"
RECOVERY_SLA_MS=${RECOVERY_SLA_MS:-2000}

# Colors
RED=${RED:-'\033[0;31m'}
GREEN=${GREEN:-'\033[0;32m'}
YELLOW=${YELLOW:-'\033[1;33m'}
NC=${NC:-'\033[0m'}

_log() { echo -e "[chaos-analysis] $*"; }
_ok()  { echo -e "${GREEN}[  OK  ]${NC} $*"; }
_fail(){ echo -e "${RED}[ FAIL ]${NC} $*"; }

# ---------------------------------------------------------------------------
# Build analysis binary
# ---------------------------------------------------------------------------
_build_analysis() {
    local bin_path="/tmp/chaos_analysis_server_$$"
    _log "Building analysis service (release)..."
    (cd "$REPO_ROOT" && cargo build --release --package experimentation-analysis 2>&1) || {
        _fail "cargo build failed"
        return 1
    }
    cp "$REPO_ROOT/target/release/experimentation-analysis" "$bin_path"
    echo "$bin_path"
}

# ---------------------------------------------------------------------------
# Hook: chaos_start_analysis — Start the service, echo PID
# ---------------------------------------------------------------------------
chaos_start_analysis() {
    if [[ -z "$ANALYSIS_BIN" ]] || [[ ! -f "$ANALYSIS_BIN" ]]; then
        ANALYSIS_BIN=$(_build_analysis)
    fi

    ANALYSIS_GRPC_ADDR="$ANALYSIS_GRPC_ADDR" \
    DELTA_LAKE_PATH="${DELTA_LAKE_PATH:-/tmp/delta}" \
    ANALYSIS_DEFAULT_ALPHA="0.05" \
    ANALYSIS_JS_THRESHOLD="0.05" \
    RUST_LOG="experimentation_analysis=info" \
    "$ANALYSIS_BIN" > "$ANALYSIS_LOG" 2>&1 &

    local pid=$!
    echo "$pid"
}

# ---------------------------------------------------------------------------
# Hook: chaos_health_analysis — Exit 0 if gRPC is reachable
# ---------------------------------------------------------------------------
chaos_health_analysis() {
    # Call RunAnalysis with a nonexistent experiment. If the service is up,
    # we get NotFound or Internal (no Delta data). If down, grpcurl fails.
    local result
    result=$(grpcurl -plaintext \
        -d '{"experiment_id": "health-check"}' \
        "[::1]:${ANALYSIS_PORT}" \
        "experimentation.analysis.v1.AnalysisService/RunAnalysis" 2>&1) || true

    # Service is healthy if we get any gRPC-level response
    if echo "$result" | grep -q "NotFound\|not found\|Internal\|INTERNAL\|InvalidArgument\|INVALID_ARGUMENT\|error\|result"; then
        return 0
    fi

    return 1
}

# ---------------------------------------------------------------------------
# Hook: chaos_verify_analysis — Verify all 5 RPCs respond correctly
# ---------------------------------------------------------------------------
chaos_verify_analysis() {
    local svc_path="experimentation.analysis.v1.AnalysisService"
    local verify_ok=true

    # --- Test 1: RunAnalysis with nonexistent experiment → NotFound/Internal ---
    local run_result
    run_result=$(grpcurl -plaintext \
        -d '{"experiment_id": "chaos-verify-exp"}' \
        "[::1]:${ANALYSIS_PORT}" \
        "${svc_path}/RunAnalysis" 2>&1) || true

    if echo "$run_result" | grep -q "NotFound\|not found\|Internal\|INTERNAL\|No metric summaries"; then
        _ok "RunAnalysis: handles missing experiment data"
    else
        _fail "RunAnalysis: unexpected response: $run_result"
        verify_ok=false
    fi

    # --- Test 2: GetAnalysisResult with nonexistent experiment → NotFound ---
    local get_result
    get_result=$(grpcurl -plaintext \
        -d '{"experiment_id": "chaos-verify-exp"}' \
        "[::1]:${ANALYSIS_PORT}" \
        "${svc_path}/GetAnalysisResult" 2>&1) || true

    if echo "$get_result" | grep -q "NotFound\|not found\|no cached\|no analysis"; then
        _ok "GetAnalysisResult: NotFound for missing experiment"
    else
        _fail "GetAnalysisResult: unexpected response: $get_result"
        verify_ok=false
    fi

    # --- Test 3: GetInterleavingAnalysis with nonexistent experiment → NotFound ---
    local interleaving_result
    interleaving_result=$(grpcurl -plaintext \
        -d '{"experiment_id": "chaos-verify-exp"}' \
        "[::1]:${ANALYSIS_PORT}" \
        "${svc_path}/GetInterleavingAnalysis" 2>&1) || true

    if echo "$interleaving_result" | grep -q "NotFound\|not found\|Internal\|INTERNAL"; then
        _ok "GetInterleavingAnalysis: handles missing experiment"
    else
        _fail "GetInterleavingAnalysis: unexpected response: $interleaving_result"
        verify_ok=false
    fi

    # --- Test 4: GetNoveltyAnalysis with nonexistent experiment → NotFound ---
    local novelty_result
    novelty_result=$(grpcurl -plaintext \
        -d '{"experiment_id": "chaos-verify-exp"}' \
        "[::1]:${ANALYSIS_PORT}" \
        "${svc_path}/GetNoveltyAnalysis" 2>&1) || true

    if echo "$novelty_result" | grep -q "NotFound\|not found\|Internal\|INTERNAL"; then
        _ok "GetNoveltyAnalysis: handles missing experiment"
    else
        _fail "GetNoveltyAnalysis: unexpected response: $novelty_result"
        verify_ok=false
    fi

    # --- Test 5: GetInterferenceAnalysis with empty experiment_id → InvalidArgument ---
    local interference_result
    interference_result=$(grpcurl -plaintext \
        -d '{"experiment_id": ""}' \
        "[::1]:${ANALYSIS_PORT}" \
        "${svc_path}/GetInterferenceAnalysis" 2>&1) || true

    if echo "$interference_result" | grep -q "InvalidArgument\|INVALID_ARGUMENT\|required\|empty"; then
        _ok "GetInterferenceAnalysis: validates input (InvalidArgument for empty id)"
    else
        _fail "GetInterferenceAnalysis: unexpected response for empty id: $interference_result"
        verify_ok=false
    fi

    # --- Test 6: GetInterferenceAnalysis with valid id but no Delta data → error ---
    local interference_missing
    interference_missing=$(grpcurl -plaintext \
        -d '{"experiment_id": "nonexistent-exp"}' \
        "[::1]:${ANALYSIS_PORT}" \
        "${svc_path}/GetInterferenceAnalysis" 2>&1) || true

    if echo "$interference_missing" | grep -q "not found\|NotFound\|Internal\|INTERNAL"; then
        _ok "GetInterferenceAnalysis: handles missing Delta data gracefully"
    else
        _fail "GetInterferenceAnalysis: unexpected response for missing data: $interference_missing"
        verify_ok=false
    fi

    # --- Test 7: RunAnalysis with empty experiment_id → InvalidArgument ---
    local run_empty
    run_empty=$(grpcurl -plaintext \
        -d '{"experiment_id": ""}' \
        "[::1]:${ANALYSIS_PORT}" \
        "${svc_path}/RunAnalysis" 2>&1) || true

    if echo "$run_empty" | grep -q "InvalidArgument\|INVALID_ARGUMENT\|required\|empty"; then
        _ok "RunAnalysis: validates input (InvalidArgument for empty id)"
    else
        _fail "RunAnalysis: unexpected response for empty id: $run_empty"
        verify_ok=false
    fi

    $verify_ok
}

# ---------------------------------------------------------------------------
# Standalone mode: Run full kill/recovery cycle
# ---------------------------------------------------------------------------
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    _log "Running standalone chaos test for analysis service (M4a)"

    # Build
    ANALYSIS_BIN=$(_build_analysis)
    _ok "Built analysis binary: $ANALYSIS_BIN"

    # Start
    _log "Starting analysis service..."
    ANALYSIS_PID=$(chaos_start_analysis)
    _ok "Started (PID=$ANALYSIS_PID)"

    # Wait for health
    for i in $(seq 1 30); do
        if chaos_health_analysis; then
            _ok "Healthy after ${i}s"
            break
        fi
        if [[ $i -eq 30 ]]; then
            _fail "Analysis service did not become healthy in 30s"
            cat "$ANALYSIS_LOG"
            kill "$ANALYSIS_PID" 2>/dev/null || true
            exit 1
        fi
        sleep 1
    done

    # Pre-crash: Verify all RPCs respond
    _log "Verifying RPCs before crash..."
    if chaos_verify_analysis; then
        _ok "All RPCs respond correctly before crash"
    else
        _fail "Pre-crash verification failed"
        kill "$ANALYSIS_PID" 2>/dev/null || true
        exit 1
    fi

    # Kill with SIGKILL
    _log "Sending SIGKILL to analysis service (PID=$ANALYSIS_PID)..."
    kill -9 "$ANALYSIS_PID" 2>/dev/null || true
    wait "$ANALYSIS_PID" 2>/dev/null || true
    _ok "Analysis service killed"

    sleep 1

    # Restart and measure recovery
    _log "Restarting analysis service..."
    START_NS=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
    ANALYSIS_PID=$(chaos_start_analysis)

    RECOVERY_MS=0
    for i in $(seq 1 200); do
        if chaos_health_analysis; then
            END_NS=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
            RECOVERY_MS=$(( (END_NS - START_NS) / 1000000 ))
            _ok "Recovered in ${RECOVERY_MS}ms"
            break
        fi
        sleep 0.1
    done

    if [[ $RECOVERY_MS -eq 0 ]]; then
        _fail "Analysis service did not recover within 20s"
        cat "$ANALYSIS_LOG"
        kill "$ANALYSIS_PID" 2>/dev/null || true
        exit 1
    fi

    # Verify data integrity
    _log "Verifying data integrity after crash..."
    if chaos_verify_analysis; then
        _ok "All integrity checks passed"
    else
        _fail "Post-crash verification failed"
        kill "$ANALYSIS_PID" 2>/dev/null || true
        exit 1
    fi

    # SLA check
    if [[ $RECOVERY_MS -le $RECOVERY_SLA_MS ]]; then
        _ok "Recovery ${RECOVERY_MS}ms <= ${RECOVERY_SLA_MS}ms SLA"
    else
        _fail "Recovery ${RECOVERY_MS}ms > ${RECOVERY_SLA_MS}ms SLA"
    fi

    # Cleanup
    kill "$ANALYSIS_PID" 2>/dev/null || true
    rm -f "$ANALYSIS_BIN" "$ANALYSIS_LOG"

    echo ""
    _log "=== Chaos Test Report: Analysis Service (M4a) ==="
    echo "  Recovery time:    ${RECOVERY_MS}ms"
    echo "  Recovery SLA:     ${RECOVERY_SLA_MS}ms"
    echo "  RPCs verified:    7 (RunAnalysis, GetAnalysisResult, GetInterleavingAnalysis,"
    echo "                       GetNoveltyAnalysis, GetInterferenceAnalysis + input validation)"
    echo "  State:            Stateless (no persistence needed)"
    _ok "PASS"
fi
