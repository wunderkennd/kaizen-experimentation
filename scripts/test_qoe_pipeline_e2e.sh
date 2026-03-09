#!/usr/bin/env bash
# =============================================================================
# QoE Pipeline End-to-End Test (M3.5)
# =============================================================================
# Validates the full QoE event flow through the M2 pipeline:
#   1. Verify qoe_events topic exists with correct config (64 partitions)
#   2. Send valid QoE events via gRPC → verify accepted
#   3. Validation: missing PlaybackMetrics → INVALID_ARGUMENT
#   4. Validation: out-of-range values → INVALID_ARGUMENT
#   5. Dedup: same event_id → rejected (accepted: false)
#   6. Batch ingestion → verify batch response counts
#   7. Kafka offset advancement matches accepted events
#   8. Consume and spot-check protobuf content
#
# This validates milestone M3.5 (Playback QoE experiment pipeline).
# Agent-2 ingests QoE events; Agent-3 consumes from qoe_events for aggregation.
#
# Prerequisites:
#   - Docker Compose: docker compose up -d kafka kafka-init
#   - Pipeline service: cargo run -p experimentation-pipeline
#   - grpcurl: brew install grpcurl
#
# Usage:
#   ./scripts/test_qoe_pipeline_e2e.sh
#   ./scripts/test_qoe_pipeline_e2e.sh --events 20 --timeout 15
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
NUM_EVENTS=${NUM_EVENTS:-5}
CONSUME_TIMEOUT=${CONSUME_TIMEOUT:-10}
KAFKA_BOOTSTRAP="localhost:9092"
KAFKA_INTERNAL="kafka:29092"
TOPIC="qoe_events"
PIPELINE_HOST="localhost:50051"
EXPECTED_PARTITIONS=64
EXPECTED_RETENTION_MS=7776000000  # 90 days
CONSUMER_GROUP="qoe-e2e-test-$$"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMP_DIR=$(mktemp -d)
PROTO_IMPORT_PATH="$REPO_ROOT/proto"
PROTO_FILE="experimentation/pipeline/v1/pipeline_service.proto"
SERVICE="experimentation.pipeline.v1.EventIngestionService"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[qoe-e2e]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK ]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN ]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; }

cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --events)   NUM_EVENTS="$2"; shift 2 ;;
        --timeout)  CONSUME_TIMEOUT="$2"; shift 2 ;;
        --host)     PIPELINE_HOST="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--events NUM] [--timeout SECS] [--host HOST:PORT]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
PASSED=0
FAILED=0
SKIPPED=0

pass() { ok "$1"; PASSED=$((PASSED + 1)); }
fail_test() { fail "$1"; FAILED=$((FAILED + 1)); }
skip() { warn "SKIP: $1"; SKIPPED=$((SKIPPED + 1)); }

# Current timestamp in RFC 3339 format for protobuf JSON
now_ts() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

# Call gRPC via grpcurl, capturing stdout and stderr separately
grpc_call() {
    local rpc="$1"
    local payload="$2"
    grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$PROTO_FILE" \
        -d "$payload" \
        "$PIPELINE_HOST" \
        "$SERVICE/$rpc" 2>"$TEMP_DIR/grpc_stderr.txt" || true
}

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
log "QoE Pipeline E2E Test: ${NUM_EVENTS} events, ${CONSUME_TIMEOUT}s timeout"

if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    fail "Kafka not running. Start with: docker compose up -d kafka kafka-init"
    exit 1
fi

HAVE_GRPCURL=true
if ! command -v grpcurl &>/dev/null; then
    warn "grpcurl not found. Install with: brew install grpcurl"
    HAVE_GRPCURL=false
fi

# Check if pipeline service is reachable
PIPELINE_UP=false
if $HAVE_GRPCURL; then
    if grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$PROTO_FILE" \
        "$PIPELINE_HOST" list 2>/dev/null | grep -q "$SERVICE"; then
        PIPELINE_UP=true
        ok "Pipeline service reachable at $PIPELINE_HOST"
    else
        warn "Pipeline service not reachable at $PIPELINE_HOST"
        warn "Start with: cargo run -p experimentation-pipeline"
    fi
fi

# ==========================================================================
# Step 1: Verify topic exists and inspect config
# ==========================================================================
log "Step 1: Verifying '${TOPIC}' topic config..."

TOPIC_DESC=$(docker compose exec -T kafka kafka-topics \
    --bootstrap-server "$KAFKA_INTERNAL" \
    --describe --topic "$TOPIC" 2>/dev/null) || {
    fail_test "Topic '${TOPIC}' does not exist. Run: docker compose up -d kafka-init"
    echo ""
    echo "============================================================="
    echo "  ABORT: Cannot continue without qoe_events topic"
    echo "============================================================="
    exit 1
}

PARTITION_COUNT=$(echo "$TOPIC_DESC" | grep -c "Partition:" || echo 0)

if [[ $PARTITION_COUNT -ge $EXPECTED_PARTITIONS ]]; then
    pass "Topic '${TOPIC}' has ${PARTITION_COUNT} partitions (expected >= $EXPECTED_PARTITIONS)"
else
    fail_test "Topic '${TOPIC}' has ${PARTITION_COUNT} partitions (expected >= $EXPECTED_PARTITIONS)"
fi

# Check retention config
RETENTION=$(echo "$TOPIC_DESC" | head -1 | grep -oP 'retention\.ms=\K[0-9]+' || echo "unknown")
if [[ "$RETENTION" == "$EXPECTED_RETENTION_MS" ]]; then
    pass "Retention: ${RETENTION}ms (90 days)"
elif [[ "$RETENTION" == "unknown" ]]; then
    warn "Could not parse retention config — may use broker default"
else
    warn "Retention: ${RETENTION}ms (expected ${EXPECTED_RETENTION_MS})"
fi

# ==========================================================================
# Step 2: Record starting offsets
# ==========================================================================
log "Step 2: Recording starting offsets..."

START_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_INTERNAL" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || START_OFFSET=0

log "Starting offset: $START_OFFSET"

# ==========================================================================
# Step 3: Send valid QoE events via gRPC
# ==========================================================================
if $PIPELINE_UP; then
    log "Step 3: Sending valid QoE events via gRPC..."

    ACCEPTED_COUNT=0
    TS=$(now_ts)

    for i in $(seq 1 "$NUM_EVENTS"); do
        PAYLOAD=$(cat <<EOJSON
{
  "event": {
    "eventId": "qoe-e2e-${$}-${i}",
    "sessionId": "session-e2e-${$}-${i}",
    "contentId": "movie-test-${i}",
    "userId": "user-e2e-${i}",
    "metrics": {
      "timeToFirstFrameMs": "250",
      "rebufferCount": 1,
      "rebufferRatio": 0.02,
      "avgBitrateKbps": 5000,
      "resolutionSwitches": 2,
      "peakResolutionHeight": 1080,
      "startupFailureRate": 0.0,
      "playbackDurationMs": "60000"
    },
    "cdnProvider": "akamai",
    "abrAlgorithm": "bola",
    "encodingProfile": "h264_high",
    "timestamp": "${TS}"
  }
}
EOJSON
)
        RESULT=$(grpc_call "IngestQoEEvent" "$PAYLOAD")

        if echo "$RESULT" | grep -q '"accepted": true'; then
            ACCEPTED_COUNT=$((ACCEPTED_COUNT + 1))
        elif echo "$RESULT" | grep -q '"accepted"'; then
            # accepted: false means duplicate (Bloom filter hit)
            log "  Event qoe-e2e-${$}-${i}: duplicate (Bloom filter)"
        else
            STDERR=$(cat "$TEMP_DIR/grpc_stderr.txt" 2>/dev/null || echo "")
            warn "  Event qoe-e2e-${$}-${i}: unexpected response: $RESULT $STDERR"
        fi
    done

    if [[ $ACCEPTED_COUNT -eq $NUM_EVENTS ]]; then
        pass "All ${NUM_EVENTS} QoE events accepted"
    else
        fail_test "Only ${ACCEPTED_COUNT}/${NUM_EVENTS} events accepted"
    fi

    # ==========================================================================
    # Step 4: Validation — missing metrics → INVALID_ARGUMENT
    # ==========================================================================
    log "Step 4: Validation — missing PlaybackMetrics..."

    NO_METRICS_PAYLOAD=$(cat <<EOJSON
{
  "event": {
    "eventId": "qoe-e2e-no-metrics-${$}",
    "sessionId": "session-no-metrics",
    "contentId": "movie-test",
    "userId": "user-test",
    "timestamp": "$(now_ts)"
  }
}
EOJSON
)
    RESULT=$(grpc_call "IngestQoEEvent" "$NO_METRICS_PAYLOAD")
    STDERR=$(cat "$TEMP_DIR/grpc_stderr.txt" 2>/dev/null || echo "")

    if echo "$STDERR" | grep -qi "InvalidArgument\|INVALID_ARGUMENT\|invalid"; then
        pass "Missing metrics correctly rejected with INVALID_ARGUMENT"
    else
        fail_test "Missing metrics should return INVALID_ARGUMENT (got: $RESULT $STDERR)"
    fi

    # ==========================================================================
    # Step 5: Validation — out-of-range rebuffer_ratio
    # ==========================================================================
    log "Step 5: Validation — out-of-range rebuffer_ratio (>1.0)..."

    BAD_RATIO_PAYLOAD=$(cat <<EOJSON
{
  "event": {
    "eventId": "qoe-e2e-bad-ratio-${$}",
    "sessionId": "session-bad-ratio",
    "contentId": "movie-test",
    "userId": "user-test",
    "metrics": {
      "timeToFirstFrameMs": "250",
      "rebufferCount": 1,
      "rebufferRatio": 1.5,
      "avgBitrateKbps": 5000,
      "resolutionSwitches": 2,
      "peakResolutionHeight": 1080,
      "startupFailureRate": 0.0,
      "playbackDurationMs": "60000"
    },
    "timestamp": "$(now_ts)"
  }
}
EOJSON
)
    RESULT=$(grpc_call "IngestQoEEvent" "$BAD_RATIO_PAYLOAD")
    STDERR=$(cat "$TEMP_DIR/grpc_stderr.txt" 2>/dev/null || echo "")

    if echo "$STDERR" | grep -qi "InvalidArgument\|INVALID_ARGUMENT\|invalid"; then
        pass "Out-of-range rebuffer_ratio (1.5) correctly rejected"
    else
        fail_test "Out-of-range rebuffer_ratio should be rejected (got: $RESULT $STDERR)"
    fi

    # ==========================================================================
    # Step 6: Dedup — same event_id → rejected
    # ==========================================================================
    log "Step 6: Dedup — duplicate event_id..."

    DEDUP_PAYLOAD=$(cat <<EOJSON
{
  "event": {
    "eventId": "qoe-e2e-${$}-1",
    "sessionId": "session-dedup-test",
    "contentId": "movie-dedup",
    "userId": "user-dedup",
    "metrics": {
      "timeToFirstFrameMs": "300",
      "rebufferCount": 0,
      "rebufferRatio": 0.0,
      "avgBitrateKbps": 8000,
      "resolutionSwitches": 1,
      "peakResolutionHeight": 1080,
      "startupFailureRate": 0.0,
      "playbackDurationMs": "120000"
    },
    "timestamp": "$(now_ts)"
  }
}
EOJSON
)
    RESULT=$(grpc_call "IngestQoEEvent" "$DEDUP_PAYLOAD")

    if echo "$RESULT" | grep -q '"accepted": false'; then
        pass "Duplicate event_id correctly rejected by Bloom filter"
    else
        fail_test "Duplicate event_id should return accepted: false (got: $RESULT)"
    fi

    # ==========================================================================
    # Step 7: Batch ingestion
    # ==========================================================================
    log "Step 7: Batch ingestion (3 valid + 1 invalid)..."

    BATCH_PAYLOAD=$(cat <<EOJSON
{
  "events": [
    {
      "eventId": "qoe-e2e-batch-${$}-1",
      "sessionId": "session-batch-1",
      "contentId": "movie-batch-1",
      "userId": "user-batch-1",
      "metrics": {
        "timeToFirstFrameMs": "200",
        "rebufferCount": 0,
        "rebufferRatio": 0.0,
        "avgBitrateKbps": 10000,
        "resolutionSwitches": 0,
        "peakResolutionHeight": 2160,
        "startupFailureRate": 0.0,
        "playbackDurationMs": "90000"
      },
      "timestamp": "$(now_ts)"
    },
    {
      "eventId": "qoe-e2e-batch-${$}-2",
      "sessionId": "session-batch-2",
      "contentId": "movie-batch-2",
      "userId": "user-batch-2",
      "metrics": {
        "timeToFirstFrameMs": "500",
        "rebufferCount": 3,
        "rebufferRatio": 0.05,
        "avgBitrateKbps": 3000,
        "resolutionSwitches": 4,
        "peakResolutionHeight": 720,
        "startupFailureRate": 0.0,
        "playbackDurationMs": "45000"
      },
      "timestamp": "$(now_ts)"
    },
    {
      "eventId": "qoe-e2e-batch-${$}-3",
      "sessionId": "session-batch-3",
      "contentId": "movie-batch-3",
      "userId": "user-batch-3",
      "metrics": {
        "timeToFirstFrameMs": "1000",
        "rebufferCount": 10,
        "rebufferRatio": 0.1,
        "avgBitrateKbps": 1500,
        "resolutionSwitches": 8,
        "peakResolutionHeight": 480,
        "startupFailureRate": 0.0,
        "playbackDurationMs": "30000"
      },
      "timestamp": "$(now_ts)"
    },
    {
      "eventId": "qoe-e2e-batch-${$}-invalid",
      "sessionId": "session-batch-invalid",
      "contentId": "movie-batch-invalid",
      "userId": "user-batch-invalid",
      "timestamp": "$(now_ts)"
    }
  ]
}
EOJSON
)
    RESULT=$(grpc_call "IngestQoEEventBatch" "$BATCH_PAYLOAD")

    BATCH_ACCEPTED=$(echo "$RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('acceptedCount', 0))" 2>/dev/null || echo "?")
    BATCH_INVALID=$(echo "$RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('invalidCount', 0))" 2>/dev/null || echo "?")

    if [[ "$BATCH_ACCEPTED" == "3" && "$BATCH_INVALID" == "1" ]]; then
        pass "Batch: 3 accepted, 1 invalid (missing metrics)"
    else
        fail_test "Batch: expected 3 accepted + 1 invalid (got accepted=$BATCH_ACCEPTED, invalid=$BATCH_INVALID, response=$RESULT)"
    fi

    # Total events expected on Kafka: NUM_EVENTS (step 3) + 3 batch (step 7)
    EXPECTED_ON_KAFKA=$((NUM_EVENTS + 3))
else
    skip "Steps 3–7: Pipeline service not reachable (gRPC tests skipped)"
    EXPECTED_ON_KAFKA=0
fi

# ==========================================================================
# Step 8: Verify Kafka offset advancement
# ==========================================================================
log "Step 8: Verifying Kafka offset advancement..."

# Give Kafka a moment to flush
sleep 1

END_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_INTERNAL" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || END_OFFSET=0

DELTA=$((END_OFFSET - START_OFFSET))
log "Offset delta: $DELTA (start=$START_OFFSET, end=$END_OFFSET, expected=$EXPECTED_ON_KAFKA)"

if [[ $EXPECTED_ON_KAFKA -gt 0 ]]; then
    if [[ $DELTA -ge $EXPECTED_ON_KAFKA ]]; then
        pass "Kafka offset advanced by $DELTA >= $EXPECTED_ON_KAFKA expected events"
    else
        fail_test "Kafka offset delta $DELTA < $EXPECTED_ON_KAFKA expected"
    fi
else
    log "Skipping offset check (no gRPC events sent)"
fi

# ==========================================================================
# Step 9: Consume and spot-check
# ==========================================================================
if [[ $DELTA -gt 0 ]]; then
    log "Step 9: Consuming events for spot-check..."

    docker compose exec -T kafka kafka-console-consumer \
        --bootstrap-server "$KAFKA_INTERNAL" \
        --topic "$TOPIC" \
        --from-beginning \
        --max-messages 1 \
        --timeout-ms "$((CONSUME_TIMEOUT * 1000))" \
        --group "$CONSUMER_GROUP" \
        --property print.key=true \
        --property key.separator="|||" \
        2>/dev/null > "$TEMP_DIR/consumed.txt" || true

    if [[ -s "$TEMP_DIR/consumed.txt" ]]; then
        # Events are protobuf-encoded — we can at least verify key is present
        KEY=$(head -1 "$TEMP_DIR/consumed.txt" | cut -d'|' -f1)
        if [[ -n "$KEY" ]]; then
            pass "Consumed event with key: ${KEY} (QoE events keyed by session_id)"
        else
            warn "Consumed event but key is empty"
        fi
    else
        warn "No events consumed within timeout"
    fi
else
    log "Step 9: Skipped (no new events on topic)"
fi

# ==========================================================================
# Report
# ==========================================================================
echo ""
echo "============================================================="
echo "  QOE PIPELINE E2E TEST REPORT (M3.5)"
echo "============================================================="
echo "  Topic:            $TOPIC"
echo "  Partitions:       $PARTITION_COUNT"
echo "  Pipeline:         $(if $PIPELINE_UP; then echo "UP ($PIPELINE_HOST)"; else echo "NOT RUNNING"; fi)"
echo "  Events sent:      $EXPECTED_ON_KAFKA"
echo "  Kafka offset Δ:   $DELTA"
echo ""
echo "  Passed:           $PASSED"
echo "  Failed:           $FAILED"
echo "  Skipped:          $SKIPPED"
echo ""

if [[ $FAILED -eq 0 && $SKIPPED -eq 0 ]]; then
    ok "PASS: QoE pipeline working end-to-end"
    echo ""
    echo "  M3.5 validated: QoE events flow from gRPC ingest → Kafka"
    echo "  Agent-3 can consume from '${TOPIC}' for QoE metric aggregation"
elif [[ $FAILED -eq 0 ]]; then
    warn "PARTIAL: Topic config verified, gRPC tests skipped"
    echo "  Start pipeline: cargo run -p experimentation-pipeline"
else
    fail "FAIL: $FAILED test(s) failed"
fi
echo "============================================================="
echo ""

if [[ $FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
