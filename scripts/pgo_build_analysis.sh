#!/usr/bin/env bash
# =============================================================================
# PGO Build: Profile-Guided Optimization for M4a Analysis Service
# =============================================================================
# 3-phase build pipeline:
#   Phase 1: INSTRUMENT — build with -Cprofile-generate
#   Phase 2: PROFILE   — run realistic workload to collect .profraw data
#   Phase 3: OPTIMIZE  — build with -Cprofile-use for optimized binary
#
# Prerequisites:
#   - rustup component add llvm-tools (in rust-toolchain.toml)
#   - grpcurl installed (brew install grpcurl)
#   - python3 with pyarrow (pip install pyarrow)
#
# Usage:
#   ./scripts/pgo_build_analysis.sh
#   PGO_PORT=50097 ./scripts/pgo_build_analysis.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PGO_DIR="${PGO_DIR:-/tmp/pgo-data-analysis}"
PGO_PORT="${PGO_PORT:-50097}"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

log()  { echo -e "${BLUE}[pgo-build-analysis]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; exit 1; }

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

# Find llvm-profdata in rustup sysroot
LLVM_PROFDATA=""
SYSROOT=$(rustc --print sysroot 2>/dev/null || true)
if [[ -n "$SYSROOT" ]]; then
    CANDIDATE=$(find "$SYSROOT" -name "llvm-profdata" -type f 2>/dev/null | head -1)
    if [[ -n "$CANDIDATE" ]]; then
        LLVM_PROFDATA="$CANDIDATE"
    fi
fi

if [[ -z "$LLVM_PROFDATA" ]]; then
    log "llvm-profdata not found. Install with: rustup component add llvm-tools"
    log "Falling back to standard release build (no PGO)."
    cd "$REPO_ROOT"
    cargo build --release --package experimentation-analysis
    ok "Standard release build complete (no PGO): target/release/experimentation-analysis"
    exit 0
fi

if ! command -v grpcurl &>/dev/null; then
    fail "grpcurl not found. Install: brew install grpcurl"
fi

log "Using llvm-profdata: $LLVM_PROFDATA"

# ---------------------------------------------------------------------------
# Phase 1: INSTRUMENT
# ---------------------------------------------------------------------------
log "Phase 1/3: Building instrumented binary..."
rm -rf "$PGO_DIR"
mkdir -p "$PGO_DIR"

cd "$REPO_ROOT"
RUSTFLAGS="-Cprofile-generate=$PGO_DIR" \
    cargo build --release --package experimentation-analysis 2>&1

INSTRUMENTED_BIN="$REPO_ROOT/target/release/experimentation-analysis"
if [[ ! -f "$INSTRUMENTED_BIN" ]]; then
    fail "Instrumented binary not found at $INSTRUMENTED_BIN"
fi
ok "Instrumented binary built"

# ---------------------------------------------------------------------------
# Phase 2: PROFILE — run workload to collect .profraw files
# ---------------------------------------------------------------------------
log "Phase 2/3: Collecting profile data via workload..."

# Generate synthetic Delta Lake data for profiling
TEMP_DELTA=$(mktemp -d)
log "Generating synthetic Delta Lake data in $TEMP_DELTA..."

if command -v python3 &>/dev/null; then
    python3 "$SCRIPT_DIR/generate_synthetic_delta.py" --output "$TEMP_DELTA" 2>&1 || {
        log "Python synthetic data generation failed, creating minimal test data..."
        mkdir -p "$TEMP_DELTA/metric_summaries" "$TEMP_DELTA/interleaving_scores"
    }
else
    log "python3 not found, creating empty Delta Lake directories..."
    mkdir -p "$TEMP_DELTA/metric_summaries" "$TEMP_DELTA/interleaving_scores"
fi

# Start the instrumented server
# DATABASE_URL not set — PostgreSQL cache disabled (degrades gracefully)
ANALYSIS_GRPC_ADDR="0.0.0.0:${PGO_PORT}" \
DELTA_LAKE_PATH="$TEMP_DELTA" \
RUST_LOG=warn \
LLVM_PROFILE_FILE="$PGO_DIR/default_%m_%p.profraw" \
    "$INSTRUMENTED_BIN" &
SERVER_PID=$!

cleanup_server() {
    if kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$TEMP_DELTA"
}
trap cleanup_server EXIT

# Wait for server to be ready
log "Waiting for instrumented server (port $PGO_PORT)..."
for i in $(seq 1 30); do
    if grpcurl -plaintext "localhost:${PGO_PORT}" list >/dev/null 2>&1; then
        ok "Instrumented server ready (PID=$SERVER_PID)"
        break
    fi
    if [[ $i -eq 30 ]]; then
        fail "Instrumented server failed to start within 30s"
    fi
    sleep 1
done

# Run the profiling workload
PGO_PORT="$PGO_PORT" bash "$SCRIPT_DIR/pgo_workload_analysis.sh"

# Stop the server gracefully to flush profile data
log "Stopping instrumented server (flushing profile data)..."
kill "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true
trap - EXIT
rm -rf "$TEMP_DELTA"

# Verify .profraw files were generated
PROFRAW_COUNT=$(find "$PGO_DIR" -name "*.profraw" 2>/dev/null | wc -l | tr -d ' ')
if [[ "$PROFRAW_COUNT" -eq 0 ]]; then
    fail "No .profraw files generated in $PGO_DIR"
fi
ok "Collected $PROFRAW_COUNT profile data files"

# Merge profiles
log "Merging profile data..."
"$LLVM_PROFDATA" merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw
ok "Merged profile: $PGO_DIR/merged.profdata ($(du -h "$PGO_DIR/merged.profdata" | cut -f1))"

# ---------------------------------------------------------------------------
# Phase 3: OPTIMIZE — build with profile data
# ---------------------------------------------------------------------------
log "Phase 3/3: Building PGO-optimized binary..."

cd "$REPO_ROOT"
RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
    cargo build --release --package experimentation-analysis 2>&1

ok "PGO-optimized binary: target/release/experimentation-analysis"

# Report sizes
BINARY_SIZE=$(du -h "$REPO_ROOT/target/release/experimentation-analysis" | cut -f1)
log "Binary size: $BINARY_SIZE"

echo ""
echo "============================================================="
echo "  PGO BUILD COMPLETE: M4a Analysis Service"
echo "============================================================="
echo "  Binary:   target/release/experimentation-analysis"
echo "  Size:     $BINARY_SIZE"
echo "  Profiles: $PROFRAW_COUNT .profraw files merged"
echo "  Data:     $PGO_DIR/merged.profdata"
echo ""
echo "  Next steps:"
echo "    cargo bench -p experimentation-stats         # validate perf"
echo "    bash scripts/validate_policy_p99.sh          # check SLA"
echo "============================================================="
