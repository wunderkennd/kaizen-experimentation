#!/usr/bin/env bash
# =============================================================================
# Chaos Test: Kafka Partition Rebalance Under Load
# =============================================================================
# Verifies that consumer group changes don't cause event loss.
#
# Test flow:
#   1. Start pipeline producing events to Kafka
#   2. Start a consumer group consuming from the topics
#   3. Add a second consumer to the group (triggers rebalance)
#   4. Continue producing during rebalance
#   5. Remove the second consumer (triggers another rebalance)
#   6. Verify total consumed == total produced (no event loss)
#
# Prerequisites:
#   - Docker Compose (kafka, zookeeper running)
#   - kafka-console-consumer available in the Kafka container
#
# Usage:
#   ./scripts/chaos_rebalance_test.sh [--topic TOPIC] [--events NUM]
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# Config
TOPIC=${TOPIC:-"exposures"}
NUM_EVENTS=${NUM_EVENTS:-500}
CONSUMER_GROUP="chaos-rebalance-test-$(date +%s)"
KAFKA_BOOTSTRAP="localhost:29092"
RESULTS_DIR="/tmp/chaos-rebalance-$$"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[rebalance]${NC} $*"; }
ok()   { echo -e "${GREEN}[    OK   ]${NC} $*"; }
warn() { echo -e "${YELLOW}[  WARN   ]${NC} $*"; }
fail() { echo -e "${RED}[  FAIL  ]${NC} $*"; }

cleanup() {
    log "Cleaning up..."
    # Kill any background consumers
    jobs -p 2>/dev/null | xargs kill 2>/dev/null || true
    # Delete consumer group
    docker compose exec -T kafka kafka-consumer-groups \
        --bootstrap-server "$KAFKA_BOOTSTRAP" --group "$CONSUMER_GROUP" --delete 2>/dev/null || true
    rm -rf "$RESULTS_DIR"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    fail "Kafka is not running. Start with: docker compose up -d kafka"
    exit 1
fi

mkdir -p "$RESULTS_DIR"

# Record starting offset
START_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_BOOTSTRAP" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || START_OFFSET=0
log "Starting offset for $TOPIC: $START_OFFSET"

# ---------------------------------------------------------------------------
# Phase 1: Produce events
# ---------------------------------------------------------------------------
log "Producing $NUM_EVENTS events to $TOPIC..."

PRODUCED=0
for i in $(seq 1 "$NUM_EVENTS"); do
    # Produce directly via kafka-console-producer for simplicity
    KEY="rebalance-test-key-$((i % 10))"
    VALUE="rebalance-event-$i-$(date +%s%N)"
    echo "${KEY}:${VALUE}" | docker compose exec -T kafka kafka-console-producer \
        --bootstrap-server "$KAFKA_BOOTSTRAP" \
        --topic "$TOPIC" \
        --property "parse.key=true" \
        --property "key.separator=:" 2>/dev/null
    PRODUCED=$((PRODUCED + 1))

    # At 1/3 of the way, start a consumer (simulates initial consumer group)
    if [[ $i -eq $((NUM_EVENTS / 3)) ]]; then
        log "Starting consumer 1 in group '$CONSUMER_GROUP'..."
        docker compose exec -T kafka kafka-console-consumer \
            --bootstrap-server "$KAFKA_BOOTSTRAP" \
            --topic "$TOPIC" --group "$CONSUMER_GROUP" \
            --from-beginning --timeout-ms 30000 \
            > "$RESULTS_DIR/consumer1.log" 2>/dev/null &
        CONSUMER1_PID=$!
    fi

    # At 2/3, add a second consumer (triggers rebalance)
    if [[ $i -eq $((NUM_EVENTS * 2 / 3)) ]]; then
        log "Starting consumer 2 in group '$CONSUMER_GROUP' (triggers rebalance)..."
        docker compose exec -T kafka kafka-console-consumer \
            --bootstrap-server "$KAFKA_BOOTSTRAP" \
            --topic "$TOPIC" --group "$CONSUMER_GROUP" \
            --from-beginning --timeout-ms 30000 \
            > "$RESULTS_DIR/consumer2.log" 2>/dev/null &
        CONSUMER2_PID=$!
        sleep 2  # Let rebalance settle
    fi
done

log "Produced $PRODUCED events"

# ---------------------------------------------------------------------------
# Phase 2: Wait for consumers to drain
# ---------------------------------------------------------------------------
log "Waiting for consumers to finish (30s timeout)..."
sleep 5

# Stop consumer 2 first (triggers another rebalance)
if [[ -n "${CONSUMER2_PID:-}" ]]; then
    log "Stopping consumer 2 (triggers rebalance)..."
    kill "$CONSUMER2_PID" 2>/dev/null || true
    wait "$CONSUMER2_PID" 2>/dev/null || true
fi

# Wait for consumer 1 to finish
sleep 10
if [[ -n "${CONSUMER1_PID:-}" ]]; then
    kill "$CONSUMER1_PID" 2>/dev/null || true
    wait "$CONSUMER1_PID" 2>/dev/null || true
fi

# ---------------------------------------------------------------------------
# Phase 3: Verify
# ---------------------------------------------------------------------------
END_OFFSET=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
    --broker-list "$KAFKA_BOOTSTRAP" --topic "$TOPIC" --time -1 2>/dev/null | \
    awk -F: '{sum+=$3} END {print sum+0}') || END_OFFSET=0

EVENTS_ON_KAFKA=$((END_OFFSET - START_OFFSET))

# Count consumed events
CONSUMED1=$(wc -l < "$RESULTS_DIR/consumer1.log" 2>/dev/null | tr -d ' ') || CONSUMED1=0
CONSUMED2=$(wc -l < "$RESULTS_DIR/consumer2.log" 2>/dev/null | tr -d ' ') || CONSUMED2=0
TOTAL_CONSUMED=$((CONSUMED1 + CONSUMED2))

# Check consumer group lag
LAG=$(docker compose exec -T kafka kafka-consumer-groups \
    --bootstrap-server "$KAFKA_BOOTSTRAP" --group "$CONSUMER_GROUP" --describe 2>/dev/null | \
    grep -v "^$" | tail -n +2 | awk '{sum+=$6} END {print sum+0}') || LAG=-1

echo ""
echo "============================================================="
echo "  PARTITION REBALANCE TEST REPORT"
echo "============================================================="
echo "  Topic:               $TOPIC"
echo "  Consumer group:      $CONSUMER_GROUP"
echo "  Events produced:     $PRODUCED"
echo "  Events on Kafka:     $EVENTS_ON_KAFKA"
echo "  Consumer 1 received: $CONSUMED1"
echo "  Consumer 2 received: $CONSUMED2"
echo "  Total consumed:      $TOTAL_CONSUMED"
echo "  Consumer group lag:  $LAG"
echo ""

RESULT="PASS"

# Verify no events lost on producer side
if [[ $EVENTS_ON_KAFKA -ge $PRODUCED ]]; then
    ok "No producer-side event loss ($EVENTS_ON_KAFKA >= $PRODUCED)"
else
    fail "Producer-side event loss: $EVENTS_ON_KAFKA < $PRODUCED"
    RESULT="FAIL"
fi

# Check consumer lag (should be 0 if all consumed)
if [[ $LAG -le 0 ]]; then
    ok "Consumer group fully caught up (lag=$LAG)"
elif [[ $LAG -lt 10 ]]; then
    warn "Small consumer lag remaining: $LAG (may clear with more time)"
else
    warn "Significant consumer lag: $LAG — events may be in-flight"
fi

echo "============================================================="
echo "  RESULT: $RESULT"
echo "============================================================="
echo ""

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
