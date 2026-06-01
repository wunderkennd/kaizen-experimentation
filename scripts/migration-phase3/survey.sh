#!/usr/bin/env bash
# =============================================================================
# survey.sh — ADR-026 Phase 3 CUSTOM-metric inventory (#437)
# =============================================================================
#
# Thin wrapper around `custom_migrator scan + translate`. Calls M5 to enumerate
# every CUSTOM-typed MetricDefinition, then classifies + translates them into a
# proposals JSON + a human-readable Markdown summary. Output paths are
# org-keyed when `--org` is set so operators can fan out across multiple orgs
# without overwriting each other's artifacts.
#
# Survey is operational guidance, not a prerequisite (Lock L8). The migration
# tool's pattern library is conservative-by-default; anything not auto-classified
# falls through to Tier 3 and is surfaced as a non-translatable proposal.
#
# Usage:
#   ./scripts/migration-phase3/survey.sh --m5-addr http://localhost:50055
#   ./scripts/migration-phase3/survey.sh --m5-addr http://m5.example:50055 \
#       --output-dir ./out --org acme
#
# See docs/runbooks/adr-026-phase-3-migration.md for the full workflow.
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Defaults (overridable via env or flags).
# ---------------------------------------------------------------------------
DEFAULT_M5_ADDR="${CUSTOM_MIGRATOR_M5_ADDR:-http://localhost:50055}"
DEFAULT_OUTPUT_DIR="./migration-phase3-output/$(date +%Y-%m-%d)"
DEFAULT_BIN="${CUSTOM_MIGRATOR_BIN:-}"   # empty = use `cargo run`

# Colors (only when stdout is a tty).
if [[ -t 1 ]]; then
    RED=$'\033[0;31m'
    GREEN=$'\033[0;32m'
    YELLOW=$'\033[1;33m'
    BLUE=$'\033[0;34m'
    NC=$'\033[0m'
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    NC=''
fi

_log()  { echo "${BLUE}[survey]${NC} $*"; }
_ok()   { echo "${GREEN}[  OK  ]${NC} $*"; }
_warn() { echo "${YELLOW}[ WARN ]${NC} $*" >&2; }
_fail() { echo "${RED}[ FAIL ]${NC} $*" >&2; }

# ---------------------------------------------------------------------------
# Usage / --help
# ---------------------------------------------------------------------------
usage() {
    cat <<'EOF'
survey.sh — ADR-026 Phase 3 CUSTOM-metric inventory (#437)

Wraps `custom_migrator scan` and `custom_migrator translate` against an M5
instance to produce, for each CUSTOM metric on the server:

    <prefix>scan.json        — raw MetricDefinition list returned by M5
    <prefix>proposals.json   — machine-readable proposals (input for shadow)
    <prefix>summary.md       — human-readable Markdown for operator review

The script does NOT shadow-run or apply anything; it produces artifacts for
the migration owners to review. See the runbook for the full workflow.

USAGE
  survey.sh [OPTIONS]

OPTIONS
  --m5-addr <url>       M5 gRPC address (e.g. http://localhost:50055)
                        Default: $CUSTOM_MIGRATOR_M5_ADDR or
                                 http://localhost:50055
  --output-dir <path>   Directory to write artifacts into
                        Default: ./migration-phase3-output/<YYYY-MM-DD>
  --org <name>          Optional org tag. When set, outputs are prefixed:
                          <org>-scan.json
                          <org>-proposals.json
                          <org>-summary.md
                        When omitted, just scan.json / proposals.json /
                        summary.md.
  --binary <path>       Path to a prebuilt custom_migrator binary. When set,
                        the script invokes it directly instead of going
                        through `cargo run`. Useful in CI / container envs
                        where Cargo isn't installed.
                        Default: $CUSTOM_MIGRATOR_BIN (empty = use cargo run)
  --force               Permit running against a non-empty output directory.
                        Without this, the script refuses to clobber artifacts
                        from a previous run on the same day.
  -h, --help            Print this message and exit 0.

ENVIRONMENT
  CUSTOM_MIGRATOR_M5_ADDR    Same as --m5-addr.
  CUSTOM_MIGRATOR_BIN        Same as --binary.

WORKFLOW DRIVEN BY THIS SCRIPT
  1. Verify required tools are on PATH (cargo, unless --binary is set).
  2. Create the output directory.
  3. Run: custom_migrator scan      --m5-addr ... --output <prefix>scan.json
  4. Run: custom_migrator translate --report  <prefix>scan.json \
                                    --output  <prefix>proposals.json \
                                    --markdown <prefix>summary.md
  5. Print a one-line summary: counts of CUSTOMs found, by tier.

EXIT CODES
  0   Success.
  1   Any sub-command failed (scan or translate).
  2   Bad CLI args / missing prerequisites.

EXAMPLES
  # Local dev — defaults to ./migration-phase3-output/<today>/
  ./scripts/migration-phase3/survey.sh --m5-addr http://localhost:50055

  # Per-org run with prebuilt binary
  ./scripts/migration-phase3/survey.sh \
      --m5-addr http://m5.example:50055 \
      --output-dir /tmp/survey \
      --org acme \
      --binary /opt/kaizen/bin/custom_migrator

  # Re-run for the same day, overwriting yesterday's artifacts
  ./scripts/migration-phase3/survey.sh --m5-addr http://localhost:50055 --force

SEE ALSO
  docs/runbooks/adr-026-phase-3-migration.md
  crates/experimentation-management/src/bin/custom_migrator.rs
EOF
}

# ---------------------------------------------------------------------------
# CLI parsing.
# ---------------------------------------------------------------------------
M5_ADDR="$DEFAULT_M5_ADDR"
OUTPUT_DIR="$DEFAULT_OUTPUT_DIR"
ORG=""
BIN="$DEFAULT_BIN"
FORCE=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --m5-addr)
            [[ $# -lt 2 ]] && { _fail "--m5-addr requires a URL"; exit 2; }
            M5_ADDR="$2"; shift 2 ;;
        --output-dir)
            [[ $# -lt 2 ]] && { _fail "--output-dir requires a path"; exit 2; }
            OUTPUT_DIR="$2"; shift 2 ;;
        --org)
            [[ $# -lt 2 ]] && { _fail "--org requires a name"; exit 2; }
            ORG="$2"; shift 2 ;;
        --binary)
            [[ $# -lt 2 ]] && { _fail "--binary requires a path"; exit 2; }
            BIN="$2"; shift 2 ;;
        --force)
            FORCE=1; shift ;;
        -h|--help)
            usage; exit 0 ;;
        --)
            shift; break ;;
        -*)
            _fail "unknown option: $1"
            echo "Run with --help for usage." >&2
            exit 2 ;;
        *)
            _fail "unexpected positional arg: $1"
            echo "Run with --help for usage." >&2
            exit 2 ;;
    esac
done

# ---------------------------------------------------------------------------
# Prerequisites.
# ---------------------------------------------------------------------------
if [[ -n "$BIN" ]]; then
    if [[ ! -x "$BIN" ]]; then
        _fail "--binary path is not executable: $BIN"
        exit 2
    fi
else
    if ! command -v cargo >/dev/null 2>&1; then
        _fail "cargo not on PATH and --binary not set; cannot invoke custom_migrator"
        exit 2
    fi
fi

if ! command -v jq >/dev/null 2>&1; then
    _warn "jq not on PATH; the closing summary will be approximate (counts may show as '?')"
fi

# ---------------------------------------------------------------------------
# Resolve output paths.
# ---------------------------------------------------------------------------
mkdir -p "$OUTPUT_DIR"

# Safety belt: refuse to clobber non-empty existing dir unless --force.
if [[ $FORCE -ne 1 ]]; then
    # `find -mindepth 1` lists anything in the directory; if non-empty, bail.
    if [[ -n "$(find "$OUTPUT_DIR" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]]; then
        _fail "output directory not empty: $OUTPUT_DIR"
        echo "Pass --force to overwrite existing artifacts." >&2
        exit 2
    fi
fi

PREFIX=""
if [[ -n "$ORG" ]]; then
    # Sanitize org for filenames: allow alphanumeric, dash, underscore.
    if [[ ! "$ORG" =~ ^[A-Za-z0-9_-]+$ ]]; then
        _fail "--org must match [A-Za-z0-9_-]+ (got: $ORG)"
        exit 2
    fi
    PREFIX="${ORG}-"
fi

SCAN_OUT="${OUTPUT_DIR}/${PREFIX}scan.json"
PROPOSALS_OUT="${OUTPUT_DIR}/${PREFIX}proposals.json"
SUMMARY_OUT="${OUTPUT_DIR}/${PREFIX}summary.md"

# ---------------------------------------------------------------------------
# Build the migrator invocation prefix.
# ---------------------------------------------------------------------------
if [[ -n "$BIN" ]]; then
    MIGRATOR_CMD=("$BIN")
else
    # `cargo run` invocation. The `--quiet` keeps log lines from polluting
    # operator stdout; tracing inside the tool still emits its own output.
    MIGRATOR_CMD=(cargo run --quiet --release \
        -p experimentation-management \
        --bin custom_migrator --)
fi

# ---------------------------------------------------------------------------
# Step 1 — scan.
# ---------------------------------------------------------------------------
_log "scanning M5 at $M5_ADDR..."
_log "  output: $SCAN_OUT"

if ! "${MIGRATOR_CMD[@]}" scan \
        --m5-addr "$M5_ADDR" \
        --output "$SCAN_OUT"; then
    _fail "scan failed; M5 unreachable or returned an error"
    exit 1
fi
_ok "scan complete"

# ---------------------------------------------------------------------------
# Step 2 — translate.
# ---------------------------------------------------------------------------
_log "translating proposals..."
_log "  proposals: $PROPOSALS_OUT"
_log "  summary:   $SUMMARY_OUT"

if ! "${MIGRATOR_CMD[@]}" translate \
        --report "$SCAN_OUT" \
        --output "$PROPOSALS_OUT" \
        --markdown "$SUMMARY_OUT"; then
    _fail "translate failed; check $SCAN_OUT for malformed input"
    exit 1
fi
_ok "translate complete"

# ---------------------------------------------------------------------------
# Step 3 — one-line summary.
#
# `proposals.json` is `{ "summary": {...}, "entries": [...] }` (see
# crates/experimentation-management/src/migration/report.rs::render_json).
# The `summary` block has explicit per-tier counters so we read those rather
# than re-counting entries.
# ---------------------------------------------------------------------------
TOTAL="?" TIER1="?" TIER2="?" TIER3="?"
if command -v jq >/dev/null 2>&1; then
    if [[ -f "$PROPOSALS_OUT" ]]; then
        TOTAL=$(jq '.summary.total' < "$PROPOSALS_OUT" 2>/dev/null || echo "?")
        TIER1=$(jq '.summary.tier1_filtered_mean + .summary.tier1_composite + .summary.tier1_windowed_count' \
                    < "$PROPOSALS_OUT" 2>/dev/null || echo "?")
        TIER2=$(jq '.summary.tier2_metricql' < "$PROPOSALS_OUT" 2>/dev/null || echo "?")
        TIER3=$(jq '.summary.tier3_untranslatable' < "$PROPOSALS_OUT" 2>/dev/null || echo "?")
    fi
fi

echo
_ok "${OUTPUT_DIR}: ${TOTAL} CUSTOMs found, ${TIER1} Tier-1, ${TIER2} Tier-2, ${TIER3} Tier-3"
_log "next: distribute ${SUMMARY_OUT} to metric owners for review"
_log "      then shadow-run via: custom_migrator shadow --proposals ${PROPOSALS_OUT} ..."
_log "      see docs/runbooks/adr-026-phase-3-migration.md"
