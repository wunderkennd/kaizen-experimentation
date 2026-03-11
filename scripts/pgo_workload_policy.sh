#!/usr/bin/env bash
# =============================================================================
# PGO Workload: Realistic gRPC traffic for M4b Bandit Policy Service
# =============================================================================
# Exercises all BanditPolicyService hot paths to generate representative
# profiling data for PGO builds:
#   Phase 0: CreateColdStartBandit — seed 10 experiments (Thompson + LinUCB)
#   Phase 1: SelectArm (Thompson)  — 8,000 Beta-Bernoulli arm selections
#   Phase 2: SelectArm (contextual) — 5,000 LinUCB with feature vectors
#   Phase 3: ExportAffinityScores  — 200 LinUCB score exports
#   Phase 4: GetPolicySnapshot     — 100 RocksDB reads
#   Total:   ~13,300 calls
#
# Usage:
#   PGO_PORT=50098 ./scripts/pgo_workload_policy.sh
# =============================================================================

set -euo pipefail

PORT="${PGO_PORT:-${POLICY_PORT:-50054}}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROTO_DIR="$REPO_ROOT/proto"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
NC='\033[0m'

log()  { echo -e "${BLUE}[pgo-workload-policy]${NC} $*"; }
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

SVC="experimentation.bandit.v1.BanditPolicyService"

# ---------------------------------------------------------------------------
# Phase 0: CreateColdStartBandit — seed experiments (10 calls)
# ---------------------------------------------------------------------------
log "Phase 0/4: CreateColdStartBandit — seed 10 experiments..."

# 5 Thompson Sampling experiments (no context features)
for i in $(seq 1 5); do
    call_grpc "$SVC" "CreateColdStartBandit" \
        "{\"content_id\":\"pgo-thompson-$i\",\"content_metadata\":{\"genre\":\"action\",\"year\":\"2025\"},\"window_days\":14}"
done

# 5 LinUCB experiments (with context features)
for i in $(seq 1 5); do
    call_grpc "$SVC" "CreateColdStartBandit" \
        "{\"content_id\":\"pgo-linucb-$i\",\"content_metadata\":{\"genre\":\"drama\",\"year\":\"2025\",\"rating\":\"4.2\"},\"window_days\":7}"
done

ok "Seeded experiments: $TOTAL_SENT created"

# ---------------------------------------------------------------------------
# Phase 1: SelectArm — Thompson Sampling (8000 calls)
# ---------------------------------------------------------------------------
log "Phase 1/4: SelectArm — Thompson Sampling (8000 calls)..."
PHASE1_START=$TOTAL_SENT

for i in $(seq 1 8000); do
    exp_idx=$(( (i % 5) + 1 ))
    call_grpc "$SVC" "SelectArm" \
        "{\"experiment_id\":\"pgo-thompson-${exp_idx}\",\"user_id\":\"pgo-user-$i\"}"
done

PHASE1_TOTAL=$((TOTAL_SENT - PHASE1_START))
ok "Thompson SelectArm: $PHASE1_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 2: SelectArm — LinUCB with context features (5000 calls)
# ---------------------------------------------------------------------------
log "Phase 2/4: SelectArm — LinUCB contextual (5000 calls)..."
PHASE2_START=$TOTAL_SENT

for i in $(seq 1 5000); do
    exp_idx=$(( (i % 5) + 1 ))
    # Vary features to exercise different code paths
    f1=$(echo "scale=2; ($i % 100) / 100" | bc)
    f2=$(echo "scale=2; ($i % 50) / 50" | bc)
    f3=$(echo "scale=2; ($i % 25) / 25" | bc)
    call_grpc "$SVC" "SelectArm" \
        "{\"experiment_id\":\"pgo-linucb-${exp_idx}\",\"user_id\":\"pgo-user-ctx-$i\",\"context_features\":{\"genre_affinity\":${f1},\"recency_days\":${f2},\"tenure_months\":${f3}}}"
done

PHASE2_TOTAL=$((TOTAL_SENT - PHASE2_START))
ok "LinUCB SelectArm: $PHASE2_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 3: ExportAffinityScores (200 calls)
# ---------------------------------------------------------------------------
log "Phase 3/4: ExportAffinityScores (200 calls)..."
PHASE3_START=$TOTAL_SENT

for i in $(seq 1 200); do
    exp_idx=$(( (i % 5) + 1 ))
    call_grpc "$SVC" "ExportAffinityScores" \
        "{\"experiment_id\":\"pgo-linucb-${exp_idx}\"}"
done

PHASE3_TOTAL=$((TOTAL_SENT - PHASE3_START))
ok "ExportAffinityScores: $PHASE3_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 4: GetPolicySnapshot (100 calls)
# ---------------------------------------------------------------------------
log "Phase 4/4: GetPolicySnapshot (100 calls)..."
PHASE4_START=$TOTAL_SENT

for i in $(seq 1 100); do
    exp_idx=$(( (i % 10) + 1 ))
    if [[ $exp_idx -le 5 ]]; then
        call_grpc "$SVC" "GetPolicySnapshot" \
            "{\"experiment_id\":\"pgo-thompson-${exp_idx}\"}"
    else
        idx=$((exp_idx - 5))
        call_grpc "$SVC" "GetPolicySnapshot" \
            "{\"experiment_id\":\"pgo-linucb-${idx}\"}"
    fi
done

PHASE4_TOTAL=$((TOTAL_SENT - PHASE4_START))
ok "GetPolicySnapshot: $PHASE4_TOTAL sent"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  PGO WORKLOAD COMPLETE: M4b Bandit Policy Service"
echo "============================================================="
echo "  Total calls sent:    $TOTAL_SENT"
echo "  Total errors:        $TOTAL_ERRORS"
echo "  Target port:         $PORT"
echo ""
echo "  Breakdown:"
echo "    CreateColdStartBandit:  10"
echo "    SelectArm (Thompson):   $PHASE1_TOTAL"
echo "    SelectArm (LinUCB):     $PHASE2_TOTAL"
echo "    ExportAffinityScores:   $PHASE3_TOTAL"
echo "    GetPolicySnapshot:      $PHASE4_TOTAL"
echo "============================================================="
