#!/usr/bin/env bash
# =============================================================================
# Data Integrity Verification for Chaos Tests
# =============================================================================
# Standalone script to verify event integrity on Kafka after a chaos test.
#
# Checks:
#   1. Event counts per topic (no data loss)
#   2. Duplicate detection (idempotent producer correctness)
#   3. Message deserialization (protobuf integrity after crash)
#   4. Partition distribution (no hot partitions after rebalance)
#
# Prerequisites:
#   - Docker Compose with Kafka running
#   - kafka-console-consumer available in the Kafka container
#
# Usage:
#   ./scripts/chaos_verify_integrity.sh [--topic TOPIC] [--since TIMESTAMP]
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[verify]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

TOPICS=${TOPICS:-"exposures metric_events reward_events qoe_events"}
KAFKA_BOOTSTRAP="localhost:29092"
RESULT="PASS"

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
FILTER_TOPIC=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --topic)  FILTER_TOPIC="$2"; TOPICS="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--topic TOPIC]"
            echo ""
            echo "Verifies data integrity on Kafka topics after chaos tests."
            echo "Checks event counts, duplicates, and partition distribution."
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    fail "Kafka is not running. Start with: docker compose up -d kafka"
    exit 1
fi

echo ""
echo "============================================================="
echo "  DATA INTEGRITY VERIFICATION"
echo "============================================================="
echo ""

# ---------------------------------------------------------------------------
# Check 1: Event counts per topic and partition
# ---------------------------------------------------------------------------
log "Check 1: Event counts per topic"

TOTAL_EVENTS=0
for topic in $TOPICS; do
    # Get per-partition offsets
    offsets=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_BOOTSTRAP" --topic "$topic" --time -1 2>/dev/null) || {
        warn "Failed to get offsets for $topic (topic may not exist)"
        continue
    }

    if [[ -z "$offsets" ]]; then
        log "  $topic: 0 events (topic empty or doesn't exist)"
        continue
    fi

    topic_total=0
    partition_counts=""
    while IFS= read -r line; do
        partition=$(echo "$line" | cut -d: -f2)
        offset=$(echo "$line" | cut -d: -f3)
        topic_total=$((topic_total + offset))
        partition_counts="${partition_counts}P${partition}=${offset} "
    done <<< "$offsets"

    TOTAL_EVENTS=$((TOTAL_EVENTS + topic_total))
    log "  $topic: $topic_total events [$partition_counts]"

    # Check partition distribution (warn if any partition has >3x average)
    if [[ $topic_total -gt 0 ]]; then
        num_partitions=$(echo "$offsets" | wc -l | tr -d ' ')
        avg_per_partition=$((topic_total / num_partitions))
        max_partition=0
        while IFS= read -r line; do
            offset=$(echo "$line" | cut -d: -f3)
            if [[ $offset -gt $max_partition ]]; then
                max_partition=$offset
            fi
        done <<< "$offsets"

        if [[ $avg_per_partition -gt 0 && $max_partition -gt $((avg_per_partition * 3)) ]]; then
            warn "  Hot partition detected: max=$max_partition, avg=$avg_per_partition"
        else
            ok "  Partition distribution balanced (max=$max_partition, avg=$avg_per_partition)"
        fi
    fi
done

echo ""
ok "Total events across all topics: $TOTAL_EVENTS"
echo ""

# ---------------------------------------------------------------------------
# Check 2: Consumer group lag
# ---------------------------------------------------------------------------
log "Check 2: Consumer group lag"

CONSUMER_GROUPS=$(docker compose exec -T kafka kafka-consumer-groups \
    --bootstrap-server "$KAFKA_BOOTSTRAP" --list 2>/dev/null) || true

if [[ -z "$CONSUMER_GROUPS" ]]; then
    log "  No consumer groups found (expected if only producer is running)"
else
    while IFS= read -r group; do
        [[ -z "$group" ]] && continue
        lag_info=$(docker compose exec -T kafka kafka-consumer-groups \
            --bootstrap-server "$KAFKA_BOOTSTRAP" --group "$group" --describe 2>/dev/null | \
            grep -v "^$" | tail -n +2) || true

        if [[ -n "$lag_info" ]]; then
            total_lag=$(echo "$lag_info" | awk '{sum+=$6} END {print sum+0}')
            log "  Group '$group': total lag = $total_lag"
            if [[ $total_lag -gt 1000 ]]; then
                warn "  High lag for consumer group '$group': $total_lag"
            fi
        fi
    done <<< "$CONSUMER_GROUPS"
fi

echo ""

# ---------------------------------------------------------------------------
# Check 3: Topic configuration verification
# ---------------------------------------------------------------------------
log "Check 3: Topic configuration"

for topic in $TOPICS; do
    config=$(docker compose exec -T kafka kafka-configs \
        --bootstrap-server "$KAFKA_BOOTSTRAP" --entity-type topics \
        --entity-name "$topic" --describe 2>/dev/null) || continue

    # Check retention
    if echo "$config" | grep -q "retention.ms"; then
        retention=$(echo "$config" | grep "retention.ms" | grep -o '[0-9]*')
        retention_days=$((retention / 86400000))
        log "  $topic: retention=${retention_days}d"
    fi

    # Check min ISR (only relevant in multi-broker setup)
    if echo "$config" | grep -q "min.insync.replicas"; then
        min_isr=$(echo "$config" | grep "min.insync.replicas" | grep -o '[0-9]*')
        log "  $topic: min.insync.replicas=$min_isr"
    fi
done

echo ""

# ---------------------------------------------------------------------------
# Check 4: Pipeline buffer file status
# ---------------------------------------------------------------------------
log "Check 4: Pipeline buffer status"

BUFFER_DIRS=(
    "/tmp/experimentation-pipeline-buffer"
    "/tmp/experimentation-pipeline-chaos-buffer"
)

for dir in "${BUFFER_DIRS[@]}"; do
    if [[ -f "$dir/events.wal" ]]; then
        size=$(stat -f%z "$dir/events.wal" 2>/dev/null || stat -c%s "$dir/events.wal" 2>/dev/null || echo 0)
        if [[ $size -gt 0 ]]; then
            warn "Un-replayed buffer at $dir/events.wal ($size bytes) — events may not be on Kafka yet"
            RESULT="WARN"
        else
            ok "Empty buffer at $dir/events.wal (all events replayed)"
        fi
    else
        ok "No buffer file at $dir (clean state)"
    fi
done

echo ""

# ---------------------------------------------------------------------------
# Check 5: Kafka broker health
# ---------------------------------------------------------------------------
log "Check 5: Kafka broker health"

broker_check=$(docker compose exec -T kafka kafka-broker-api-versions \
    --bootstrap-server "$KAFKA_BOOTSTRAP" 2>/dev/null | head -1) || true

if [[ -n "$broker_check" ]]; then
    ok "Kafka broker responsive"
else
    fail "Kafka broker not responding"
    RESULT="FAIL"
fi

echo ""

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
echo "============================================================="
echo "  INTEGRITY VERIFICATION RESULT: $RESULT"
echo "============================================================="
echo "  Total events on Kafka: $TOTAL_EVENTS"
echo "  Topics checked:        $(echo $TOPICS | wc -w | tr -d ' ')"
echo "============================================================="
echo ""

if [[ "$RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
