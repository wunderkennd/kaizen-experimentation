#!/usr/bin/env bash
# =============================================================================
# PGO Workload: Realistic gRPC traffic for M2 Event Pipeline profiling
# =============================================================================
# Exercises all M2 Event Pipeline hot paths to generate representative
# profiling data for PGO builds:
#   - 4,000 IngestExposure calls (primary hot path)
#   - 3,000 IngestMetricEvent calls (second most common)
#   - 2,000 IngestRewardEvent calls (bandit reward pipeline)
#   - 1,500 IngestQoEEvent calls (QoE validation)
#   - 500 IngestExposureBatch calls (batch code path)
#   - 500 IngestQoEEventBatch calls (batch QoE validation)
#
# Total: ~11,500 gRPC calls.
# Target runtime: ~60s depending on system speed.
#
# Usage:
#   PGO_PORT=50096 ./scripts/pgo_workload_pipeline.sh
#   PIPELINE_PORT=50052 ./scripts/pgo_workload_pipeline.sh
# =============================================================================

set -euo pipefail

PORT="${PGO_PORT:-${PIPELINE_PORT:-50052}}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROTO_DIR="$REPO_ROOT/proto"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
NC='\033[0m'

log()  { echo -e "${BLUE}[pgo-workload-pipeline]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }

ADDR="localhost:${PORT}"
SERVICE="experimentation.pipeline.v1.EventIngestionService"
TOTAL_SENT=0
TOTAL_ERRORS=0

NOW_SECS=$(date +%s)

# Experiment pool (rotate across 5 experiments)
EXPERIMENTS=("exp_dev_001" "exp_dev_002" "exp_dev_003" "exp_dev_005" "exp_dev_006")

# ---------------------------------------------------------------------------
# Helper: fire gRPC call, count success/failure
# ---------------------------------------------------------------------------
call_grpc() {
    local method="$1"
    local payload="$2"

    if grpcurl -plaintext -import-path "$PROTO_DIR" \
        -proto experimentation/pipeline/v1/pipeline_service.proto \
        -d "$payload" "$ADDR" "${SERVICE}/${method}" >/dev/null 2>&1; then
        TOTAL_SENT=$((TOTAL_SENT + 1))
    else
        TOTAL_ERRORS=$((TOTAL_ERRORS + 1))
    fi
}

# ---------------------------------------------------------------------------
# Phase 1: IngestExposure (4000 calls)
# ---------------------------------------------------------------------------
log "Phase 1/6: IngestExposure (4000 calls)..."

for i in $(seq 1 4000); do
    EXP_IDX=$((i % 5))
    call_grpc "IngestExposure" \
        "{\"event\":{\"event_id\":\"pgo-exp-$i-$$\",\"experiment_id\":\"${EXPERIMENTS[$EXP_IDX]}\",\"user_id\":\"pgo-user-$((i % 10000))\",\"variant_id\":\"control\",\"timestamp\":{\"seconds\":$NOW_SECS},\"platform\":\"web\"}}"
done
ok "IngestExposure: $TOTAL_SENT sent"

# ---------------------------------------------------------------------------
# Phase 2: IngestMetricEvent (3000 calls)
# ---------------------------------------------------------------------------
log "Phase 2/6: IngestMetricEvent (3000 calls)..."
PHASE2_START=$TOTAL_SENT

for i in $(seq 1 3000); do
    call_grpc "IngestMetricEvent" \
        "{\"event\":{\"event_id\":\"pgo-met-$i-$$\",\"user_id\":\"pgo-user-$((i % 10000))\",\"event_type\":\"play_start\",\"value\":$((RANDOM % 3600)),\"content_id\":\"content-$((i % 500))\",\"timestamp\":{\"seconds\":$NOW_SECS}}}"
done
PHASE2_TOTAL=$((TOTAL_SENT - PHASE2_START))
ok "IngestMetricEvent: $PHASE2_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 3: IngestRewardEvent (2000 calls)
# ---------------------------------------------------------------------------
log "Phase 3/6: IngestRewardEvent (2000 calls)..."
PHASE3_START=$TOTAL_SENT

for i in $(seq 1 2000); do
    EXP_IDX=$((i % 5))
    call_grpc "IngestRewardEvent" \
        "{\"event\":{\"event_id\":\"pgo-rew-$i-$$\",\"experiment_id\":\"${EXPERIMENTS[$EXP_IDX]}\",\"user_id\":\"pgo-user-$((i % 10000))\",\"arm_id\":\"arm-$((i % 4))\",\"reward\":0.$((RANDOM % 100)),\"timestamp\":{\"seconds\":$NOW_SECS}}}"
done
PHASE3_TOTAL=$((TOTAL_SENT - PHASE3_START))
ok "IngestRewardEvent: $PHASE3_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 4: IngestQoEEvent (1500 calls)
# ---------------------------------------------------------------------------
log "Phase 4/6: IngestQoEEvent (1500 calls)..."
PHASE4_START=$TOTAL_SENT

for i in $(seq 1 1500); do
    call_grpc "IngestQoEEvent" \
        "{\"event\":{\"event_id\":\"pgo-qoe-$i-$$\",\"session_id\":\"session-$((i % 5000))\",\"content_id\":\"content-$((i % 500))\",\"user_id\":\"pgo-user-$((i % 10000))\",\"metrics\":{\"time_to_first_frame_ms\":$((RANDOM % 5000)),\"rebuffer_count\":$((RANDOM % 10)),\"rebuffer_ratio\":0.0$((RANDOM % 50)),\"avg_bitrate_kbps\":$((2000 + RANDOM % 8000)),\"peak_resolution_height\":1080},\"timestamp\":{\"seconds\":$NOW_SECS}}}"
done
PHASE4_TOTAL=$((TOTAL_SENT - PHASE4_START))
ok "IngestQoEEvent: $PHASE4_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 5: IngestExposureBatch (500 calls, ~50 events each)
# ---------------------------------------------------------------------------
log "Phase 5/6: IngestExposureBatch (500 calls × 50 events)..."
PHASE5_START=$TOTAL_SENT

for i in $(seq 1 500); do
    # Build batch of 50 events
    EVENTS=""
    for j in $(seq 1 50); do
        EXP_IDX=$(( (i * 50 + j) % 5 ))
        EVENT="{\"event_id\":\"pgo-batch-$i-$j-$$\",\"experiment_id\":\"${EXPERIMENTS[$EXP_IDX]}\",\"user_id\":\"pgo-user-$(( (i * 50 + j) % 10000 ))\",\"variant_id\":\"treatment\",\"timestamp\":{\"seconds\":$NOW_SECS},\"platform\":\"ios\"}"
        if [[ -n "$EVENTS" ]]; then
            EVENTS="$EVENTS,$EVENT"
        else
            EVENTS="$EVENT"
        fi
    done
    call_grpc "IngestExposureBatch" "{\"events\":[$EVENTS]}"
done
PHASE5_TOTAL=$((TOTAL_SENT - PHASE5_START))
ok "IngestExposureBatch: $PHASE5_TOTAL sent"

# ---------------------------------------------------------------------------
# Phase 6: IngestQoEEventBatch (500 calls, ~10 events each)
# ---------------------------------------------------------------------------
log "Phase 6/6: IngestQoEEventBatch (500 calls × 10 events)..."
PHASE6_START=$TOTAL_SENT

for i in $(seq 1 500); do
    EVENTS=""
    for j in $(seq 1 10); do
        EVENT="{\"event_id\":\"pgo-qbatch-$i-$j-$$\",\"session_id\":\"session-$(( (i * 10 + j) % 5000 ))\",\"content_id\":\"content-$((j % 100))\",\"user_id\":\"pgo-user-$(( (i * 10 + j) % 10000 ))\",\"metrics\":{\"time_to_first_frame_ms\":$((RANDOM % 5000)),\"rebuffer_ratio\":0.0$((RANDOM % 50)),\"avg_bitrate_kbps\":$((2000 + RANDOM % 8000)),\"peak_resolution_height\":1080},\"timestamp\":{\"seconds\":$NOW_SECS}}"
        if [[ -n "$EVENTS" ]]; then
            EVENTS="$EVENTS,$EVENT"
        else
            EVENTS="$EVENT"
        fi
    done
    call_grpc "IngestQoEEventBatch" "{\"events\":[$EVENTS]}"
done
PHASE6_TOTAL=$((TOTAL_SENT - PHASE6_START))
ok "IngestQoEEventBatch: $PHASE6_TOTAL sent"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  PGO WORKLOAD COMPLETE: M2 Event Pipeline"
echo "============================================================="
echo "  Total calls sent:    $TOTAL_SENT"
echo "  Total errors:        $TOTAL_ERRORS"
echo "  Target port:         $PORT"
echo "============================================================="
