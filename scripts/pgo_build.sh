#!/usr/bin/env bash
# =============================================================================
# PGO Build: Profile-Guided Optimization for M1 Assignment Service
# =============================================================================
# 3-phase build pipeline:
#   Phase 1: INSTRUMENT — build with -Cprofile-generate
#   Phase 2: PROFILE   — run realistic workload to collect .profraw data
#   Phase 3: OPTIMIZE  — build with -Cprofile-use for optimized binary
#
# Prerequisites:
#   - rustup component add llvm-tools (in rust-toolchain.toml)
#   - grpcurl installed (brew install grpcurl)
#   - dev/config.json present
#
# Usage:
#   ./scripts/pgo_build.sh
#   PGO_PORT=50099 ./scripts/pgo_build.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PGO_DIR="${PGO_DIR:-/tmp/pgo-data}"
PGO_PORT="${PGO_PORT:-50099}"
CONFIG_PATH="$REPO_ROOT/dev/config.json"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

log()  { echo -e "${BLUE}[pgo-build]${NC} $*"; }
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
    cargo build --release --package experimentation-assignment
    ok "Standard release build complete (no PGO): target/release/experimentation-assignment"
    exit 0
fi

if ! command -v grpcurl &>/dev/null; then
    fail "grpcurl not found. Install: brew install grpcurl"
fi

if [[ ! -f "$CONFIG_PATH" ]]; then
    fail "Config file not found: $CONFIG_PATH"
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
    cargo build --release --package experimentation-assignment 2>&1

INSTRUMENTED_BIN="$REPO_ROOT/target/release/experimentation-assignment"
if [[ ! -f "$INSTRUMENTED_BIN" ]]; then
    fail "Instrumented binary not found at $INSTRUMENTED_BIN"
fi
ok "Instrumented binary built"

# ---------------------------------------------------------------------------
# Phase 2: PROFILE — run workload to collect .profraw files
# ---------------------------------------------------------------------------
log "Phase 2/3: Collecting profile data via workload..."

# Start the instrumented server
CONFIG_PATH="$CONFIG_PATH" \
GRPC_ADDR="0.0.0.0:${PGO_PORT}" \
RUST_LOG=warn \
LLVM_PROFILE_FILE="$PGO_DIR/default_%m_%p.profraw" \
    "$INSTRUMENTED_BIN" &
SERVER_PID=$!

cleanup_server() {
    if kill -0 "$SERVER_PID" 2>/dev/null; then
        # SIGTERM lets the process flush .profraw files
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
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
PGO_PORT="$PGO_PORT" bash "$SCRIPT_DIR/pgo_workload.sh"

# Stop the server gracefully to flush profile data
log "Stopping instrumented server (flushing profile data)..."
kill "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true
trap - EXIT

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
    cargo build --release --package experimentation-assignment 2>&1

ok "PGO-optimized binary: target/release/experimentation-assignment"

# Report sizes
BINARY_SIZE=$(du -h "$REPO_ROOT/target/release/experimentation-assignment" | cut -f1)
log "Binary size: $BINARY_SIZE"

echo ""
echo "============================================================="
echo "  PGO BUILD COMPLETE"
echo "============================================================="
echo "  Binary:   target/release/experimentation-assignment"
echo "  Size:     $BINARY_SIZE"
echo "  Profiles: $PROFRAW_COUNT .profraw files merged"
echo "  Data:     $PGO_DIR/merged.profdata"
echo ""
echo "  Next steps:"
echo "    cargo bench -p experimentation-assignment   # validate perf"
echo "    bash scripts/validate_assignment_p99.sh     # check SLA"
echo "============================================================="
