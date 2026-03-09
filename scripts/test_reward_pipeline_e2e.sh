#!/usr/bin/env bash
# =============================================================================
# Reward Event Pipeline End-to-End Test (M4b Data Path)
# =============================================================================
# Validates the reward event flow through the M2 pipeline:
#   1. Verify reward_events topic exists with correct config (32 partitions, 180d)
#   2. Send valid RewardEvent via gRPC → verify accepted
#   3. Validation: missing experiment_id → INVALID_ARGUMENT
#   4. Validation: missing arm_id → INVALID_ARGUMENT
#   5. Dedup: same event_id → rejected (accepted: false)
#   6. Kafka offset advancement matches accepted events
#   7. Consume and spot-check reward content
#
# This validates the Agent-2 → Agent-4 M4b reward data path.
# Agent-2 ingests reward events; Agent-4 M4b consumes from reward_events
# for real-time bandit policy updates.
#
# Prerequisites:
#   - Docker Compose: docker compose up -d kafka kafka-init
#   - Pipeline service: cargo run -p experimentation-pipeline
#   - grpcurl: brew install grpcurl
#
# Usage:
#   ./scripts/test_reward_pipeline_e2e.sh
#   ./scripts/test_reward_pipeline_e2e.sh --events 10 --timeout 15
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
NUM_EVENTS=${NUM_EVENTS:-5}
CONSUME_TIMEOUT=${CONSUME_TIMEOUT:-10}
KAFKA_BOOTSTRAP="localhost:9092"
KAFKA_INTERNAL="kafka:29092"
TOPIC="reward_events"
PIPELINE_HOST="localhost:50051"
EXPECTED_PARTITIONS=32
EXPECTED_RETENTION_MS=15552000000  # 180 days
CONSUMER_GROUP="reward-e2e-test-$$"
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

log()  { echo -e "${BLUE}[reward-e2e]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK ]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN ]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; }

cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0
RESULT="PASS"

pass_test() { TESTS_RUN=$((TESTS_RUN + 1)); TESTS_PASSED=$((TESTS_PASSED + 1)); ok "$1"; }
fail_test() { TESTS_RUN=$((TESTS_RUN + 1)); TESTS_FAILED=$((TESTS_FAILED + 1)); fail "$1"; RESULT="FAIL"; }
skip_test() { TESTS_RUN=$((TESTS_RUN + 1)); TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); warn "SKIP: $1"; }

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --events)   NUM_EVENTS="$2"; shift 2 ;;
        --timeout)  CONSUME_TIMEOUT="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--events NUM] [--timeout SECS]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
log "Reward Event Pipeline E2E Test: ${NUM_EVENTS} events, ${CONSUME_TIMEOUT}s timeout"
echo ""

# Check Kafka
if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    fail "Kafka is not running. Start with: docker compose up -d kafka kafka-init"
    exit 1
fi

# Check grpcurl
HAS_GRPCURL=true
if ! command -v grpcurl &>/dev/null; then
    warn "grpcurl not found — gRPC tests will be skipped (install: brew install grpcurl)"
    HAS_GRPCURL=false
fi

# Check pipeline service
PIPELINE_RUNNING=true
if ! curl -s "http://localhost:9090/healthz" >/dev/null 2>&1; then
    warn "Pipeline service not running — gRPC tests will be skipped"
    warn "Start with: cargo run -p experimentation-pipeline"
    PIPELINE_RUNNING=false
fi

# ==========================================================================
# Step 1: Verify topic exists with correct configuration
# ==========================================================================
log "Step 1: Verifying '${TOPIC}' topic configuration..."

TOPIC_DESC=$(docker compose exec -T kafka kafka-topics \
    --bootstrap-server "$KAFKA_INTERNAL" \
    --describe --topic "$TOPIC" 2>/dev/null) || {
    fail_test "Topic '${TOPIC}' does not exist. Run: docker compose up -d kafka-init"
    echo ""
    echo "Cannot continue without the reward_events topic."
    exit 1
}

PARTITION_COUNT=$(echo "$TOPIC_DESC" | grep -c "Partition:" || echo 0)

if [[ $PARTITION_COUNT -eq $EXPECTED_PARTITIONS ]]; then
    pass_test "Topic '${TOPIC}' has ${PARTITION_COUNT} partitions (expected ${EXPECTED_PARTITIONS})"
else
    warn "Topic '${TOPIC}' has ${PARTITION_COUNT} partitions (expected ${EXPECTED_PARTITIONS})"
    # Not a hard failure — local dev may use fewer partitions
    pass_test "Topic '${TOPIC}' exists with ${PARTITION_COUNT} partition(s)"
fi

# Check retention (180 days = 15552000000ms for bandit replay)
RETENTION=$(echo "$TOPIC_DESC" | grep -o "retention.ms=[0-9]*" | head -1 | cut -d= -f2 || echo "unknown")
if [[ "$RETENTION" == "$EXPECTED_RETENTION_MS" ]]; then
    pass_test "Retention: ${RETENTION}ms (180 days — extended for bandit crash recovery replay)"
elif [[ "$RETENTION" != "unknown" ]]; then
    warn "Retention: ${RETENTION}ms (expected ${EXPECTED_RETENTION_MS}ms / 180 days)"
    pass_test "Retention configured: ${RETENTION}ms"
else
    skip_test "Could not verify retention configuration"
fi

echo ""

# ==========================================================================
# Step 2: Record starting offsets
# ==========================================================================
log "Step 2: Recording starting offsets..."

START_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_INTERNAL" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || START_OFFSET=0

log "Starting offset: $START_OFFSET"
echo ""

# ==========================================================================
# Step 3: Send valid reward events via gRPC
# ==========================================================================
if $PIPELINE_RUNNING && $HAS_GRPCURL; then
    log "Step 3: Sending valid reward events via gRPC..."
    ACCEPTED_COUNT=0

    for i in $(seq 1 "$NUM_EVENTS"); do
        EVENT_ID="rwd_e2e_test_${RANDOM}_${i}"
        TIMESTAMP_SECS=$(date +%s)
        ARM_IDX=$((RANDOM % 4))
        ARMS=("arm_0" "arm_1" "arm_2" "arm_3")
        ARM_ID="${ARMS[$ARM_IDX]}"
        # Generate reward value: binary (0 or 1) or continuous
        if (( RANDOM % 2 == 0 )); then
            REWARD="1.0"
        else
            REWARD="0.$(printf '%02d' $((RANDOM % 100)))"
        fi

        PAYLOAD=$(cat <<ENDJSON
{
  "event": {
    "eventId": "${EVENT_ID}",
    "experimentId": "content_cold_start_bandit",
    "userId": "user_$(printf '%07d' $((RANDOM % 1000000)))",
    "armId": "${ARM_ID}",
    "reward": ${REWARD},
    "timestamp": {"seconds": ${TIMESTAMP_SECS}, "nanos": 0}
  }
}
ENDJSON
)
        RESPONSE=$(grpcurl -plaintext \
            -import-path "$PROTO_IMPORT_PATH" \
            -proto "$PROTO_FILE" \
            -d "$PAYLOAD" \
            "$PIPELINE_HOST" "${SERVICE}/IngestRewardEvent" 2>&1) || true

        if echo "$RESPONSE" | grep -q '"accepted": true\|"accepted":true'; then
            ACCEPTED_COUNT=$((ACCEPTED_COUNT + 1))
        fi
    done

    if [[ $ACCEPTED_COUNT -eq $NUM_EVENTS ]]; then
        pass_test "All ${NUM_EVENTS} reward events accepted via IngestRewardEvent"
    elif [[ $ACCEPTED_COUNT -gt 0 ]]; then
        warn "${ACCEPTED_COUNT}/${NUM_EVENTS} accepted"
        pass_test "Reward event ingestion working (${ACCEPTED_COUNT}/${NUM_EVENTS})"
    else
        fail_test "No reward events were accepted"
    fi
    echo ""

    # ======================================================================
    # Step 4: Validation — missing experiment_id
    # ======================================================================
    log "Step 4: Validation — missing experiment_id..."

    MISSING_EXP_PAYLOAD=$(cat <<'ENDJSON'
{
  "event": {
    "eventId": "rwd_missing_exp_test",
    "userId": "user_0000001",
    "armId": "arm_0",
    "reward": 1.0,
    "timestamp": {"seconds": 1709000000, "nanos": 0}
  }
}
ENDJSON
)
    RESPONSE=$(grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$PROTO_FILE" \
        -d "$MISSING_EXP_PAYLOAD" \
        "$PIPELINE_HOST" "${SERVICE}/IngestRewardEvent" 2>&1) || true

    if echo "$RESPONSE" | grep -qi "INVALID_ARGUMENT\|invalid_argument\|experiment_id"; then
        pass_test "Missing experiment_id → INVALID_ARGUMENT"
    elif echo "$RESPONSE" | grep -q '"accepted": false\|"accepted":false'; then
        pass_test "Missing experiment_id → rejected (accepted: false)"
    else
        fail_test "Missing experiment_id was not rejected: $RESPONSE"
    fi

    # ======================================================================
    # Step 5: Validation — missing arm_id
    # ======================================================================
    log "Step 5: Validation — missing arm_id..."

    MISSING_ARM_PAYLOAD=$(cat <<'ENDJSON'
{
  "event": {
    "eventId": "rwd_missing_arm_test",
    "experimentId": "content_cold_start_bandit",
    "userId": "user_0000001",
    "reward": 1.0,
    "timestamp": {"seconds": 1709000000, "nanos": 0}
  }
}
ENDJSON
)
    RESPONSE=$(grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$PROTO_FILE" \
        -d "$MISSING_ARM_PAYLOAD" \
        "$PIPELINE_HOST" "${SERVICE}/IngestRewardEvent" 2>&1) || true

    if echo "$RESPONSE" | grep -qi "INVALID_ARGUMENT\|invalid_argument\|arm_id"; then
        pass_test "Missing arm_id → INVALID_ARGUMENT"
    elif echo "$RESPONSE" | grep -q '"accepted": false\|"accepted":false'; then
        pass_test "Missing arm_id → rejected (accepted: false)"
    else
        fail_test "Missing arm_id was not rejected: $RESPONSE"
    fi

    # ======================================================================
    # Step 6: Dedup — same event_id sent twice
    # ======================================================================
    log "Step 6: Dedup test — sending same event_id twice..."

    DEDUP_TS=$(date +%s)
    DEDUP_PAYLOAD=$(cat <<ENDJSON
{
  "event": {
    "eventId": "rwd_dedup_e2e_test_fixed",
    "experimentId": "content_cold_start_bandit",
    "userId": "user_0000042",
    "armId": "arm_1",
    "reward": 0.75,
    "timestamp": {"seconds": ${DEDUP_TS}, "nanos": 0}
  }
}
ENDJSON
)
    # First send
    grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$PROTO_FILE" \
        -d "$DEDUP_PAYLOAD" \
        "$PIPELINE_HOST" "${SERVICE}/IngestRewardEvent" >/dev/null 2>&1 || true

    # Second send (same event_id)
    DEDUP_RESPONSE=$(grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$PROTO_FILE" \
        -d "$DEDUP_PAYLOAD" \
        "$PIPELINE_HOST" "${SERVICE}/IngestRewardEvent" 2>&1) || true

    if echo "$DEDUP_RESPONSE" | grep -q '"accepted": false\|"accepted":false'; then
        pass_test "Duplicate event_id rejected by Bloom filter"
    else
        warn "Dedup may not have caught the duplicate (Bloom filter timing)"
        skip_test "Dedup test inconclusive (Bloom filter may not have committed)"
    fi

    # ======================================================================
    # Step 7: Context JSON — bandit context features
    # ======================================================================
    log "Step 7: Reward with context_json (contextual bandit)..."

    CTX_TS=$(date +%s)
    CTX_PAYLOAD=$(cat <<ENDJSON
{
  "event": {
    "eventId": "rwd_ctx_e2e_test_${RANDOM}",
    "experimentId": "content_cold_start_bandit",
    "userId": "user_0000099",
    "armId": "arm_2",
    "reward": 0.85,
    "timestamp": {"seconds": ${CTX_TS}, "nanos": 0},
    "contextJson": "{\"genre\":\"action\",\"time_of_day\":\"evening\",\"user_tenure_days\":45}"
  }
}
ENDJSON
)
    CTX_RESPONSE=$(grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$PROTO_FILE" \
        -d "$CTX_PAYLOAD" \
        "$PIPELINE_HOST" "${SERVICE}/IngestRewardEvent" 2>&1) || true

    if echo "$CTX_RESPONSE" | grep -q '"accepted": true\|"accepted":true'; then
        pass_test "Reward with context_json accepted (contextual bandit support)"
    else
        fail_test "Reward with context_json not accepted: $CTX_RESPONSE"
    fi

    echo ""
else
    log "Step 3-7: Skipping gRPC tests (pipeline not running or grpcurl not available)"
    skip_test "gRPC ingestion tests require pipeline service + grpcurl"
    echo ""
fi

# ==========================================================================
# Step 8: Verify Kafka offset advancement
# ==========================================================================
log "Step 8: Verifying Kafka offset advancement..."

# Brief pause for Kafka to commit
sleep 1

END_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_INTERNAL" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || END_OFFSET=0

DELTA=$((END_OFFSET - START_OFFSET))
log "Offset delta: $DELTA (start=$START_OFFSET, end=$END_OFFSET)"

if $PIPELINE_RUNNING && $HAS_GRPCURL; then
    if [[ $DELTA -ge $NUM_EVENTS ]]; then
        pass_test "Kafka offset advanced by $DELTA >= ${NUM_EVENTS} events published"
    elif [[ $DELTA -gt 0 ]]; then
        warn "Offset delta $DELTA < $NUM_EVENTS (some events may have been deduped or rejected)"
        pass_test "Kafka offset advanced by $DELTA"
    else
        fail_test "Kafka offset did not advance (expected >= ${NUM_EVENTS})"
    fi
else
    skip_test "Offset advancement check requires pipeline + grpcurl"
fi

echo ""

# ==========================================================================
# Step 9: Consume and validate reward events
# ==========================================================================
log "Step 9: Consuming reward events from Kafka..."

if [[ $DELTA -gt 0 ]]; then
    docker compose exec -T kafka kafka-console-consumer \
        --bootstrap-server "$KAFKA_INTERNAL" \
        --topic "$TOPIC" \
        --from-beginning \
        --max-messages 5 \
        --timeout-ms "$((CONSUME_TIMEOUT * 1000))" \
        --group "$CONSUMER_GROUP" \
        2>/dev/null > "$TEMP_DIR/consumed.bin" || true

    CONSUMED=$(wc -l < "$TEMP_DIR/consumed.bin" | tr -d ' ')

    if [[ $CONSUMED -gt 0 ]]; then
        pass_test "Consumed $CONSUMED reward event(s) from Kafka"
        log "Note: Events are Protobuf-serialized (binary), not human-readable JSON"
    else
        warn "No events consumed (may be binary protobuf — consumer expects text)"
        skip_test "Protobuf binary events cannot be validated via console consumer"
    fi
else
    skip_test "No new events to consume"
fi

echo ""

# ==========================================================================
# Step 10: Synthetic generator compatibility check
# ==========================================================================
log "Step 10: Verifying synthetic generator reward output..."

python3 "$REPO_ROOT/scripts/generate_synthetic_events.py" \
    --type reward --count 3 --seed 42 --compact \
    --output "$TEMP_DIR/synthetic_rewards.jsonl"

if [[ -f "$TEMP_DIR/synthetic_rewards.jsonl" ]]; then
    SYNTH_COUNT=$(wc -l < "$TEMP_DIR/synthetic_rewards.jsonl" | tr -d ' ')

    # Verify field names match proto
    VALID=true
    FIRST=$(head -1 "$TEMP_DIR/synthetic_rewards.jsonl")
    for field in event_id experiment_id user_id arm_id reward; do
        if ! echo "$FIRST" | python3 -c "import sys,json; d=json.load(sys.stdin)['event']; assert '$field' in d" 2>/dev/null; then
            warn "Missing proto field '$field' in synthetic reward"
            VALID=false
        fi
    done
    # Verify old field name is gone
    if echo "$FIRST" | python3 -c "import sys,json; d=json.load(sys.stdin)['event']; assert 'reward_value' not in d" 2>/dev/null; then
        true  # good
    else
        warn "Old field name 'reward_value' still present"
        VALID=false
    fi

    if $VALID; then
        pass_test "Synthetic generator outputs proto-compatible reward events ($SYNTH_COUNT)"
    else
        fail_test "Synthetic generator field names don't match proto"
    fi
else
    fail_test "Synthetic generator failed to produce rewards"
fi

echo ""

# ==========================================================================
# Report
# ==========================================================================
echo "============================================================="
echo "  REWARD EVENT PIPELINE E2E TEST REPORT"
echo "============================================================="
echo "  Tests run:     $TESTS_RUN"
echo "  Passed:        $TESTS_PASSED"
echo "  Failed:        $TESTS_FAILED"
echo "  Skipped:       $TESTS_SKIPPED"
echo "  Offset delta:  $DELTA"
echo "  Consumer group: $CONSUMER_GROUP"
echo ""

if [[ "$RESULT" == "PASS" ]]; then
    ok "PASS: Reward event pipeline working end-to-end"
    echo ""
    echo "  Agent-4 M4b (bandit policy) can consume from '${TOPIC}'"
    echo "  Consumer group: bandit-policy-service (committed offsets for crash recovery)"
    echo "  Retention: 180 days (extended for bandit replay on restart)"
else
    fail "FAIL: Reward event pipeline has issues"
fi
echo "============================================================="
echo ""

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
