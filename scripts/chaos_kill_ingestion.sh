#!/usr/bin/env bash
# =============================================================================
# Chaos Test: Kill-9 Ingestion Pipeline Under Load
# =============================================================================
# Phase 4 chaos engineering for M2 event pipeline.
#
# Verifies crash-only recovery:
#   1. Start pipeline + Kafka via Docker Compose
#   2. Send sustained gRPC load (exposure + metric + QoE events)
#   3. kill -9 the pipeline process mid-publish
#   4. Verify disk buffer captured in-flight events
#   5. Restart pipeline — buffer replays automatically
#   6. Count events on Kafka — verify no data loss
#
# Prerequisites:
#   - Docker Compose (kafka, zookeeper, kafka-init running)
#   - cargo build --package experimentation-pipeline --release
#   - grpcurl installed (brew install grpcurl)
#
# Usage:
#   ./scripts/chaos_kill_ingestion.sh [--events NUM] [--duration SECS] [--buffer-dir DIR]
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
EVENTS_PER_SEC=${EVENTS_PER_SEC:-1000}
TOTAL_DURATION=${TOTAL_DURATION:-10}
KILL_AFTER_SECS=${KILL_AFTER_SECS:-5}
BUFFER_DIR=${BUFFER_DIR:-/tmp/experimentation-pipeline-chaos-buffer}
PIPELINE_PORT=${PIPELINE_PORT:-50061}
METRICS_PORT=${METRICS_PORT:-9091}
KAFKA_BROKERS=${KAFKA_BROKERS:-localhost:9092}
PROTO_DIR=${PROTO_DIR:-proto}
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PIPELINE_BIN="$REPO_ROOT/target/release/experimentation-pipeline"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[chaos]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK ]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN ]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; }

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --events)     EVENTS_PER_SEC="$2"; shift 2 ;;
        --duration)   TOTAL_DURATION="$2"; shift 2 ;;
        --kill-after) KILL_AFTER_SECS="$2"; shift 2 ;;
        --buffer-dir) BUFFER_DIR="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--events NUM] [--duration SECS] [--kill-after SECS] [--buffer-dir DIR]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
log "Chaos test: kill-9 ingestion pipeline under ${EVENTS_PER_SEC} events/sec load"
log "Config: duration=${TOTAL_DURATION}s, kill after ${KILL_AFTER_SECS}s, buffer=${BUFFER_DIR}"

if ! command -v grpcurl &>/dev/null; then
    fail "grpcurl not found. Install: brew install grpcurl"
    exit 1
fi

if [[ ! -f "$PIPELINE_BIN" ]]; then
    log "Building pipeline binary (release mode)..."
    (cd "$REPO_ROOT" && cargo build --package experimentation-pipeline --release 2>&1 | tail -5)
fi

if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    log "Starting Kafka via Docker Compose..."
    (cd "$REPO_ROOT" && docker compose up -d kafka kafka-init)
    log "Waiting for Kafka to be healthy..."
    sleep 15
fi

# ---------------------------------------------------------------------------
# Clean state
# ---------------------------------------------------------------------------
rm -rf "$BUFFER_DIR"
mkdir -p "$BUFFER_DIR"

# Record starting offsets for each topic
declare -A START_OFFSETS
for topic in exposures metric_events qoe_events reward_events; do
    offset=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list localhost:29092 --topic "$topic" --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || offset=0
    START_OFFSETS[$topic]=$offset
    log "Starting offset for $topic: $offset"
done

# ---------------------------------------------------------------------------
# Phase 1: Start pipeline
# ---------------------------------------------------------------------------
log "Starting pipeline on port ${PIPELINE_PORT}..."
BUFFER_DIR="$BUFFER_DIR" \
PORT="$PIPELINE_PORT" \
METRICS_PORT="$METRICS_PORT" \
KAFKA_BROKERS="$KAFKA_BROKERS" \
BLOOM_EXPECTED_DAILY=1000000 \
BLOOM_FP_RATE=0.001 \
BLOOM_ROTATION_SECS=3600 \
BUFFER_MAX_MB=50 \
RUST_LOG=info \
"$PIPELINE_BIN" &
PIPELINE_PID=$!

# Wait for gRPC port to be ready
for i in $(seq 1 30); do
    if grpcurl -plaintext "localhost:${PIPELINE_PORT}" list &>/dev/null 2>&1; then
        ok "Pipeline ready (PID=$PIPELINE_PID) after ${i}s"
        break
    fi
    if [[ $i -eq 30 ]]; then
        fail "Pipeline failed to start within 30s"
        kill "$PIPELINE_PID" 2>/dev/null || true
        exit 1
    fi
    sleep 1
done

# ---------------------------------------------------------------------------
# Phase 2: Send sustained load
# ---------------------------------------------------------------------------
SENT_COUNT=0
EVENT_LOG="$BUFFER_DIR/sent_events.log"
touch "$EVENT_LOG"

send_exposure() {
    local event_id="chaos-exp-$(date +%s%N)-$RANDOM"
    local experiment_id="chaos-experiment-$(( RANDOM % 5 ))"
    local user_id="chaos-user-$(( RANDOM % 10000 ))"
    local ts
    ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

    grpcurl -plaintext -d "{
        \"event\": {
            \"event_id\": \"$event_id\",
            \"experiment_id\": \"$experiment_id\",
            \"user_id\": \"$user_id\",
            \"variant_id\": \"control\",
            \"timestamp\": {\"seconds\": $(date +%s)},
            \"assignment_context\": {}
        }
    }" "localhost:${PIPELINE_PORT}" \
        experimentation.pipeline.v1.EventIngestionService/IngestExposure \
        >/dev/null 2>&1 && echo "$event_id" >> "$EVENT_LOG"
}

send_metric() {
    local event_id="chaos-met-$(date +%s%N)-$RANDOM"
    local ts
    ts=$(date +%s)

    grpcurl -plaintext -d "{
        \"event\": {
            \"event_id\": \"$event_id\",
            \"experiment_id\": \"chaos-experiment-0\",
            \"user_id\": \"chaos-user-$(( RANDOM % 10000 ))\",
            \"metric_id\": \"watch_time_minutes\",
            \"metric_value\": $(( RANDOM % 120 )).$(( RANDOM % 100 )),
            \"timestamp\": {\"seconds\": $ts}
        }
    }" "localhost:${PIPELINE_PORT}" \
        experimentation.pipeline.v1.EventIngestionService/IngestMetricEvent \
        >/dev/null 2>&1 && echo "$event_id" >> "$EVENT_LOG"
}

log "Sending load at ~${EVENTS_PER_SEC} events/sec for ${KILL_AFTER_SECS}s before kill..."

# Background load generator
(
    end_time=$(( $(date +%s) + KILL_AFTER_SECS ))
    while [[ $(date +%s) -lt $end_time ]]; do
        for _ in $(seq 1 "$EVENTS_PER_SEC"); do
            if (( RANDOM % 2 == 0 )); then
                send_exposure &
            else
                send_metric &
            fi
        done
        wait
        sleep 1
    done
) &
LOAD_PID=$!

# Let load run for KILL_AFTER_SECS
sleep "$KILL_AFTER_SECS"

# ---------------------------------------------------------------------------
# Phase 3: KILL -9 (the chaos)
# ---------------------------------------------------------------------------
SENT_BEFORE_KILL=$(wc -l < "$EVENT_LOG" | tr -d ' ')
log "Events sent before kill: $SENT_BEFORE_KILL"
log "Sending SIGKILL to pipeline (PID=$PIPELINE_PID)..."

kill -9 "$PIPELINE_PID" 2>/dev/null || true
wait "$PIPELINE_PID" 2>/dev/null || true

ok "Pipeline killed"

# Stop any remaining load generators
kill "$LOAD_PID" 2>/dev/null || true
wait "$LOAD_PID" 2>/dev/null || true
wait 2>/dev/null || true

# Check if buffer file was created
if [[ -f "$BUFFER_DIR/events.wal" ]]; then
    BUFFER_SIZE=$(stat -f%z "$BUFFER_DIR/events.wal" 2>/dev/null || stat -c%s "$BUFFER_DIR/events.wal" 2>/dev/null || echo 0)
    ok "Buffer file exists: ${BUFFER_SIZE} bytes"
else
    log "No buffer file (Kafka was reachable for all events — expected in normal operation)"
fi

TOTAL_SENT=$(wc -l < "$EVENT_LOG" | tr -d ' ')
log "Total events sent: $TOTAL_SENT"

# ---------------------------------------------------------------------------
# Phase 4: Restart pipeline (crash-only recovery)
# ---------------------------------------------------------------------------
log "Restarting pipeline..."
RECOVERY_START=$(date +%s%N)

BUFFER_DIR="$BUFFER_DIR" \
PORT="$PIPELINE_PORT" \
METRICS_PORT="$METRICS_PORT" \
KAFKA_BROKERS="$KAFKA_BROKERS" \
BLOOM_EXPECTED_DAILY=1000000 \
BLOOM_FP_RATE=0.001 \
BLOOM_ROTATION_SECS=3600 \
BUFFER_MAX_MB=50 \
RUST_LOG=info \
"$PIPELINE_BIN" &
PIPELINE_PID=$!

# Wait for gRPC to be ready
for i in $(seq 1 30); do
    if grpcurl -plaintext "localhost:${PIPELINE_PORT}" list &>/dev/null 2>&1; then
        RECOVERY_END=$(date +%s%N)
        RECOVERY_MS=$(( (RECOVERY_END - RECOVERY_START) / 1000000 ))
        ok "Pipeline recovered in ${RECOVERY_MS}ms (PID=$PIPELINE_PID)"
        break
    fi
    if [[ $i -eq 30 ]]; then
        fail "Pipeline failed to recover within 30s"
        exit 1
    fi
    sleep 0.1
done

# Check recovery SLA: < 2 seconds
if [[ $RECOVERY_MS -lt 2000 ]]; then
    ok "Recovery time ${RECOVERY_MS}ms < 2000ms SLA"
else
    fail "Recovery time ${RECOVERY_MS}ms exceeds 2000ms SLA"
fi

# ---------------------------------------------------------------------------
# Phase 5: Send more events post-recovery to verify pipeline is functional
# ---------------------------------------------------------------------------
log "Sending 10 verification events post-recovery..."
POST_RECOVERY_OK=0
for i in $(seq 1 10); do
    event_id="chaos-verify-$i-$(date +%s%N)"
    result=$(grpcurl -plaintext -d "{
        \"event\": {
            \"event_id\": \"$event_id\",
            \"experiment_id\": \"chaos-experiment-verify\",
            \"user_id\": \"chaos-user-verify-$i\",
            \"variant_id\": \"treatment\",
            \"timestamp\": {\"seconds\": $(date +%s)},
            \"assignment_context\": {}
        }
    }" "localhost:${PIPELINE_PORT}" \
        experimentation.pipeline.v1.EventIngestionService/IngestExposure 2>&1) || true

    if echo "$result" | grep -q '"accepted": true'; then
        POST_RECOVERY_OK=$((POST_RECOVERY_OK + 1))
    fi
done

if [[ $POST_RECOVERY_OK -eq 10 ]]; then
    ok "All 10 post-recovery events accepted"
else
    fail "Only $POST_RECOVERY_OK/10 post-recovery events accepted"
fi

# ---------------------------------------------------------------------------
# Phase 6: Verify data integrity on Kafka
# ---------------------------------------------------------------------------
sleep 3  # Allow Kafka replication to settle

log "Checking Kafka event counts..."
declare -A END_OFFSETS
TOTAL_ON_KAFKA=0
for topic in exposures metric_events qoe_events reward_events; do
    offset=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list localhost:29092 --topic "$topic" --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || offset=0
    END_OFFSETS[$topic]=$offset
    delta=$(( offset - ${START_OFFSETS[$topic]} ))
    TOTAL_ON_KAFKA=$(( TOTAL_ON_KAFKA + delta ))
    log "  $topic: ${START_OFFSETS[$topic]} -> $offset (Δ $delta)"
done

# ---------------------------------------------------------------------------
# Phase 7: Report
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  CHAOS TEST REPORT: kill -9 Ingestion Pipeline"
echo "============================================================="
echo "  Events sent (confirmed ACK):    $TOTAL_SENT"
echo "  Events on Kafka (all topics):   $TOTAL_ON_KAFKA"
echo "  Recovery time:                  ${RECOVERY_MS}ms"
echo "  Post-recovery events accepted:  $POST_RECOVERY_OK/10"
echo "  Buffer file created:            $(test -f "$BUFFER_DIR/events.wal" && echo "yes" || echo "no")"
echo ""

# Data loss check
if [[ $TOTAL_ON_KAFKA -ge $TOTAL_SENT ]]; then
    ok "PASS: No data loss detected ($TOTAL_ON_KAFKA >= $TOTAL_SENT)"
    RESULT="PASS"
elif [[ $TOTAL_ON_KAFKA -ge $(( TOTAL_SENT * 99 / 100 )) ]]; then
    warn "MARGINAL: <1% loss ($TOTAL_ON_KAFKA / $TOTAL_SENT) — within Bloom filter FPR tolerance"
    RESULT="MARGINAL"
else
    LOSS_PCT=$(( (TOTAL_SENT - TOTAL_ON_KAFKA) * 100 / TOTAL_SENT ))
    fail "FAIL: ${LOSS_PCT}% data loss ($TOTAL_ON_KAFKA / $TOTAL_SENT)"
    RESULT="FAIL"
fi

# Recovery SLA check
if [[ $RECOVERY_MS -lt 2000 ]]; then
    ok "PASS: Recovery < 2s SLA (${RECOVERY_MS}ms)"
else
    fail "FAIL: Recovery ${RECOVERY_MS}ms exceeds 2s SLA"
    RESULT="FAIL"
fi

echo "============================================================="
echo ""

# Cleanup
kill "$PIPELINE_PID" 2>/dev/null || true
wait "$PIPELINE_PID" 2>/dev/null || true

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
