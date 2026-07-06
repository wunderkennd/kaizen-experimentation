#!/usr/bin/env bash
# =============================================================================
# Throughput Test: M2 Pipeline — 100K events/sec via Redpanda (issue #502)
# =============================================================================
# Phase 4 Validation headline SLA: M2 sustains TARGET_EPS events/sec for
# DURATION with zero message loss and bounded downstream consumer lag.
#
# Measurement points (spec: docs/superpowers/specs/2026-04-20-multi-cloud-
# gcp-aws-design.md, Phase 4):
#   1. Ingest acceptance      — k6 batch-gRPC accounting (loadtest_m2_throughput.js)
#   2. Producer offset advance — rpk high-watermark sampling (m2_throughput_watch.py)
#   3. Downstream consumer lag — rpk group describe sampling
# Pass/fail gate: m2_throughput_watch.py evaluate. Exit 0 = PASS.
#
# Prerequisites: k6, rpk (>= 23.x), python3. The target M2 ingest endpoint
# and Redpanda brokers must be reachable — see docs/runbooks/
# m2-throughput-loadtest.md for the GCP dev-stack procedure and credentials.
#
# Usage (or: just loadtest-m2-throughput / loadtest-m2-throughput-smoke):
#   bash scripts/loadtest_m2_throughput.sh                    # full 100K x 5min
#   SMOKE=1 bash scripts/loadtest_m2_throughput.sh            # wiring check, low rate
#   TARGET_EPS=120000 DURATION=600 bash scripts/loadtest_m2_throughput.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# --- Configuration (env-overridable) ----------------------------------------
PIPELINE_ADDR="${PIPELINE_ADDR:-localhost:50052}"
PLAINTEXT="${PLAINTEXT:-true}"              # false for Cloud Run / TLS endpoints
export BROKERS="${BROKERS:-localhost:9092}" # consumed by m2_throughput_watch.py
TARGET_EPS="${TARGET_EPS:-100000}"
DURATION="${DURATION:-300}"                 # steady-state seconds (SLA: 5 min)
WARMUP="${WARMUP:-30}"                      # seconds excluded from the gate window
DRAIN_WAIT="${DRAIN_WAIT:-60}"              # post-load flush/lag-drain observation
BATCH_SIZE="${BATCH_SIZE:-100}"
SAMPLE_INTERVAL="${SAMPLE_INTERVAL:-5}"
LAG_THRESHOLD="${LAG_THRESHOLD:-100000}"    # parity: PipelineConsumerLag alert
BUCKET_FLOOR="${BUCKET_FLOOR:-0.95}"
TOPICS="${TOPICS:-exposures,metric_events,reward_events,qoe_events}"
CONSUMER_GROUPS="${CONSUMER_GROUPS:-bandit-policy-service}"
REQUIRE_GROUPS="${REQUIRE_GROUPS:-0}"       # 1 = absent consumer groups fail the gate

if [[ "${SMOKE:-0}" == "1" ]]; then
    # Wiring check, not the SLA: low rate, short windows, forgiving floor
    # (k6 init can eat into the 5s warmup at these timescales).
    TARGET_EPS="${SMOKE_TARGET_EPS:-2000}"
    DURATION=30; WARMUP=5; DRAIN_WAIT=15; SAMPLE_INTERVAL=2; BUCKET_FLOOR=0.50
fi

RUN_ID="m2t-$(date +%s)"
OUT_DIR="${OUT_DIR:-/tmp/${RUN_ID}}"
SAMPLES="$OUT_DIR/samples.jsonl"
K6_SUMMARY="$OUT_DIR/k6_summary.json"
REPORT="$OUT_DIR/gate_report.json"

BLUE='\033[0;34m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
log()  { echo -e "${BLUE}[m2-throughput]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

SAMPLER_PID=""
cleanup() {
    if [[ -n "$SAMPLER_PID" ]]; then
        kill "$SAMPLER_PID" 2>/dev/null || true
        wait "$SAMPLER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# --- Pre-flight ---------------------------------------------------------------
for tool in k6 rpk python3; do
    if ! command -v "$tool" &>/dev/null; then
        fail "$tool not found — see docs/runbooks/m2-throughput-loadtest.md"
        exit 1
    fi
done

mkdir -p "$OUT_DIR"
log "Preflight: sampling Redpanda ($BROKERS) once..."
if ! python3 "$SCRIPT_DIR/m2_throughput_watch.py" sample --once \
        --topics "$TOPICS" --groups "$CONSUMER_GROUPS"; then
    fail "Cannot read topic offsets from $BROKERS — check connectivity/credentials"
    exit 1
fi
ok "Redpanda reachable; topics visible"

# --- Run ----------------------------------------------------------------------
log "Starting offset/lag sampler (every ${SAMPLE_INTERVAL}s -> $SAMPLES)"
python3 "$SCRIPT_DIR/m2_throughput_watch.py" sample \
    --out "$SAMPLES" --interval "$SAMPLE_INTERVAL" \
    --topics "$TOPICS" --groups "$CONSUMER_GROUPS" &
SAMPLER_PID=$!
sleep "$SAMPLE_INTERVAL"  # guarantee a pre-traffic baseline sample

K6_DURATION=$(( WARMUP + DURATION ))
T_START=$(date +%s)
log "k6: ${TARGET_EPS} events/sec for ${K6_DURATION}s (${WARMUP}s warmup + ${DURATION}s steady) -> $PIPELINE_ADDR"

cd "$REPO_ROOT"  # k6 loads the gRPC schema from the repo-relative proto/ root
set +e
k6 run \
    --env "PIPELINE_ADDR=$PIPELINE_ADDR" \
    --env "TARGET_EPS=$TARGET_EPS" \
    --env "DURATION=${K6_DURATION}s" \
    --env "BATCH_SIZE=$BATCH_SIZE" \
    --env "RUN_ID=$RUN_ID" \
    --env "PLAINTEXT=$PLAINTEXT" \
    --env "K6_SUMMARY_PATH=$K6_SUMMARY" \
    "$SCRIPT_DIR/loadtest_m2_throughput.js"
K6_RC=$?
set -e
# Steady window ends when scheduled load stops — NOT at k6 process exit, which
# includes the gracefulStop tail where the offered rate ramps to zero.
T_STEADY_END=$(( T_START + WARMUP + DURATION ))
[[ $K6_RC -ne 0 ]] && log "k6 exited $K6_RC (thresholds?) — gate will judge from the summary"

log "Draining ${DRAIN_WAIT}s (in-flight publishes + consumer catch-up)..."
sleep "$DRAIN_WAIT"
cleanup; SAMPLER_PID=""

# --- Gate ---------------------------------------------------------------------
REQUIRE_FLAG=""
[[ "$REQUIRE_GROUPS" == "1" ]] && REQUIRE_FLAG="--require-groups"
set +e
# shellcheck disable=SC2086  # REQUIRE_FLAG is deliberately word-split (empty or one flag)
python3 "$SCRIPT_DIR/m2_throughput_watch.py" evaluate \
    --samples "$SAMPLES" \
    --k6-summary "$K6_SUMMARY" \
    --target-eps "$TARGET_EPS" \
    --steady-start "$(( T_START + WARMUP ))" \
    --steady-end "$T_STEADY_END" \
    --lag-threshold "$LAG_THRESHOLD" \
    --bucket-floor "$BUCKET_FLOOR" \
    --report "$REPORT" \
    $REQUIRE_FLAG
GATE_RC=$?
set -e

log "Artifacts: $OUT_DIR (samples, k6 summary, gate report)"
if [[ $GATE_RC -eq 0 ]]; then
    ok "GATE PASS — ${TARGET_EPS} events/sec sustained, zero loss, lag bounded"
else
    fail "GATE FAIL — see report above and $REPORT"
fi
exit $GATE_RC
