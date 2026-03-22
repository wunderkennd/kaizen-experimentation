#!/usr/bin/env bash
# =============================================================================
# Load Test: M2 Event Pipeline — p99 < 10ms at configurable rps
# =============================================================================
# SLA validation. Builds the release binary, starts the server with Kafka,
# runs k6 gRPC load at TARGET_RPS for DURATION, and validates:
#   - IngestExposure p99 < 10ms
#   - IngestMetricEvent p99 < 10ms
#   - Error rate < 0.1%
#
# Requires Docker Kafka running (unlike PGO build which uses unreachable Kafka).
# For best results, use a PGO-optimized binary:
#   just pgo-build-pipeline && TARGET_RPS=50000 ./scripts/loadtest_pipeline.sh
#
# Prerequisites:
#   - k6 installed (brew install k6)
#   - Rust toolchain (cargo build)
#   - Docker Kafka running (just infra)
#
# Usage:
#   ./scripts/loadtest_pipeline.sh
#   TARGET_RPS=50000 ./scripts/loadtest_pipeline.sh
#   DURATION=30s ./scripts/loadtest_pipeline.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PORT="${PIPELINE_PORT:-50052}"
METRICS_PORT="${PIPELINE_METRICS_PORT:-9091}"
TARGET_RPS="${TARGET_RPS:-10000}"
DURATION="${DURATION:-60s}"
KAFKA_BROKERS="${KAFKA_BROKERS:-localhost:9092}"
BUFFER_DIR="${BUFFER_DIR:-/tmp/loadtest-pipeline-buffer}"
# Prefer PGO-optimized binary if available
PIPELINE_BIN="$REPO_ROOT/target/release/experimentation-pipeline"
RESULTS_FILE="$REPO_ROOT/loadtest_pipeline_results.json"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${BLUE}[loadtest-m2]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

PIPELINE_PID=""
cleanup() {
    if [[ -n "$PIPELINE_PID" ]]; then
        kill "$PIPELINE_PID" 2>/dev/null || true
        wait "$PIPELINE_PID" 2>/dev/null || true
    fi
    rm -rf "$BUFFER_DIR"
    rm -f "$RESULTS_FILE"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

if ! command -v k6 &>/dev/null; then
    fail "k6 not found. Install: brew install k6"
    exit 1
fi

# Check Kafka is running (required for pipeline load test)
if ! docker compose ps kafka 2>/dev/null | grep -q "running"; then
    warn "Kafka not detected. Pipeline needs Kafka for event publishing."
    warn "Start with: just infra"
    warn "Continuing anyway — events will go to disk buffer."
fi

# ---------------------------------------------------------------------------
# Build release binary
# ---------------------------------------------------------------------------

if [[ ! -f "$PIPELINE_BIN" ]]; then
    log "Building pipeline binary (release mode)..."
    cd "$REPO_ROOT"
    cargo build --release --package experimentation-pipeline 2>&1 | tail -3
fi
ok "Release binary ready: $PIPELINE_BIN"

if [[ "$TARGET_RPS" -ge 50000 ]]; then
    warn "High RPS target ($TARGET_RPS). For best results, build with PGO: just pgo-build-pipeline"
fi

# ---------------------------------------------------------------------------
# Start pipeline server
# ---------------------------------------------------------------------------

mkdir -p "$BUFFER_DIR"

log "Starting pipeline service on port $PORT (metrics: $METRICS_PORT)..."
PORT="$PORT" \
METRICS_PORT="$METRICS_PORT" \
KAFKA_BROKERS="$KAFKA_BROKERS" \
BLOOM_EXPECTED_DAILY="10000000" \
BLOOM_FP_RATE="0.001" \
BUFFER_DIR="$BUFFER_DIR" \
RUST_LOG=warn \
    "$PIPELINE_BIN" &
PIPELINE_PID=$!

# Wait for HTTP health check on metrics port
for i in $(seq 1 30); do
    if curl -sf "http://localhost:${METRICS_PORT}/healthz" >/dev/null 2>&1; then
        ok "Pipeline service ready (PID=$PIPELINE_PID)"
        break
    fi
    if [[ $i -eq 30 ]]; then
        fail "Pipeline service failed to start within 30s"
        exit 1
    fi
    sleep 1
done

# ---------------------------------------------------------------------------
# Run k6 load test
# ---------------------------------------------------------------------------

log "Running k6 load test: ${TARGET_RPS} rps for ${DURATION}..."
echo ""

cd "$REPO_ROOT"
k6 run \
    --summary-trend-stats="p(50),p(90),p(95),p(99),p(99.9)" \
    --env "PIPELINE_ADDR=localhost:${PORT}" \
    --env "TARGET_RPS=${TARGET_RPS}" \
    --env "DURATION=${DURATION}" \
    scripts/loadtest_pipeline.js \
    2>&1

echo ""

# ---------------------------------------------------------------------------
# Validate results
# ---------------------------------------------------------------------------

if [[ -f "$RESULTS_FILE" ]]; then
    ALL_PASS=$(python3 -c "
import json, sys
with open('$RESULTS_FILE') as f:
    r = json.load(f)
print('true' if r.get('all_pass', False) else 'false')
print(f\"  IngestExposure p99:     {r.get('exposure_p99_ms', 'N/A')} ms\")
print(f\"  IngestMetricEvent p99:  {r.get('metric_p99_ms', 'N/A')} ms\")
print(f\"  Total rps achieved:     {r.get('total_rps', 0):.0f}\")
print(f\"  Error rate:             {r.get('error_rate', 0)*100:.3f}%\")
" 2>/dev/null || echo "false")

    PASS_LINE=$(echo "$ALL_PASS" | head -1)
    echo "$ALL_PASS" | tail -n +2

    echo ""
    if [[ "$PASS_LINE" == "true" ]]; then
        ok "ALL SLAs MET — p99 < 10ms at ${TARGET_RPS} rps"
        exit 0
    else
        fail "SLA VIOLATION — see report above"
        exit 1
    fi
else
    warn "No results file found — k6 may have reported inline"
    # k6 thresholds cause exit code 99 on failure
    exit 0
fi
