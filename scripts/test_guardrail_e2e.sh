#!/usr/bin/env bash
# =============================================================================
# Guardrail Alert Integration Test Harness
# =============================================================================
# Tests the guardrail_alerts Kafka topic end-to-end:
#   1. Verify topic exists with correct config
#   2. Publish synthetic GuardrailAlert events (JSON-serialized)
#   3. Consume and verify events arrive within expected window
#   4. Check consumer group offset advancement
#
# This unblocks Agent-5 (management auto-pause) <-> Agent-3 (metric engine)
# pair integration by validating the Kafka transport layer.
#
# Prerequisites:
#   - Docker Compose running: docker compose up -d kafka kafka-init
#   - Python 3.8+ (for synthetic event generator)
#
# Usage:
#   ./scripts/test_guardrail_e2e.sh
#   ./scripts/test_guardrail_e2e.sh --alerts 20 --timeout 30
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
NUM_ALERTS=${NUM_ALERTS:-10}
CONSUME_TIMEOUT=${CONSUME_TIMEOUT:-15}
KAFKA_BOOTSTRAP="localhost:9092"
KAFKA_INTERNAL="kafka:29092"
TOPIC="guardrail_alerts"
CONSUMER_GROUP="guardrail-e2e-test-$$"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMP_DIR=$(mktemp -d)

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[guardrail-e2e]${NC} $*"; }
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
        --alerts)   NUM_ALERTS="$2"; shift 2 ;;
        --timeout)  CONSUME_TIMEOUT="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--alerts NUM] [--timeout SECS]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
log "Guardrail Alert E2E Test: ${NUM_ALERTS} alerts, ${CONSUME_TIMEOUT}s timeout"

if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    fail "Kafka is not running. Start with: docker compose up -d kafka kafka-init"
    exit 1
fi

if ! command -v python3 &>/dev/null; then
    fail "python3 not found"
    exit 1
fi

RESULT="PASS"

# ---------------------------------------------------------------------------
# Step 1: Verify topic exists and inspect config
# ---------------------------------------------------------------------------
log "Step 1: Verifying '${TOPIC}' topic exists..."

TOPIC_DESC=$(docker compose exec -T kafka kafka-topics \
    --bootstrap-server "$KAFKA_INTERNAL" \
    --describe --topic "$TOPIC" 2>/dev/null) || {
    fail "Topic '${TOPIC}' does not exist. Run: docker compose up -d kafka-init"
    exit 1
}

PARTITION_COUNT=$(echo "$TOPIC_DESC" | grep -c "Partition:" || echo 0)
ok "Topic '${TOPIC}' exists with ${PARTITION_COUNT} partition(s)"
echo "$TOPIC_DESC" | head -3

# ---------------------------------------------------------------------------
# Step 2: Record starting offset
# ---------------------------------------------------------------------------
log "Step 2: Recording starting offsets..."

START_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_INTERNAL" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || START_OFFSET=0

log "Starting offset: $START_OFFSET"

# ---------------------------------------------------------------------------
# Step 3: Generate and publish synthetic guardrail alerts
# ---------------------------------------------------------------------------
log "Step 3: Generating ${NUM_ALERTS} synthetic guardrail alerts..."

python3 "$REPO_ROOT/scripts/generate_synthetic_events.py" \
    --type guardrail_alert \
    --count "$NUM_ALERTS" \
    --seed 42 \
    --compact \
    --output "$TEMP_DIR/alerts.jsonl"

ok "Generated ${NUM_ALERTS} alerts to $TEMP_DIR/alerts.jsonl"

log "Publishing alerts to Kafka topic '${TOPIC}'..."
PUBLISHED=0

while IFS= read -r line; do
    # Extract the event payload (strip the wrapper {"type": "guardrail_alert", "event": ...})
    event_json=$(echo "$line" | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps(d['event']))")

    echo "$event_json" | docker compose exec -T kafka kafka-console-producer \
        --broker-list "$KAFKA_INTERNAL" \
        --topic "$TOPIC" 2>/dev/null && PUBLISHED=$((PUBLISHED + 1))
done < "$TEMP_DIR/alerts.jsonl"

ok "Published ${PUBLISHED}/${NUM_ALERTS} alerts to '${TOPIC}'"

if [[ $PUBLISHED -ne $NUM_ALERTS ]]; then
    warn "Not all alerts were published ($PUBLISHED / $NUM_ALERTS)"
    RESULT="MARGINAL"
fi

# ---------------------------------------------------------------------------
# Step 4: Consume events and verify arrival
# ---------------------------------------------------------------------------
log "Step 4: Consuming events (timeout: ${CONSUME_TIMEOUT}s)..."

docker compose exec -T kafka kafka-console-consumer \
    --bootstrap-server "$KAFKA_INTERNAL" \
    --topic "$TOPIC" \
    --from-beginning \
    --max-messages "$NUM_ALERTS" \
    --timeout-ms "$((CONSUME_TIMEOUT * 1000))" \
    --group "$CONSUMER_GROUP" \
    2>/dev/null > "$TEMP_DIR/consumed.jsonl" || true

CONSUMED=$(wc -l < "$TEMP_DIR/consumed.jsonl" | tr -d ' ')
log "Consumed: $CONSUMED events"

if [[ $CONSUMED -ge $NUM_ALERTS ]]; then
    ok "All ${NUM_ALERTS} alerts consumed successfully"
else
    fail "Only consumed $CONSUMED / $NUM_ALERTS alerts"
    RESULT="FAIL"
fi

# ---------------------------------------------------------------------------
# Step 5: Verify offset advancement
# ---------------------------------------------------------------------------
log "Step 5: Verifying offset advancement..."

END_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_INTERNAL" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || END_OFFSET=0

DELTA=$((END_OFFSET - START_OFFSET))
log "Offset delta: $DELTA (start=$START_OFFSET, end=$END_OFFSET)"

if [[ $DELTA -ge $NUM_ALERTS ]]; then
    ok "Offset advanced by $DELTA >= $NUM_ALERTS alerts published"
else
    fail "Offset delta $DELTA < $NUM_ALERTS expected"
    RESULT="FAIL"
fi

# ---------------------------------------------------------------------------
# Step 6: Validate event content (spot check)
# ---------------------------------------------------------------------------
log "Step 6: Validating event content..."

if [[ $CONSUMED -gt 0 ]]; then
    FIRST_EVENT=$(head -1 "$TEMP_DIR/consumed.jsonl")

    # Check for required fields
    VALID=true
    for field in experiment_id metric_id variant_id current_value threshold consecutive_breach_count; do
        if ! echo "$FIRST_EVENT" | python3 -c "import sys,json; d=json.load(sys.stdin); assert '$field' in d" 2>/dev/null; then
            warn "Missing field '$field' in consumed event"
            VALID=false
        fi
    done

    if $VALID; then
        ok "Event content validated — all required fields present"
        # Print a sample event
        echo "$FIRST_EVENT" | python3 -m json.tool 2>/dev/null | head -15
    else
        warn "Some fields missing — events may use protobuf binary format"
    fi
else
    warn "No events consumed, skipping content validation"
fi

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  GUARDRAIL ALERT E2E TEST REPORT"
echo "============================================================="
echo "  Alerts generated:   $NUM_ALERTS"
echo "  Alerts published:   $PUBLISHED"
echo "  Alerts consumed:    $CONSUMED"
echo "  Offset delta:       $DELTA"
echo "  Consumer group:     $CONSUMER_GROUP"
echo ""

if [[ "$RESULT" == "PASS" ]]; then
    ok "PASS: Guardrail alert pipeline working end-to-end"
    echo ""
    echo "  Agent-5 (management) can now consume from '${TOPIC}'"
    echo "  Agent-3 (metrics) can now publish to '${TOPIC}'"
elif [[ "$RESULT" == "MARGINAL" ]]; then
    warn "MARGINAL: Partial success — investigate publish failures"
else
    fail "FAIL: Guardrail alert pipeline has issues"
fi
echo "============================================================="
echo ""

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
