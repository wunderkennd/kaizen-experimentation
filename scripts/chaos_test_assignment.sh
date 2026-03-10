#!/usr/bin/env bash
# =============================================================================
# Pluggable Chaos Hook: M1 Assignment Service
# =============================================================================
# Sourced by scripts/chaos_e2e_framework.sh (Milestone 4.5).
#
# Exports three functions matching the framework contract:
#   chaos_start_assignment   — Start the service, echo PID
#   chaos_health_assignment  — Exit 0 if healthy
#   chaos_verify_assignment  — Exit 0 if data integrity OK after recovery
#
# The assignment service is fully stateless. Crash-only design means:
#   - No warm-up or recovery needed
#   - Config loaded from JSON file on startup
#   - Deterministic hash assignments available immediately
# =============================================================================

ASSIGNMENT_PORT=${ASSIGNMENT_PORT:-50051}
ASSIGNMENT_BIN="${REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}/target/release/experimentation-assignment"
ASSIGNMENT_CONFIG="${REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}/dev/config.json"

chaos_start_assignment() {
    CONFIG_PATH="$ASSIGNMENT_CONFIG" \
    GRPC_ADDR="0.0.0.0:${ASSIGNMENT_PORT}" \
    RUST_LOG=warn \
    "$ASSIGNMENT_BIN" > "${WORK_DIR:-/tmp}/logs/assignment.log" 2>&1 &
    echo $!
}

chaos_health_assignment() {
    grpcurl -plaintext "localhost:${ASSIGNMENT_PORT}" list >/dev/null 2>&1
}

chaos_verify_assignment() {
    # Verify GetAssignment returns a valid variant for a known experiment
    # from dev/config.json (exp_dev_001 is an AB test with 100% allocation).
    local result
    result=$(grpcurl -plaintext -d '{
        "user_id": "chaos-verify-user",
        "experiment_id": "exp_dev_001"
    }' "localhost:${ASSIGNMENT_PORT}" \
        experimentation.assignment.v1.AssignmentService/GetAssignment 2>&1) || return 1

    echo "$result" | grep -q '"variantId"'
}
