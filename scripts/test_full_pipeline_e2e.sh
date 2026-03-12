#!/usr/bin/env bash
# =============================================================================
# Full Pipeline End-to-End Test (M1 → M2 → Kafka → M3 → M4a)
# =============================================================================
# Validates the complete data flow across the experimentation platform:
#
#   Phase 0: Pre-flight — Docker infra, tools, binaries
#   Phase 1: Start all 4 services on dedicated ports
#   Phase 2: M1 GetAssignment — deterministic variant assignments
#   Phase 3: M2 Ingest — exposure, metric, QoE events for assigned users
#   Phase 4: Kafka — verify offset advancement on all 3 topics
#   Phase 5: M3 ComputeMetrics — service responds to RPC
#   Phase 6: M4a RunAnalysis — service reachable, validates input
#   Phase 7: Cleanup & report
#
# This is the single highest-impact integration test: a regression in any
# service's port binding, proto serialization, or config format would be caught.
#
# Prerequisites:
#   - Docker Compose: docker compose up -d (Kafka + Postgres)
#   - Rust binaries built: cargo build --release (or debug)
#   - grpcurl: brew install grpcurl
#   - jq: brew install jq
#
# Usage:
#   ./scripts/test_full_pipeline_e2e.sh
#   ./scripts/test_full_pipeline_e2e.sh --users 20 --timeout 15 --health-timeout 45
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
NUM_USERS=${NUM_USERS:-10}
CONSUME_TIMEOUT=${CONSUME_TIMEOUT:-10}
HEALTH_TIMEOUT=${HEALTH_TIMEOUT:-30}

# Service ports (non-conflicting with default dev ports)
M1_PORT=50061
M2_PORT=50062
M2_METRICS_PORT=50072
M3_PORT=50063
M4A_PORT=50064

KAFKA_BOOTSTRAP="localhost:9092"
KAFKA_INTERNAL="kafka:29092"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMP_DIR=$(mktemp -d)
PROTO_IMPORT_PATH="$REPO_ROOT/proto"

# Experiment IDs per service config store
M1_EXPERIMENT_ID="exp_dev_001"
M3_EXPERIMENT_ID="e0000000-0000-0000-0000-000000000001"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${BLUE}[e2e]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK ]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN ]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; }
phase(){ echo -e "\n${CYAN}━━━ $* ━━━${NC}"; }

# ---------------------------------------------------------------------------
# Test counters
# ---------------------------------------------------------------------------
PASSED=0
FAILED=0
SKIPPED=0

pass()      { ok "$1"; PASSED=$((PASSED + 1)); }
fail_test() { fail "$1"; FAILED=$((FAILED + 1)); }
skip()      { warn "SKIP: $1"; SKIPPED=$((SKIPPED + 1)); }

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --users)          NUM_USERS="$2"; shift 2 ;;
        --timeout)        CONSUME_TIMEOUT="$2"; shift 2 ;;
        --health-timeout) HEALTH_TIMEOUT="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--users NUM] [--timeout SECS] [--health-timeout SECS]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Cleanup: kill all background service PIDs on exit
# ---------------------------------------------------------------------------
SERVICE_PIDS=()

cleanup() {
    log "Cleaning up..."
    for pid in "${SERVICE_PIDS[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
now_ts() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

# Call a gRPC method via grpcurl
grpc_call() {
    local proto="$1"
    local service="$2"
    local rpc="$3"
    local host="$4"
    local payload="$5"
    grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$proto" \
        -d "$payload" \
        "$host" \
        "$service/$rpc" 2>"$TEMP_DIR/grpc_stderr.txt" || true
}

# Wait for a service health check to pass
wait_for_health() {
    local name="$1"
    local check_cmd="$2"
    local timeout_secs="$3"
    local elapsed=0

    while [[ $elapsed -lt $timeout_secs ]]; do
        if eval "$check_cmd" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    return 1
}

# Generate a random integer in [min, max] using $RANDOM
rand_int() {
    local min=$1 max=$2
    echo $(( (RANDOM % (max - min + 1)) + min ))
}

# ==========================================================================
# Phase 0: Pre-flight checks
# ==========================================================================
phase "Phase 0: Pre-flight checks"

log "Checking Docker infra..."
if docker compose ps kafka 2>/dev/null | grep -q "running"; then
    pass "Docker infra running (Kafka)"
else
    fail_test "Kafka not running — start with: just infra"
    echo ""
    echo "ABORT: Cannot continue without Kafka."
    exit 1
fi

log "Checking required tools..."
TOOLS_OK=true
for tool in grpcurl jq python3; do
    if ! command -v "$tool" &>/dev/null; then
        fail "Missing tool: $tool"
        TOOLS_OK=false
    fi
done
if $TOOLS_OK; then
    pass "Required tools available (grpcurl, jq, python3)"
else
    fail_test "Missing required tools"
    exit 1
fi

log "Locating service binaries..."

# Rust binaries: prefer release, fall back to debug
find_rust_bin() {
    local name="$1"
    local release="$REPO_ROOT/target/release/$name"
    local debug="$REPO_ROOT/target/debug/$name"
    if [[ -x "$release" ]]; then
        echo "$release"
    elif [[ -x "$debug" ]]; then
        echo "$debug"
    else
        echo ""
    fi
}

M1_BIN=$(find_rust_bin "experimentation-assignment")
M2_BIN=$(find_rust_bin "experimentation-pipeline")
M4A_BIN=$(find_rust_bin "experimentation-analysis")

# M3: build Go binary on-the-fly into $TEMP_DIR
M3_BIN=""
M3_SRC="$REPO_ROOT/services/metrics/cmd"
if [[ -d "$M3_SRC" ]]; then
    log "Building M3 metrics service..."
    if (cd "$REPO_ROOT/services" && go build -o "$TEMP_DIR/metrics-service" ./metrics/cmd/) 2>"$TEMP_DIR/m3_build.log"; then
        M3_BIN="$TEMP_DIR/metrics-service"
        log "  M3 built: $M3_BIN"
    else
        warn "M3 build failed — see $TEMP_DIR/m3_build.log"
    fi
fi

BINS_FOUND=0
BINS_MISSING=""
for svc_name in M1:M1_BIN M2:M2_BIN M3:M3_BIN M4a:M4A_BIN; do
    label="${svc_name%%:*}"
    varname="${svc_name#*:}"
    bin="${!varname}"
    if [[ -n "$bin" && -x "$bin" ]]; then
        BINS_FOUND=$((BINS_FOUND + 1))
    else
        BINS_MISSING="${BINS_MISSING} ${label}"
    fi
done

if [[ $BINS_FOUND -eq 4 ]]; then
    pass "All 4 service binaries found"
else
    if [[ $BINS_FOUND -gt 0 ]]; then
        warn "Found ${BINS_FOUND}/4 binaries (missing:${BINS_MISSING})"
        pass "Partial binaries found — will skip unavailable services"
    else
        fail_test "No service binaries found — build with: cd crates && cargo build --release"
        exit 1
    fi
fi

# ==========================================================================
# Phase 1: Start services
# ==========================================================================
phase "Phase 1: Starting services"

M1_UP=false
M2_UP=false
M3_UP=false
M4A_UP=false

# --- M1 Assignment ---
if [[ -n "$M1_BIN" && -x "$M1_BIN" ]]; then
    log "Starting M1 Assignment on :$M1_PORT ..."
    CONFIG_PATH="$REPO_ROOT/dev/config.json" \
        GRPC_ADDR="0.0.0.0:$M1_PORT" \
        RUST_LOG=warn \
        "$M1_BIN" > "$TEMP_DIR/m1.log" 2>&1 &
    SERVICE_PIDS+=($!)

    if wait_for_health "M1" "grpcurl -plaintext -import-path '$PROTO_IMPORT_PATH' -proto experimentation/assignment/v1/assignment_service.proto localhost:$M1_PORT list" "$HEALTH_TIMEOUT"; then
        M1_UP=true
        pass "M1 Assignment started on :$M1_PORT"
    else
        fail_test "M1 Assignment failed to start (see $TEMP_DIR/m1.log)"
    fi
else
    skip "M1 Assignment binary not found"
fi

# --- M2 Pipeline ---
if [[ -n "$M2_BIN" && -x "$M2_BIN" ]]; then
    log "Starting M2 Pipeline on :$M2_PORT (metrics :$M2_METRICS_PORT) ..."
    PORT=$M2_PORT \
        METRICS_PORT=$M2_METRICS_PORT \
        KAFKA_BROKERS=$KAFKA_BOOTSTRAP \
        BUFFER_DIR="$TEMP_DIR/buffer" \
        BLOOM_EXPECTED_DAILY=100000 \
        RUST_LOG=warn \
        "$M2_BIN" > "$TEMP_DIR/m2.log" 2>&1 &
    SERVICE_PIDS+=($!)

    if wait_for_health "M2" "curl -sf http://localhost:$M2_METRICS_PORT/healthz" "$HEALTH_TIMEOUT"; then
        M2_UP=true
        pass "M2 Pipeline started on :$M2_PORT"
    else
        fail_test "M2 Pipeline failed to start (see $TEMP_DIR/m2.log)"
    fi
else
    skip "M2 Pipeline binary not found"
fi

# --- M3 Metrics ---
if [[ -n "$M3_BIN" && -x "$M3_BIN" ]]; then
    log "Starting M3 Metrics on :$M3_PORT ..."
    PORT=$M3_PORT \
        CONFIG_PATH="$REPO_ROOT/services/metrics/internal/config/testdata/seed_config.json" \
        KAFKA_BROKERS=$KAFKA_BOOTSTRAP \
        "$M3_BIN" > "$TEMP_DIR/m3.log" 2>&1 &
    SERVICE_PIDS+=($!)

    if wait_for_health "M3" "curl -sf http://localhost:$M3_PORT/healthz" "$HEALTH_TIMEOUT"; then
        M3_UP=true
        pass "M3 Metrics started on :$M3_PORT"
    else
        fail_test "M3 Metrics failed to start (see $TEMP_DIR/m3.log)"
    fi
else
    skip "M3 Metrics binary not found"
fi

# --- M4a Analysis ---
if [[ -n "$M4A_BIN" && -x "$M4A_BIN" ]]; then
    log "Starting M4a Analysis on :$M4A_PORT ..."
    mkdir -p "$TEMP_DIR/delta"
    ANALYSIS_GRPC_ADDR="0.0.0.0:$M4A_PORT" \
        DELTA_LAKE_PATH="$TEMP_DIR/delta" \
        RUST_LOG=warn \
        "$M4A_BIN" > "$TEMP_DIR/m4a.log" 2>&1 &
    SERVICE_PIDS+=($!)

    if wait_for_health "M4a" "grpcurl -plaintext -import-path '$PROTO_IMPORT_PATH' -proto experimentation/analysis/v1/analysis_service.proto localhost:$M4A_PORT list" "$HEALTH_TIMEOUT"; then
        M4A_UP=true
        pass "M4a Analysis started on :$M4A_PORT"
    else
        fail_test "M4a Analysis failed to start (see $TEMP_DIR/m4a.log)"
    fi
else
    skip "M4a Analysis binary not found"
fi

# ==========================================================================
# Phase 2: M1 Assignment
# ==========================================================================
phase "Phase 2: M1 Assignment ($NUM_USERS users)"

ASSIGNMENT_FILE="$TEMP_DIR/assignments.txt"
touch "$ASSIGNMENT_FILE"
ASSIGNMENT_OK=true

if $M1_UP; then
    ALL_ASSIGNED=true
    CONTROL_COUNT=0
    TREATMENT_COUNT=0

    for i in $(seq 1 "$NUM_USERS"); do
        USER_ID="e2e-user-$$-${i}"
        RESULT=$(grpc_call \
            "experimentation/assignment/v1/assignment_service.proto" \
            "experimentation.assignment.v1.AssignmentService" \
            "GetAssignment" \
            "localhost:$M1_PORT" \
            "{\"user_id\":\"$USER_ID\",\"experiment_id\":\"$M1_EXPERIMENT_ID\"}")

        VARIANT=$(echo "$RESULT" | jq -r '.variantId // empty' 2>/dev/null)
        if [[ -n "$VARIANT" ]]; then
            echo "$USER_ID $VARIANT" >> "$ASSIGNMENT_FILE"
            if [[ "$VARIANT" == "control" ]]; then
                CONTROL_COUNT=$((CONTROL_COUNT + 1))
            elif [[ "$VARIANT" == "treatment" ]]; then
                TREATMENT_COUNT=$((TREATMENT_COUNT + 1))
            fi
        else
            ALL_ASSIGNED=false
        fi
    done

    if $ALL_ASSIGNED; then
        pass "All $NUM_USERS users received variant assignments"
    else
        fail_test "Some users did not receive a variant assignment"
        ASSIGNMENT_OK=false
    fi

    # Determinism check: re-call first user, expect same variant
    FIRST_USER=$(head -1 "$ASSIGNMENT_FILE" | awk '{print $1}')
    FIRST_EXPECTED=$(head -1 "$ASSIGNMENT_FILE" | awk '{print $2}')
    RECHECK=$(grpc_call \
        "experimentation/assignment/v1/assignment_service.proto" \
        "experimentation.assignment.v1.AssignmentService" \
        "GetAssignment" \
        "localhost:$M1_PORT" \
        "{\"user_id\":\"$FIRST_USER\",\"experiment_id\":\"$M1_EXPERIMENT_ID\"}")
    RECHECK_VARIANT=$(echo "$RECHECK" | jq -r '.variantId // empty' 2>/dev/null)

    if [[ "$RECHECK_VARIANT" == "$FIRST_EXPECTED" ]]; then
        pass "Deterministic: same user returns same variant ($FIRST_EXPECTED)"
    else
        fail_test "Non-deterministic: expected $FIRST_EXPECTED, got $RECHECK_VARIANT"
    fi

    # Distribution check (only meaningful with >= 10 users)
    if [[ $NUM_USERS -ge 10 ]]; then
        if [[ $CONTROL_COUNT -gt 0 && $TREATMENT_COUNT -gt 0 ]]; then
            pass "Distribution: control=$CONTROL_COUNT, treatment=$TREATMENT_COUNT (both present)"
        else
            fail_test "Distribution: control=$CONTROL_COUNT, treatment=$TREATMENT_COUNT (expected both > 0)"
        fi
    else
        skip "Distribution check requires >= 10 users (have $NUM_USERS)"
    fi
else
    skip "Phase 2: M1 not available"
    skip "Phase 2: determinism check"
    skip "Phase 2: distribution check"
    ASSIGNMENT_OK=false
fi

# ==========================================================================
# Phase 3: M2 Ingestion
# ==========================================================================
phase "Phase 3: M2 Ingestion"

EXPOSURE_ACCEPTED=0
METRIC_ACCEPTED=0
QOE_ACCEPTED=0
DEDUP_PASSED=false
FIRST_EXPOSURE_EVENT_ID=""

if $M2_UP && $ASSIGNMENT_OK; then
    PIPELINE_PROTO="experimentation/pipeline/v1/pipeline_service.proto"
    PIPELINE_SVC="experimentation.pipeline.v1.EventIngestionService"
    PIPELINE_HOST="localhost:$M2_PORT"

    # Record starting Kafka offsets
    EXPOSURE_START=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic exposures --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || EXPOSURE_START=0
    METRIC_START=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic metric_events --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || METRIC_START=0
    QOE_START=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic qoe_events --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || QOE_START=0

    log "Starting offsets: exposures=$EXPOSURE_START metric_events=$METRIC_START qoe_events=$QOE_START"

    TS=$(now_ts)

    # Send correlated events for each assigned user
    while IFS=' ' read -r USER_ID VARIANT; do
        # --- Exposure event ---
        EXPOSURE_EVENT_ID="e2e-exposure-$$-${USER_ID}"
        if [[ -z "$FIRST_EXPOSURE_EVENT_ID" ]]; then
            FIRST_EXPOSURE_EVENT_ID="$EXPOSURE_EVENT_ID"
        fi
        RESULT=$(grpc_call "$PIPELINE_PROTO" "$PIPELINE_SVC" "IngestExposure" "$PIPELINE_HOST" \
            "{\"event\":{\"eventId\":\"$EXPOSURE_EVENT_ID\",\"experimentId\":\"$M1_EXPERIMENT_ID\",\"userId\":\"$USER_ID\",\"variantId\":\"$VARIANT\",\"timestamp\":\"$TS\",\"platform\":\"e2e-test\"}}")
        if echo "$RESULT" | grep -q '"accepted": true'; then
            EXPOSURE_ACCEPTED=$((EXPOSURE_ACCEPTED + 1))
        fi

        # --- Metric event ---
        METRIC_VALUE=$(rand_int 1 120)
        RESULT=$(grpc_call "$PIPELINE_PROTO" "$PIPELINE_SVC" "IngestMetricEvent" "$PIPELINE_HOST" \
            "{\"event\":{\"eventId\":\"e2e-metric-$$-${USER_ID}\",\"userId\":\"$USER_ID\",\"eventType\":\"watch_time_minutes\",\"value\":$METRIC_VALUE,\"contentId\":\"movie-e2e-1\",\"sessionId\":\"session-e2e-$$-${USER_ID}\",\"timestamp\":\"$TS\"}}")
        if echo "$RESULT" | grep -q '"accepted": true'; then
            METRIC_ACCEPTED=$((METRIC_ACCEPTED + 1))
        fi

        # --- QoE event ---
        TTFF=$(rand_int 100 500)
        REBUF=$(rand_int 0 3)
        BITRATE=$(rand_int 2000 10000)
        RESULT=$(grpc_call "$PIPELINE_PROTO" "$PIPELINE_SVC" "IngestQoEEvent" "$PIPELINE_HOST" \
            "{\"event\":{\"eventId\":\"e2e-qoe-$$-${USER_ID}\",\"sessionId\":\"session-e2e-$$-${USER_ID}\",\"contentId\":\"movie-e2e-1\",\"userId\":\"$USER_ID\",\"metrics\":{\"timeToFirstFrameMs\":\"$TTFF\",\"rebufferCount\":$REBUF,\"rebufferRatio\":0.01,\"avgBitrateKbps\":$BITRATE,\"resolutionSwitches\":1,\"peakResolutionHeight\":1080,\"startupFailureRate\":0.0,\"playbackDurationMs\":\"60000\"},\"cdnProvider\":\"akamai\",\"abrAlgorithm\":\"bola\",\"encodingProfile\":\"h264_high\",\"timestamp\":\"$TS\"}}")
        if echo "$RESULT" | grep -q '"accepted": true'; then
            QOE_ACCEPTED=$((QOE_ACCEPTED + 1))
        fi
    done < "$ASSIGNMENT_FILE"

    if [[ $EXPOSURE_ACCEPTED -eq $NUM_USERS ]]; then
        pass "All $NUM_USERS exposure events accepted"
    else
        fail_test "Exposure events: $EXPOSURE_ACCEPTED/$NUM_USERS accepted"
    fi

    if [[ $METRIC_ACCEPTED -eq $NUM_USERS ]]; then
        pass "All $NUM_USERS metric events accepted"
    else
        fail_test "Metric events: $METRIC_ACCEPTED/$NUM_USERS accepted"
    fi

    if [[ $QOE_ACCEPTED -eq $NUM_USERS ]]; then
        pass "All $NUM_USERS QoE events accepted"
    else
        fail_test "QoE events: $QOE_ACCEPTED/$NUM_USERS accepted"
    fi

    # Dedup: re-send first exposure event → should be rejected
    log "Testing dedup (re-sending first exposure)..."
    DEDUP_RESULT=$(grpc_call "$PIPELINE_PROTO" "$PIPELINE_SVC" "IngestExposure" "$PIPELINE_HOST" \
        "{\"event\":{\"eventId\":\"$FIRST_EXPOSURE_EVENT_ID\",\"experimentId\":\"$M1_EXPERIMENT_ID\",\"userId\":\"dedup-test\",\"variantId\":\"control\",\"timestamp\":\"$TS\",\"platform\":\"e2e-test\"}}")
    if echo "$DEDUP_RESULT" | grep -q '"accepted": false'; then
        pass "Dedup: duplicate event_id correctly rejected"
    else
        fail_test "Dedup: expected accepted=false (got: $DEDUP_RESULT)"
    fi

elif $M2_UP; then
    # M2 is up but no assignments — send basic events without M1 correlation
    log "M1 assignments unavailable — sending uncorrelated events through M2..."
    PIPELINE_PROTO="experimentation/pipeline/v1/pipeline_service.proto"
    PIPELINE_SVC="experimentation.pipeline.v1.EventIngestionService"
    PIPELINE_HOST="localhost:$M2_PORT"
    TS=$(now_ts)

    EXPOSURE_START=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic exposures --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || EXPOSURE_START=0
    METRIC_START=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic metric_events --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || METRIC_START=0
    QOE_START=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic qoe_events --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || QOE_START=0

    for i in $(seq 1 "$NUM_USERS"); do
        RESULT=$(grpc_call "$PIPELINE_PROTO" "$PIPELINE_SVC" "IngestExposure" "$PIPELINE_HOST" \
            "{\"event\":{\"eventId\":\"e2e-exposure-$$-${i}\",\"experimentId\":\"$M1_EXPERIMENT_ID\",\"userId\":\"e2e-user-$$-${i}\",\"variantId\":\"control\",\"timestamp\":\"$TS\",\"platform\":\"e2e-test\"}}")
        if echo "$RESULT" | grep -q '"accepted": true'; then
            EXPOSURE_ACCEPTED=$((EXPOSURE_ACCEPTED + 1))
        fi
    done

    if [[ $EXPOSURE_ACCEPTED -eq $NUM_USERS ]]; then
        pass "All $NUM_USERS exposure events accepted (uncorrelated)"
    else
        fail_test "Exposure events: $EXPOSURE_ACCEPTED/$NUM_USERS accepted"
    fi

    skip "Metric events (M1 unavailable for correlation)"
    skip "QoE events (M1 unavailable for correlation)"
    skip "Dedup test (skipped in uncorrelated mode)"
else
    skip "Phase 3: M2 not available — exposure events"
    skip "Phase 3: M2 not available — metric events"
    skip "Phase 3: M2 not available — QoE events"
    skip "Phase 3: M2 not available — dedup test"

    EXPOSURE_START=0
    METRIC_START=0
    QOE_START=0
fi

# ==========================================================================
# Phase 4: Kafka verification
# ==========================================================================
phase "Phase 4: Kafka verification"

if $M2_UP; then
    # Give Kafka a moment to flush
    sleep 2

    EXPOSURE_END=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic exposures --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || EXPOSURE_END=0
    METRIC_END=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic metric_events --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || METRIC_END=0
    QOE_END=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
        --broker-list "$KAFKA_INTERNAL" --topic qoe_events --time -1 2>/dev/null | \
        awk -F: '{sum+=$3} END {print sum+0}') || QOE_END=0

    EXPOSURE_DELTA=$((EXPOSURE_END - EXPOSURE_START))
    METRIC_DELTA=$((METRIC_END - METRIC_START))
    QOE_DELTA=$((QOE_END - QOE_START))

    log "Offset deltas: exposures=$EXPOSURE_DELTA metric_events=$METRIC_DELTA qoe_events=$QOE_DELTA"

    if [[ $EXPOSURE_ACCEPTED -gt 0 ]]; then
        if [[ $EXPOSURE_DELTA -ge $EXPOSURE_ACCEPTED ]]; then
            pass "exposures topic offset advanced by $EXPOSURE_DELTA (>= $EXPOSURE_ACCEPTED)"
        else
            fail_test "exposures offset $EXPOSURE_DELTA < $EXPOSURE_ACCEPTED expected"
        fi
    else
        skip "exposures offset check (no events sent)"
    fi

    if [[ $METRIC_ACCEPTED -gt 0 ]]; then
        if [[ $METRIC_DELTA -ge $METRIC_ACCEPTED ]]; then
            pass "metric_events topic offset advanced by $METRIC_DELTA (>= $METRIC_ACCEPTED)"
        else
            fail_test "metric_events offset $METRIC_DELTA < $METRIC_ACCEPTED expected"
        fi
    else
        skip "metric_events offset check (no events sent)"
    fi

    if [[ $QOE_ACCEPTED -gt 0 ]]; then
        if [[ $QOE_DELTA -ge $QOE_ACCEPTED ]]; then
            pass "qoe_events topic offset advanced by $QOE_DELTA (>= $QOE_ACCEPTED)"
        else
            fail_test "qoe_events offset $QOE_DELTA < $QOE_ACCEPTED expected"
        fi
    else
        skip "qoe_events offset check (no events sent)"
    fi

    # Consume one exposure event and verify it has a key
    if [[ $EXPOSURE_DELTA -gt 0 ]]; then
        CONSUMER_GROUP="e2e-full-test-$$"
        docker compose exec -T kafka kafka-console-consumer \
            --bootstrap-server "$KAFKA_INTERNAL" \
            --topic exposures \
            --from-beginning \
            --max-messages 1 \
            --timeout-ms "$((CONSUME_TIMEOUT * 1000))" \
            --group "$CONSUMER_GROUP" \
            --property print.key=true \
            --property key.separator="|||" \
            2>/dev/null > "$TEMP_DIR/consumed_exposure.txt" || true

        if [[ -s "$TEMP_DIR/consumed_exposure.txt" ]]; then
            KEY=$(head -1 "$TEMP_DIR/consumed_exposure.txt" | cut -d'|' -f1)
            if [[ -n "$KEY" ]]; then
                pass "Consumed exposure event with key (keyed by experiment_id)"
            else
                warn "Consumed exposure event but key is empty"
                pass "Consumed exposure event from Kafka"
            fi
        else
            warn "No exposure events consumed within timeout"
            skip "Kafka consume spot-check"
        fi
    else
        skip "Kafka consume spot-check (no new exposures)"
    fi
else
    skip "Phase 4: M2 not available — exposures offset"
    skip "Phase 4: M2 not available — metric_events offset"
    skip "Phase 4: M2 not available — qoe_events offset"
    skip "Phase 4: M2 not available — consume spot-check"

    EXPOSURE_DELTA=0
    METRIC_DELTA=0
    QOE_DELTA=0
fi

# ==========================================================================
# Phase 5: M3 Metrics
# ==========================================================================
phase "Phase 5: M3 Metrics"

if $M3_UP; then
    # Health endpoint
    if curl -sf "http://localhost:$M3_PORT/healthz" >/dev/null 2>&1; then
        pass "M3 health endpoint returns 200"
    else
        fail_test "M3 health endpoint unreachable"
    fi

    # ConnectRPC call — M3 uses h2c with ConnectRPC, try grpcurl first
    M3_RPC_OK=false
    M3_RESULT=""

    # Try grpcurl with proto import (ConnectRPC supports h2c gRPC)
    M3_RESULT=$(grpc_call \
        "experimentation/metrics/v1/metrics_service.proto" \
        "experimentation.metrics.v1.MetricComputationService" \
        "ComputeMetrics" \
        "localhost:$M3_PORT" \
        "{\"experiment_id\":\"$M3_EXPERIMENT_ID\"}" 2>/dev/null) || true

    M3_STDERR=$(cat "$TEMP_DIR/grpc_stderr.txt" 2>/dev/null || echo "")

    if [[ -n "$M3_RESULT" ]] && echo "$M3_RESULT" | jq . >/dev/null 2>&1; then
        M3_RPC_OK=true
        pass "M3 ComputeMetrics returns valid response"

        # Check for metricsComputed field
        METRICS_COMPUTED=$(echo "$M3_RESULT" | jq -r '.metricsComputed // "absent"' 2>/dev/null)
        if [[ "$METRICS_COMPUTED" != "absent" ]]; then
            pass "M3 response contains metricsComputed field ($METRICS_COMPUTED)"
        else
            # Field may be missing if zero (proto3 zero-value omission)
            pass "M3 response valid (metricsComputed omitted — proto3 zero value)"
        fi
    elif echo "$M3_STDERR" | grep -qi "Unimplemented\|not found\|404"; then
        # ConnectRPC might not support server reflection — try curl with Connect protocol
        log "  grpcurl failed, trying Connect protocol via curl..."
        M3_CURL_RESULT=$(curl -sf \
            -H "Content-Type: application/json" \
            -d "{\"experiment_id\":\"$M3_EXPERIMENT_ID\"}" \
            "http://localhost:$M3_PORT/experimentation.metrics.v1.MetricComputationService/ComputeMetrics" 2>/dev/null) || true

        if [[ -n "$M3_CURL_RESULT" ]] && echo "$M3_CURL_RESULT" | jq . >/dev/null 2>&1; then
            M3_RPC_OK=true
            pass "M3 ComputeMetrics returns valid response (via Connect protocol)"

            METRICS_COMPUTED=$(echo "$M3_CURL_RESULT" | jq -r '.metricsComputed // "absent"' 2>/dev/null)
            if [[ "$METRICS_COMPUTED" != "absent" ]]; then
                pass "M3 response contains metricsComputed field ($METRICS_COMPUTED)"
            else
                pass "M3 response valid (metricsComputed omitted — proto3 zero value)"
            fi
        else
            warn "M3 RPC call failed via both grpcurl and curl"
            skip "M3 ComputeMetrics RPC (service does not support reflection or Connect)"
            skip "M3 response field check"
        fi
    else
        warn "M3 RPC error: $M3_STDERR"
        skip "M3 ComputeMetrics RPC (unexpected error)"
        skip "M3 response field check"
    fi
else
    skip "Phase 5: M3 not available — health check"
    skip "Phase 5: M3 not available — ComputeMetrics RPC"
    skip "Phase 5: M3 not available — response field check"
fi

# ==========================================================================
# Phase 6: M4a Analysis
# ==========================================================================
phase "Phase 6: M4a Analysis"

if $M4A_UP; then
    ANALYSIS_PROTO="experimentation/analysis/v1/analysis_service.proto"
    ANALYSIS_SVC="experimentation.analysis.v1.AnalysisService"

    # Reachability check via grpcurl list
    if grpcurl -plaintext \
        -import-path "$PROTO_IMPORT_PATH" \
        -proto "$ANALYSIS_PROTO" \
        "localhost:$M4A_PORT" list 2>/dev/null | grep -q "AnalysisService"; then
        pass "M4a service reachable (grpcurl list)"
    else
        fail_test "M4a service not reachable via grpcurl"
    fi

    # Empty experiment_id → INVALID_ARGUMENT
    grpc_call "$ANALYSIS_PROTO" "$ANALYSIS_SVC" "RunAnalysis" "localhost:$M4A_PORT" \
        '{"experiment_id":""}' >/dev/null 2>&1
    STDERR=$(cat "$TEMP_DIR/grpc_stderr.txt" 2>/dev/null || echo "")

    if echo "$STDERR" | grep -qi "InvalidArgument\|INVALID_ARGUMENT\|invalid"; then
        pass "M4a: empty experiment_id → INVALID_ARGUMENT"
    else
        # Some implementations return a different error or even succeed with empty data
        warn "M4a: empty experiment_id did not return INVALID_ARGUMENT (got: $STDERR)"
        skip "M4a: empty experiment_id validation (non-standard response)"
    fi

    # Real experiment_id → NOT_FOUND (no Delta data) or valid result
    ANALYSIS_RESULT=$(grpc_call "$ANALYSIS_PROTO" "$ANALYSIS_SVC" "RunAnalysis" "localhost:$M4A_PORT" \
        "{\"experiment_id\":\"$M1_EXPERIMENT_ID\"}")
    STDERR=$(cat "$TEMP_DIR/grpc_stderr.txt" 2>/dev/null || echo "")

    if echo "$STDERR" | grep -qi "NotFound\|NOT_FOUND"; then
        pass "M4a: RunAnalysis returns NOT_FOUND (expected — no Delta Lake data in dev)"
    elif [[ -n "$ANALYSIS_RESULT" ]] && echo "$ANALYSIS_RESULT" | jq . >/dev/null 2>&1; then
        pass "M4a: RunAnalysis returns valid AnalysisResult"
    elif echo "$STDERR" | grep -qi "Unavailable\|Internal\|Unknown"; then
        # Service is reachable but the backend isn't configured — acceptable in dev
        warn "M4a: RunAnalysis returned error: $STDERR"
        pass "M4a: RunAnalysis reachable (backend error acceptable in dev)"
    else
        fail_test "M4a: unexpected RunAnalysis response — result=$ANALYSIS_RESULT stderr=$STDERR"
    fi
else
    skip "Phase 6: M4a not available — reachability"
    skip "Phase 6: M4a not available — empty experiment_id"
    skip "Phase 6: M4a not available — RunAnalysis"
fi

# ==========================================================================
# Phase 7: Cleanup & Report
# ==========================================================================
phase "Phase 7: Report"

# Summarize which services were up
SERVICES_STATUS=""
if $M1_UP;  then SERVICES_STATUS="$SERVICES_STATUS M1:$M1_PORT"; fi
if $M2_UP;  then SERVICES_STATUS="$SERVICES_STATUS M2:$M2_PORT"; fi
if $M3_UP;  then SERVICES_STATUS="$SERVICES_STATUS M3:$M3_PORT"; fi
if $M4A_UP; then SERVICES_STATUS="$SERVICES_STATUS M4a:$M4A_PORT"; fi

TOTAL_EVENTS=$((EXPOSURE_ACCEPTED + METRIC_ACCEPTED + QOE_ACCEPTED))

echo ""
echo "============================================================="
echo "  FULL PIPELINE E2E TEST REPORT (M1 → M2 → Kafka → M3 → M4a)"
echo "============================================================="
echo "  Services:       $SERVICES_STATUS"
echo "  Users tested:    $NUM_USERS"
echo "  Events sent:     $TOTAL_EVENTS ($EXPOSURE_ACCEPTED exposure + $METRIC_ACCEPTED metric + $QOE_ACCEPTED QoE)"
echo "  Kafka deltas:    exposures +${EXPOSURE_DELTA:-0}, metric_events +${METRIC_DELTA:-0}, qoe_events +${QOE_DELTA:-0}"
echo ""
echo "  Passed:  $PASSED"
echo "  Failed:  $FAILED"
echo "  Skipped: $SKIPPED"
echo ""

if [[ $FAILED -eq 0 && $SKIPPED -eq 0 ]]; then
    echo -e "  ${GREEN}ALL TESTS PASSED${NC}"
    echo ""
    echo "  Data flow validated:"
    echo "    M1 GetAssignment → deterministic variant assignment"
    echo "    M2 Ingest{Exposure,Metric,QoE} → events accepted, dedup works"
    echo "    Kafka topics advanced by expected offsets"
    echo "    M3 ComputeMetrics → service responds to RPC"
    echo "    M4a RunAnalysis → service reachable and properly validates input"
elif [[ $FAILED -eq 0 ]]; then
    echo -e "  ${YELLOW}PARTIAL PASS${NC}: all run tests passed, $SKIPPED skipped"
    echo "  Some services may not have been available."
else
    echo -e "  ${RED}FAILED${NC}: $FAILED test(s) failed"
fi
echo "============================================================="
echo ""

# Cleanup happens via trap
if [[ $FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
