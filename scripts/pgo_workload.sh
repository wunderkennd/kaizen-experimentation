#!/usr/bin/env bash
# =============================================================================
# PGO Workload: Realistic gRPC traffic for profile-guided optimization
# =============================================================================
# Exercises all M1 Assignment Service hot paths to generate representative
# profiling data for PGO builds:
#   - 10,000 GetAssignment calls (AB, SESSION_LEVEL, MAB, CONTEXTUAL_BANDIT)
#   - 1,000 GetInterleavedList calls (Team Draft, Optimized, Multileave)
#   - 500 GetAssignments (batch) calls
#
# Uses dev/config.json experiments (9 total across all types).
# Target runtime: ~30s depending on system speed.
#
# Usage:
#   PGO_PORT=50099 ./scripts/pgo_workload.sh
#   ASSIGNMENT_PORT=50051 ./scripts/pgo_workload.sh
# =============================================================================

set -euo pipefail

PORT="${PGO_PORT:-${ASSIGNMENT_PORT:-50051}}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROTO_DIR="$REPO_ROOT/proto"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
NC='\033[0m'

log()  { echo -e "${BLUE}[pgo-workload]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }

ADDR="localhost:${PORT}"
TOTAL_SENT=0
TOTAL_ERRORS=0

# ---------------------------------------------------------------------------
# Helper: fire gRPC call, count success/failure
# ---------------------------------------------------------------------------
call_grpc() {
    local service="$1"
    local method="$2"
    local payload="$3"

    if grpcurl -plaintext -d "$payload" "$ADDR" "${service}/${method}" >/dev/null 2>&1; then
        TOTAL_SENT=$((TOTAL_SENT + 1))
    else
        TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
    fi
}

# ---------------------------------------------------------------------------
# Phase 1: GetAssignment — AB experiments (4000 calls)
# ---------------------------------------------------------------------------
log "Phase 1/4: GetAssignment — AB experiments (4000 calls)..."

for i in $(seq 1 2000); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetAssignment" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_001\"}"
done
for i in $(seq 1 2000); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetAssignment" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_002\",\"user_attributes\":{\"country\":\"US\",\"tier\":\"premium\"}}"
done
ok "AB assignments: $TOTAL_SENT sent"

# ---------------------------------------------------------------------------
# Phase 2: GetAssignment — SESSION_LEVEL + bandit experiments (6000 calls)
# ---------------------------------------------------------------------------
log "Phase 2/4: GetAssignment — SESSION + MAB + CONTEXTUAL (6000 calls)..."
PHASE2_START=$TOTAL_SENT

# Session-level: both locked and unlocked (2000 calls)
for i in $(seq 1 1000); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetAssignment" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_003\",\"session_id\":\"session-$i\"}"
done
for i in $(seq 1 1000); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetAssignment" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_008\",\"session_id\":\"session-$i\"}"
done

# MAB (2000 calls)
for i in $(seq 1 2000); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetAssignment" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_005\"}"
done

# Contextual bandit / cold-start (2000 calls)
for i in $(seq 1 2000); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetAssignment" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"cold-start:movie-new-001\",\"user_attributes\":{\"genre_affinity\":\"action\",\"recency_days\":\"3\",\"tenure_months\":\"12\"}}"
done

PHASE2_TOTAL=$((TOTAL_SENT - PHASE2_START))
ok "Session + bandit assignments: $PHASE2_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 3: GetInterleavedList (1000 calls)
# ---------------------------------------------------------------------------
log "Phase 3/4: GetInterleavedList — Team Draft + Optimized + Multileave (1000 calls)..."
PHASE3_START=$TOTAL_SENT

# Team Draft (400 calls)
for i in $(seq 1 400); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetInterleavedList" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_004\",\"ranked_lists\":{\"algo_a\":{\"item_ids\":[\"a1\",\"a2\",\"a3\",\"a4\",\"a5\",\"a6\",\"a7\",\"a8\",\"a9\",\"a10\"]},\"algo_b\":{\"item_ids\":[\"b1\",\"b2\",\"b3\",\"b4\",\"b5\",\"b6\",\"b7\",\"b8\",\"b9\",\"b10\"]}}}"
done

# Optimized (300 calls)
for i in $(seq 1 300); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetInterleavedList" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_006\",\"ranked_lists\":{\"algo_a\":{\"item_ids\":[\"a1\",\"a2\",\"a3\",\"a4\",\"a5\",\"a6\",\"a7\",\"a8\",\"a9\",\"a10\"]},\"algo_b\":{\"item_ids\":[\"b1\",\"b2\",\"b3\",\"b4\",\"b5\",\"b6\",\"b7\",\"b8\",\"b9\",\"b10\"]}}}"
done

# Multileave 3-way (300 calls)
for i in $(seq 1 300); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetInterleavedList" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_id\":\"exp_dev_007\",\"ranked_lists\":{\"algo_x\":{\"item_ids\":[\"x1\",\"x2\",\"x3\",\"x4\",\"x5\"]},\"algo_y\":{\"item_ids\":[\"y1\",\"y2\",\"y3\",\"y4\",\"y5\"]},\"algo_z\":{\"item_ids\":[\"z1\",\"z2\",\"z3\",\"z4\",\"z5\"]}}}"
done

PHASE3_TOTAL=$((TOTAL_SENT - PHASE3_START))
ok "Interleaving calls: $PHASE3_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 4: GetAssignments batch (500 calls)
# ---------------------------------------------------------------------------
log "Phase 4/4: GetAssignments — batch calls (500 calls)..."
PHASE4_START=$TOTAL_SENT

for i in $(seq 1 500); do
    call_grpc "experimentation.assignment.v1.AssignmentService" "GetAssignments" \
        "{\"user_id\":\"pgo-user-$i\",\"experiment_ids\":[\"exp_dev_001\",\"exp_dev_002\",\"exp_dev_003\"]}"
done

PHASE4_TOTAL=$((TOTAL_SENT - PHASE4_START))
ok "Batch assignments: $PHASE4_TOTAL sent"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  PGO WORKLOAD COMPLETE"
echo "============================================================="
echo "  Total calls sent:    $TOTAL_SENT"
echo "  Total errors:        $TOTAL_ERRORS"
echo "  Target port:         $PORT"
echo "============================================================="
