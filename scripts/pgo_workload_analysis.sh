#!/usr/bin/env bash
# =============================================================================
# PGO Workload: Realistic gRPC traffic for M4a Analysis Service
# =============================================================================
# Exercises all AnalysisService hot paths to generate representative
# profiling data for PGO builds:
#   Phase 1: RunAnalysis              — 500 calls
#   Phase 2: GetInterleavingAnalysis  — 200 calls
#   Phase 3: GetNoveltyAnalysis       — 200 calls
#   Phase 4: GetInterferenceAnalysis  — 100 calls
#   Total:   1,000 calls
#
# Usage:
#   PGO_PORT=50097 ./scripts/pgo_workload_analysis.sh
# =============================================================================

set -euo pipefail

PORT="${PGO_PORT:-${ANALYSIS_PORT:-50055}}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROTO_DIR="$REPO_ROOT/proto"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
NC='\033[0m'

log()  { echo -e "${BLUE}[pgo-workload-analysis]${NC} $*"; }
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

SVC="experimentation.analysis.v1.AnalysisService"

# ---------------------------------------------------------------------------
# Phase 1: RunAnalysis (500 calls)
# ---------------------------------------------------------------------------
log "Phase 1/4: RunAnalysis (500 calls)..."

for i in $(seq 1 500); do
    exp_idx=$(( (i % 5) + 1 ))
    call_grpc "$SVC" "RunAnalysis" \
        "{\"experiment_id\":\"pgo-exp-${exp_idx}\"}"
done

ok "RunAnalysis: $TOTAL_SENT sent"

# ---------------------------------------------------------------------------
# Phase 2: GetInterleavingAnalysis (200 calls)
# ---------------------------------------------------------------------------
log "Phase 2/4: GetInterleavingAnalysis (200 calls)..."
PHASE2_START=$TOTAL_SENT

for i in $(seq 1 200); do
    exp_idx=$(( (i % 2) + 1 ))
    call_grpc "$SVC" "GetInterleavingAnalysis" \
        "{\"experiment_id\":\"pgo-interleave-${exp_idx}\"}"
done

PHASE2_TOTAL=$((TOTAL_SENT - PHASE2_START))
ok "GetInterleavingAnalysis: $PHASE2_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 3: GetNoveltyAnalysis (200 calls)
# ---------------------------------------------------------------------------
log "Phase 3/4: GetNoveltyAnalysis (200 calls)..."
PHASE3_START=$TOTAL_SENT

for i in $(seq 1 200); do
    exp_idx=$(( (i % 5) + 1 ))
    call_grpc "$SVC" "GetNoveltyAnalysis" \
        "{\"experiment_id\":\"pgo-exp-${exp_idx}\"}"
done

PHASE3_TOTAL=$((TOTAL_SENT - PHASE3_START))
ok "GetNoveltyAnalysis: $PHASE3_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 4: GetInterferenceAnalysis (100 calls)
# ---------------------------------------------------------------------------
log "Phase 4/4: GetInterferenceAnalysis (100 calls)..."
PHASE4_START=$TOTAL_SENT

for i in $(seq 1 100); do
    exp_idx=$(( (i % 2) + 1 ))
    call_grpc "$SVC" "GetInterferenceAnalysis" \
        "{\"experiment_id\":\"pgo-interference-${exp_idx}\"}"
done

PHASE4_TOTAL=$((TOTAL_SENT - PHASE4_START))
ok "GetInterferenceAnalysis: $PHASE4_TOTAL sent"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  PGO WORKLOAD COMPLETE: M4a Analysis Service"
echo "============================================================="
echo "  Total calls sent:    $TOTAL_SENT"
echo "  Total errors:        $TOTAL_ERRORS"
echo "  Target port:         $PORT"
echo ""
echo "  Breakdown:"
echo "    RunAnalysis:              500"
echo "    GetInterleavingAnalysis:  $PHASE2_TOTAL"
echo "    GetNoveltyAnalysis:       $PHASE3_TOTAL"
echo "    GetInterferenceAnalysis:  $PHASE4_TOTAL"
echo "============================================================="
