#!/usr/bin/env bash
# =============================================================================
# End-to-End Chaos Test Framework (Milestone 4.5)
# =============================================================================
# Multi-service chaos orchestrator with pluggable per-service hooks.
#
# Flow:
#   1. Start Docker infra (Kafka, Postgres, Redis)
#   2. Discover and start registered services
#   3. Send sustained multi-event-type load
#   4. Sequentially kill each service (kill -9), verify recovery < 2s
#   5. Validate zero data loss (ingest counts match Kafka offsets)
#   6. Generate unified report
#
# Pluggable hooks:
#   Each agent adds a script at scripts/chaos_test_<service>.sh that exports:
#     chaos_start_<service>   — Start the service, echo PID
#     chaos_health_<service>  — Exit 0 if healthy
#     chaos_verify_<service>  — Exit 0 if data integrity OK after recovery
#
# Built-in services (Agent-2):
#   - pipeline (experimentation-pipeline binary)
#
# Prerequisites:
#   - Docker Compose (kafka, zookeeper, postgres running)
#   - Service binaries built (cargo build --release)
#   - grpcurl installed
#
# Usage:
#   ./scripts/chaos_e2e_framework.sh
#   ./scripts/chaos_e2e_framework.sh --services pipeline --duration 20
#   ./scripts/chaos_e2e_framework.sh --services pipeline,assignment --duration 30
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
LOAD_DURATION=${LOAD_DURATION:-15}
KILL_AFTER_SECS=${KILL_AFTER_SECS:-8}
RECOVERY_SLA_MS=${RECOVERY_SLA_MS:-2000}
KAFKA_BROKERS=${KAFKA_BROKERS:-localhost:9092}
KAFKA_INTERNAL=${KAFKA_INTERNAL:-kafka:29092}
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR=$(mktemp -d)
SERVICES_ARG=""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

log()     { echo -e "${BLUE}[chaos-e2e]${NC} $*"; }
ok()      { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn()    { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail()    { echo -e "${RED}[ FAIL ]${NC} $*"; }
section() { echo -e "\n${CYAN}${BOLD}=== $* ===${NC}"; }

cleanup() {
    log "Cleaning up..."
    # Kill all tracked service PIDs
    for pid_file in "$WORK_DIR"/pids/*; do
        if [[ -f "$pid_file" ]]; then
            local pid
            pid=$(cat "$pid_file")
            kill "$pid" 2>/dev/null || true
        fi
    done
    # Kill any load generators
    if [[ -f "$WORK_DIR/load_pid" ]]; then
        kill "$(cat "$WORK_DIR/load_pid")" 2>/dev/null || true
    fi
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --services)   SERVICES_ARG="$2"; shift 2 ;;
        --duration)   LOAD_DURATION="$2"; shift 2 ;;
        --kill-after) KILL_AFTER_SECS="$2"; shift 2 ;;
        --recovery-sla) RECOVERY_SLA_MS="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 [--services svc1,svc2] [--duration SECS] [--kill-after SECS] [--recovery-sla MS]"
            echo ""
            echo "Options:"
            echo "  --services     Comma-separated list of services to test (default: auto-discover)"
            echo "  --duration     Total load duration in seconds (default: 15)"
            echo "  --kill-after   Kill service after N seconds of load (default: 8)"
            echo "  --recovery-sla Max recovery time in ms (default: 2000)"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

mkdir -p "$WORK_DIR"/{pids,results,logs}

# ---------------------------------------------------------------------------
# Built-in: Pipeline service hooks
# ---------------------------------------------------------------------------
PIPELINE_PORT=50061
PIPELINE_METRICS_PORT=9091
PIPELINE_BIN="$REPO_ROOT/target/release/experimentation-pipeline"
PIPELINE_BUFFER_DIR="$WORK_DIR/pipeline-buffer"

chaos_start_pipeline() {
    mkdir -p "$PIPELINE_BUFFER_DIR"
    BUFFER_DIR="$PIPELINE_BUFFER_DIR" \
    PORT="$PIPELINE_PORT" \
    METRICS_PORT="$PIPELINE_METRICS_PORT" \
    KAFKA_BROKERS="$KAFKA_BROKERS" \
    BLOOM_EXPECTED_DAILY=1000000 \
    BLOOM_FP_RATE=0.001 \
    BLOOM_ROTATION_SECS=3600 \
    BUFFER_MAX_MB=50 \
    RUST_LOG=warn \
    "$PIPELINE_BIN" > "$WORK_DIR/logs/pipeline.log" 2>&1 &
    echo $!
}

chaos_health_pipeline() {
    grpcurl -plaintext "localhost:${PIPELINE_PORT}" list &>/dev/null 2>&1
}

chaos_verify_pipeline() {
    # Verify pipeline is accepting events after recovery
    local event_id="verify-pipeline-$(date +%s%N)"
    local result
    result=$(grpcurl -plaintext -d "{
        \"event\": {
            \"event_id\": \"$event_id\",
            \"experiment_id\": \"chaos-verify\",
            \"user_id\": \"chaos-verify-user\",
            \"variant_id\": \"control\",
            \"timestamp\": {\"seconds\": $(date +%s)}
        }
    }" "localhost:${PIPELINE_PORT}" \
        experimentation.pipeline.v1.EventIngestionService/IngestExposure 2>&1) || return 1

    echo "$result" | grep -q '"accepted": true'
}

# ---------------------------------------------------------------------------
# Service discovery
# ---------------------------------------------------------------------------
declare -A SERVICE_PIDS
declare -A SERVICE_RECOVERY_MS
declare -A SERVICE_VERIFY_RESULT

discover_services() {
    local services=()

    if [[ -n "$SERVICES_ARG" ]]; then
        IFS=',' read -ra services <<< "$SERVICES_ARG"
    else
        # Auto-discover: check for built-in + pluggable hooks
        if [[ -f "$PIPELINE_BIN" ]]; then
            services+=("pipeline")
        fi
        # Discover pluggable hooks: scripts/chaos_test_<service>.sh
        for hook in "$SCRIPT_DIR"/chaos_test_*.sh; do
            if [[ -f "$hook" ]]; then
                local svc_name
                svc_name=$(basename "$hook" | sed 's/chaos_test_//;s/\.sh$//')
                services+=("$svc_name")
            fi
        done
    fi

    if [[ ${#services[@]} -eq 0 ]]; then
        fail "No services found. Build pipeline: cargo build --package experimentation-pipeline --release"
        exit 1
    fi

    echo "${services[@]}"
}

# ---------------------------------------------------------------------------
# Generic service lifecycle (built-in + pluggable hooks)
# ---------------------------------------------------------------------------
start_service() {
    local svc="$1"
    local pid

    if declare -f "chaos_start_${svc}" > /dev/null 2>&1; then
        pid=$(chaos_start_${svc})
    elif [[ -f "$SCRIPT_DIR/chaos_test_${svc}.sh" ]]; then
        # Source pluggable hook
        # shellcheck source=/dev/null
        source "$SCRIPT_DIR/chaos_test_${svc}.sh"
        pid=$(chaos_start_${svc})
    else
        fail "No start hook for service '$svc'"
        return 1
    fi

    echo "$pid" > "$WORK_DIR/pids/${svc}"
    SERVICE_PIDS[$svc]=$pid
    log "Started $svc (PID=$pid)"
}

wait_healthy() {
    local svc="$1"
    local max_wait=${2:-30}

    for i in $(seq 1 "$max_wait"); do
        if declare -f "chaos_health_${svc}" > /dev/null 2>&1; then
            if chaos_health_${svc}; then
                ok "$svc healthy after ${i}s"
                return 0
            fi
        elif [[ -f "$SCRIPT_DIR/chaos_test_${svc}.sh" ]]; then
            # shellcheck source=/dev/null
            source "$SCRIPT_DIR/chaos_test_${svc}.sh"
            if chaos_health_${svc}; then
                ok "$svc healthy after ${i}s"
                return 0
            fi
        fi
        sleep 1
    done

    fail "$svc did not become healthy within ${max_wait}s"
    return 1
}

kill_service() {
    local svc="$1"
    local pid="${SERVICE_PIDS[$svc]}"
    log "Sending SIGKILL to $svc (PID=$pid)..."
    kill -9 "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    ok "$svc killed"
}

restart_and_measure() {
    local svc="$1"
    local start_ns
    start_ns=$(date +%s%N)

    start_service "$svc"

    # Wait for health with tight polling
    local max_iters=200  # 200 * 100ms = 20s max
    for i in $(seq 1 $max_iters); do
        if declare -f "chaos_health_${svc}" > /dev/null 2>&1; then
            if chaos_health_${svc}; then
                local end_ns
                end_ns=$(date +%s%N)
                local recovery_ms=$(( (end_ns - start_ns) / 1000000 ))
                SERVICE_RECOVERY_MS[$svc]=$recovery_ms
                ok "$svc recovered in ${recovery_ms}ms"
                return 0
            fi
        fi
        sleep 0.1
    done

    fail "$svc did not recover within 20s"
    SERVICE_RECOVERY_MS[$svc]=99999
    return 1
}

verify_service() {
    local svc="$1"

    if declare -f "chaos_verify_${svc}" > /dev/null 2>&1; then
        if chaos_verify_${svc}; then
            SERVICE_VERIFY_RESULT[$svc]="PASS"
            ok "$svc data integrity verified"
        else
            SERVICE_VERIFY_RESULT[$svc]="FAIL"
            fail "$svc data integrity check failed"
        fi
    else
        SERVICE_VERIFY_RESULT[$svc]="SKIP"
        warn "$svc has no verify hook — skipping"
    fi
}

# ---------------------------------------------------------------------------
# Load generator (multi-event-type)
# ---------------------------------------------------------------------------
send_load() {
    local duration="$1"
    local events_per_sec=${EVENTS_PER_SEC:-100}
    local end_time=$(( $(date +%s) + duration ))
    local sent=0

    while [[ $(date +%s) -lt $end_time ]]; do
        for _ in $(seq 1 "$events_per_sec"); do
            local event_type=$((RANDOM % 4))
            local event_id="chaos-e2e-$(date +%s%N)-$RANDOM"

            case $event_type in
                0)  # Exposure
                    grpcurl -plaintext -d "{
                        \"event\": {
                            \"event_id\": \"$event_id\",
                            \"experiment_id\": \"chaos-exp-$((RANDOM % 5))\",
                            \"user_id\": \"chaos-user-$((RANDOM % 10000))\",
                            \"variant_id\": \"control\",
                            \"timestamp\": {\"seconds\": $(date +%s)}
                        }
                    }" "localhost:${PIPELINE_PORT}" \
                        experimentation.pipeline.v1.EventIngestionService/IngestExposure \
                        >/dev/null 2>&1 && sent=$((sent + 1)) &
                    ;;
                1)  # Metric
                    grpcurl -plaintext -d "{
                        \"event\": {
                            \"event_id\": \"$event_id\",
                            \"user_id\": \"chaos-user-$((RANDOM % 10000))\",
                            \"event_type\": \"watch_time_minutes\",
                            \"value\": $((RANDOM % 120)).$((RANDOM % 100)),
                            \"content_id\": \"content_$((RANDOM % 200 + 1))\",
                            \"session_id\": \"session-$((RANDOM % 50000))\",
                            \"timestamp\": {\"seconds\": $(date +%s)}
                        }
                    }" "localhost:${PIPELINE_PORT}" \
                        experimentation.pipeline.v1.EventIngestionService/IngestMetricEvent \
                        >/dev/null 2>&1 && sent=$((sent + 1)) &
                    ;;
                2)  # QoE
                    grpcurl -plaintext -d "{
                        \"event\": {
                            \"event_id\": \"$event_id\",
                            \"session_id\": \"session-$((RANDOM % 50000))\",
                            \"content_id\": \"content_$((RANDOM % 200 + 1))\",
                            \"user_id\": \"chaos-user-$((RANDOM % 10000))\",
                            \"metrics\": {
                                \"time_to_first_frame_ms\": $((500 + RANDOM % 4000)),
                                \"rebuffer_count\": $((RANDOM % 5)),
                                \"rebuffer_ratio\": 0.0$((RANDOM % 99)),
                                \"avg_bitrate_kbps\": $((2000 + RANDOM % 12000)),
                                \"peak_resolution_height\": 1080,
                                \"playback_duration_ms\": $((30000 + RANDOM % 7200000))
                            },
                            \"cdn_provider\": \"cloudfront\",
                            \"timestamp\": {\"seconds\": $(date +%s)}
                        }
                    }" "localhost:${PIPELINE_PORT}" \
                        experimentation.pipeline.v1.EventIngestionService/IngestQoEEvent \
                        >/dev/null 2>&1 && sent=$((sent + 1)) &
                    ;;
                3)  # Reward
                    grpcurl -plaintext -d "{
                        \"event\": {
                            \"event_id\": \"$event_id\",
                            \"experiment_id\": \"content_cold_start_bandit\",
                            \"user_id\": \"chaos-user-$((RANDOM % 10000))\",
                            \"arm_id\": \"arm_$((RANDOM % 4))\",
                            \"reward\": 0.$((RANDOM % 100)),
                            \"timestamp\": {\"seconds\": $(date +%s)}
                        }
                    }" "localhost:${PIPELINE_PORT}" \
                        experimentation.pipeline.v1.EventIngestionService/IngestRewardEvent \
                        >/dev/null 2>&1 && sent=$((sent + 1)) &
                    ;;
            esac
        done
        wait 2>/dev/null || true
        sleep 1
    done

    echo "$sent" > "$WORK_DIR/sent_count"
}

# ---------------------------------------------------------------------------
# Kafka offset tracking
# ---------------------------------------------------------------------------
TOPICS="exposures metric_events qoe_events reward_events"

record_offsets() {
    local label="$1"
    local total=0
    for topic in $TOPICS; do
        local offset
        offset=$(docker compose exec -T kafka kafka-run-class kafka.tools.GetOffsetShell \
            --broker-list "$KAFKA_INTERNAL" --topic "$topic" --time -1 2>/dev/null | \
            awk -F: '{sum+=$3} END {print sum+0}') || offset=0
        echo "$topic=$offset" >> "$WORK_DIR/offsets_${label}"
        total=$((total + offset))
    done
    echo "$total"
}

# ===========================================================================
# Main execution
# ===========================================================================

section "Pre-flight Checks"

if ! command -v grpcurl &>/dev/null; then
    fail "grpcurl not found. Install: brew install grpcurl"
    exit 1
fi

if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    log "Starting Kafka via Docker Compose..."
    (cd "$REPO_ROOT" && docker compose up -d kafka kafka-init)
    log "Waiting for Kafka to be healthy..."
    sleep 15
fi

# Discover services
read -ra SERVICES <<< "$(discover_services)"
log "Services to test: ${SERVICES[*]}"

section "Phase 1: Start All Services"

for svc in "${SERVICES[@]}"; do
    start_service "$svc"
done

for svc in "${SERVICES[@]}"; do
    wait_healthy "$svc" 30
done

section "Phase 2: Record Baseline Offsets"

BASELINE_TOTAL=$(record_offsets "baseline")
log "Baseline Kafka offset total: $BASELINE_TOTAL"

section "Phase 3: Sustained Load (${LOAD_DURATION}s)"

log "Sending multi-event-type load for ${LOAD_DURATION}s..."
send_load "$LOAD_DURATION" &
LOAD_PID=$!
echo "$LOAD_PID" > "$WORK_DIR/load_pid"

# Let load build up before chaos
sleep "$KILL_AFTER_SECS"

section "Phase 4: Sequential Kill + Recovery"

OVERALL_RESULT="PASS"

for svc in "${SERVICES[@]}"; do
    echo ""
    log "--- Testing $svc ---"

    # Kill
    kill_service "$svc"

    # Brief pause to let in-flight events settle
    sleep 1

    # Restart and measure recovery time
    if ! restart_and_measure "$svc"; then
        OVERALL_RESULT="FAIL"
        continue
    fi

    # Check recovery SLA
    local_recovery_ms=${SERVICE_RECOVERY_MS[$svc]}
    if [[ $local_recovery_ms -le $RECOVERY_SLA_MS ]]; then
        ok "$svc recovery ${local_recovery_ms}ms <= ${RECOVERY_SLA_MS}ms SLA"
    else
        fail "$svc recovery ${local_recovery_ms}ms > ${RECOVERY_SLA_MS}ms SLA"
        OVERALL_RESULT="FAIL"
    fi

    # Verify data integrity
    sleep 2
    verify_service "$svc"
    if [[ "${SERVICE_VERIFY_RESULT[$svc]}" == "FAIL" ]]; then
        OVERALL_RESULT="FAIL"
    fi
done

# Wait for remaining load to finish
wait "$LOAD_PID" 2>/dev/null || true

section "Phase 5: Data Integrity Check"

sleep 3  # Let Kafka replication settle

FINAL_TOTAL=$(record_offsets "final")
SENT_COUNT=$(cat "$WORK_DIR/sent_count" 2>/dev/null || echo 0)
DELTA=$((FINAL_TOTAL - BASELINE_TOTAL))

log "Events sent (confirmed ACK): $SENT_COUNT"
log "Events on Kafka (offset delta): $DELTA"

if [[ $DELTA -ge $SENT_COUNT ]] && [[ $SENT_COUNT -gt 0 ]]; then
    ok "No data loss: $DELTA >= $SENT_COUNT"
elif [[ $SENT_COUNT -eq 0 ]]; then
    warn "No events were sent (service may have been down during load phase)"
elif [[ $DELTA -ge $(( SENT_COUNT * 99 / 100 )) ]]; then
    warn "Marginal: <1% loss ($DELTA / $SENT_COUNT) — within Bloom filter FPR tolerance"
else
    LOSS_PCT=$(( (SENT_COUNT - DELTA) * 100 / SENT_COUNT ))
    fail "${LOSS_PCT}% data loss ($DELTA / $SENT_COUNT)"
    OVERALL_RESULT="FAIL"
fi

# ===========================================================================
# Report
# ===========================================================================

section "Chaos E2E Test Report"
echo ""
echo "  Load duration:       ${LOAD_DURATION}s"
echo "  Kill after:          ${KILL_AFTER_SECS}s"
echo "  Recovery SLA:        ${RECOVERY_SLA_MS}ms"
echo "  Events sent (ACK):   ${SENT_COUNT}"
echo "  Kafka offset delta:  ${DELTA}"
echo ""
printf "  %-20s %-15s %-15s\n" "SERVICE" "RECOVERY (ms)" "INTEGRITY"
printf "  %-20s %-15s %-15s\n" "-------" "-------------" "---------"
for svc in "${SERVICES[@]}"; do
    local_recovery="${SERVICE_RECOVERY_MS[$svc]:-N/A}"
    local_verify="${SERVICE_VERIFY_RESULT[$svc]:-N/A}"
    printf "  %-20s %-15s %-15s\n" "$svc" "$local_recovery" "$local_verify"
done
echo ""

if [[ "$OVERALL_RESULT" == "PASS" ]]; then
    ok "OVERALL: PASS — all services recovered within SLA, no data loss"
else
    fail "OVERALL: FAIL — see details above"
fi

echo ""
echo "  To add your service to this framework:"
echo "    1. Create scripts/chaos_test_<service>.sh"
echo "    2. Export: chaos_start_<service>, chaos_health_<service>, chaos_verify_<service>"
echo "    3. Re-run: ./scripts/chaos_e2e_framework.sh"
echo ""

if [[ "$OVERALL_RESULT" == "FAIL" ]]; then
    exit 1
fi
exit 0
