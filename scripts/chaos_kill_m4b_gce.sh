#!/usr/bin/env bash
# =============================================================================
# Chaos Test: Kill M4b GCE Instance — MIG Autohealing Recovery (GCP)
# =============================================================================
# Phase 4 GCP validation for the M4b Policy Service crash-recovery invariant
# (docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md, Compute
# Model → M4b Policy Service; issue #501). Cloud-level analog of the
# process-level kill -9 test in scripts/chaos_kill_policy.sh.
#
# Verifies, against the stack provisioned by infra/pkg/gcp/compute/m4b.go:
#   1. Resolve the stateful MIG topology (instance kaizen-<env>-m4b-0,
#      MIG kaizen-<env>-m4b-mig, static IP kaizen-<env>-m4b-ip, port 50054)
#   2. Build pre-crash RocksDB state (CreateColdStartBandit + SelectArm
#      warm-up) and record a state sample (SelectArm + GetPolicySnapshot)
#   3. Forcefully terminate the instance:
#        --kill-mode delete  gcloud compute instances delete (default) — the
#                            MIG recreates the instance from its per-instance
#                            config and reattaches the stateful disk + IP
#        --kill-mode panic   sysrq-trigger kernel panic over IAP SSH — the
#                            instance reboots in place (automaticRestart)
#   4. Probe the service endpoint until it serves again. M4b is gRPC, so
#      "returns 200" means: TCP connect succeeds AND a SelectArm probe gets a
#      well-formed gRPC response (an arm, or NotFound for the sentinel id)
#   5. Measure the outage window (first failed probe → first serving probe)
#      and assert it is < RECOVERY_SLA_MS (default 10000)
#   6. Verify RocksDB state survived: the pre-crash experiment still resolves
#      (SelectArm returns an arm — NOT NotFound), GetPolicySnapshot still
#      answers, and the stateful disk (device m4b-data) is attached to the
#      recreated instance
#   7. Repeat --runs N times (issue #501 acceptance: 3 consecutive runs < 10s)
#
# Measurement note: probes poll every PROBE_INTERVAL (0.2s) with a 1s connect
# timeout, so the reported outage is biased UPWARD by at most ~1.4s — a pass
# is conservative evidence for the < 10s SLO.
#
# Prerequisites:
#   - gcloud authenticated with compute.instanceAdmin.v1 on the target project
#     (plus IAP tunnel + OS Login roles for --kill-mode panic)
#   - grpcurl on PATH (server reflection is enabled on M4b)
#   - Network reach to the M4b endpoint (private subnet IP): run from inside
#     the VPC, over VPN/Interconnect, or pass --endpoint to a forwarded port.
#     See docs/runbooks/m4b-gce-chaos.md for reachability patterns.
#
# Usage:
#   ./scripts/chaos_kill_m4b_gce.sh --dry-run
#   ./scripts/chaos_kill_m4b_gce.sh --project my-gcp-project --env dev
#   ./scripts/chaos_kill_m4b_gce.sh --runs 3                  # acceptance run
#   ./scripts/chaos_kill_m4b_gce.sh --kill-mode panic --zone us-central1-a
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration (env var defaults; flags override)
# ---------------------------------------------------------------------------
KAIZEN_ENV=${KAIZEN_ENV:-dev}
GCP_PROJECT=${GCP_PROJECT:-}
ZONE=${ZONE:-}
ENDPOINT=${ENDPOINT:-}
EXPERIMENT_ID=${EXPERIMENT_ID:-}
RUNS=${RUNS:-1}
RECOVERY_SLA_MS=${RECOVERY_SLA_MS:-10000}
RECOVERY_TIMEOUT_SECS=${RECOVERY_TIMEOUT_SECS:-600}
DOWN_TIMEOUT_SECS=${DOWN_TIMEOUT_SECS:-180}
PROBE_INTERVAL=${PROBE_INTERVAL:-0.2}
WARMUP_CALLS=${WARMUP_CALLS:-25}
WARMUP_SETTLE_SECS=${WARMUP_SETTLE_SECS:-6}
INTER_RUN_SETTLE_SECS=${INTER_RUN_SETTLE_SECS:-30}
KILL_MODE=${KILL_MODE:-delete}
M4B_PORT=${M4B_PORT:-50054}
ALLOW_PROD=${ALLOW_PROD:-false}
DRY_RUN=false

SVC_PATH="experimentation.bandit.v1.BanditPolicyService"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[chaos-m4b-gce]${NC} $*"; }
ok()   { echo -e "${GREEN}[  OK  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ WARN ]${NC} $*"; }
fail() { echo -e "${RED}[ FAIL ]${NC} $*"; }

WORK_DIR=$(mktemp -d)
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --env)              KAIZEN_ENV="$2"; shift 2 ;;
        --project)          GCP_PROJECT="$2"; shift 2 ;;
        --zone)             ZONE="$2"; shift 2 ;;
        --endpoint)         ENDPOINT="$2"; shift 2 ;;
        --experiment-id)    EXPERIMENT_ID="$2"; shift 2 ;;
        --runs)             RUNS="$2"; shift 2 ;;
        --recovery-sla)     RECOVERY_SLA_MS="$2"; shift 2 ;;
        --recovery-timeout) RECOVERY_TIMEOUT_SECS="$2"; shift 2 ;;
        --kill-mode)        KILL_MODE="$2"; shift 2 ;;
        --allow-prod)       ALLOW_PROD=true; shift ;;
        --dry-run)          DRY_RUN=true; shift ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --env ENV               Kaizen environment: dev|staging|prod (default: dev)"
            echo "  --project PROJECT       GCP project (default: gcloud config get-value project)"
            echo "  --zone ZONE             GCE zone (default: discovered from the instance)"
            echo "  --endpoint HOST:PORT    M4b endpoint (default: static IP kaizen-<env>-m4b-ip:${M4B_PORT})"
            echo "  --experiment-id ID      Reuse an existing bandit experiment for the state check"
            echo "  --runs N                Consecutive kill/recover cycles (default: 1; acceptance: 3)"
            echo "  --recovery-sla MS       Max outage window in ms (default: 10000)"
            echo "  --recovery-timeout SECS Give up waiting for recovery after N secs (default: 600)"
            echo "  --kill-mode MODE        delete (MIG recreate) | panic (sysrq kernel panic) (default: delete)"
            echo "  --allow-prod            Required to target --env prod"
            echo "  --dry-run               Preflight + plan only; kill nothing"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

PREFIX="kaizen-${KAIZEN_ENV}"
INSTANCE="${PREFIX}-m4b-0"
MIG="${PREFIX}-m4b-mig"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
now_ms() {
    local ns
    ns=$(date +%s%N 2>/dev/null || true)
    if [[ -z "$ns" || "$ns" == *N* ]]; then
        python3 -c 'import time; print(int(time.time()*1000))'
    else
        echo $(( ns / 1000000 ))
    fi
}

gc() { gcloud --project "$GCP_PROJECT" "$@"; }

instance_id() {
    gc compute instances describe "$INSTANCE" --zone "$ZONE" \
        --format='value(id)' 2>/dev/null || true
}

grpc_call() {
    local rpc="$1" payload="$2" maxt="${3:-10}"
    grpcurl -plaintext -connect-timeout 2 -max-time "$maxt" -d "$payload" \
        "$ENDPOINT" "${SVC_PATH}/${rpc}" 2>&1
}

tcp_open() {
    timeout 1 bash -c ">/dev/tcp/${1}/${2}" 2>/dev/null
}

# gRPC analog of "endpoint returns 200": TCP connect + well-formed SelectArm
# response (NotFound for the sentinel id, or an arm) proves the service is up.
probe_serving() {
    local host="${ENDPOINT%:*}" port="${ENDPOINT##*:}" out
    tcp_open "$host" "$port" || return 1
    out=$(grpcurl -plaintext -connect-timeout 1 -max-time 2 \
        -d '{"experiment_id":"chaos-gce-liveness-probe","user_id":"probe"}' \
        "$ENDPOINT" "${SVC_PATH}/SelectArm" 2>&1) || true
    grep -qE "NotFound|not found|armId|arm_id" <<<"$out"
}

extract_experiment_id() {
    local body="$1" id
    id=$(grep -o '"experimentId"[[:space:]]*:[[:space:]]*"[^"]*"' <<<"$body" | head -1 | sed 's/.*"experimentId"[[:space:]]*:[[:space:]]*"//;s/"//')
    if [[ -z "$id" ]]; then
        id=$(grep -o '"experiment_id"[[:space:]]*:[[:space:]]*"[^"]*"' <<<"$body" | head -1 | sed 's/.*"experiment_id"[[:space:]]*:[[:space:]]*"//;s/"//')
    fi
    echo "$id"
}

wait_mig_stable() {
    log "Waiting for MIG ${MIG} to stabilize..."
    if gc compute instance-groups managed wait-until --stable "$MIG" \
        --zone "$ZONE" --timeout "$RECOVERY_TIMEOUT_SECS" >/dev/null 2>&1; then
        ok "MIG stable"
    else
        warn "MIG did not report stable within ${RECOVERY_TIMEOUT_SECS}s (continuing — serving probe is authoritative)"
    fi
}

# ---------------------------------------------------------------------------
# Phase 0: Preflight — resolve topology, assert invariant preconditions
# ---------------------------------------------------------------------------
log "Chaos test: kill M4b GCE instance (env=${KAIZEN_ENV}, kill-mode=${KILL_MODE}, runs=${RUNS})"

if [[ "$KAIZEN_ENV" == "prod" && "$ALLOW_PROD" != "true" ]]; then
    fail "Refusing to target env=prod without --allow-prod"
    exit 1
fi
if [[ "$KILL_MODE" != "delete" && "$KILL_MODE" != "panic" ]]; then
    fail "Invalid --kill-mode '${KILL_MODE}' (expected: delete | panic)"
    exit 1
fi

for tool in gcloud grpcurl timeout; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        fail "$tool not found on PATH"
        exit 1
    fi
done

if [[ -z "$GCP_PROJECT" ]]; then
    GCP_PROJECT=$(gcloud config get-value project 2>/dev/null || true)
fi
if [[ -z "$GCP_PROJECT" || "$GCP_PROJECT" == "(unset)" ]]; then
    fail "No GCP project — pass --project or run: gcloud config set project <ID>"
    exit 1
fi

ACTIVE_ACCOUNT=$(gcloud auth list --filter=status:ACTIVE --format='value(account)' 2>/dev/null | head -1 || true)
if [[ -z "$ACTIVE_ACCOUNT" ]]; then
    fail "No active gcloud credentials — run: gcloud auth login (or use Workload Identity in CI)"
    exit 1
fi

if [[ -z "$ZONE" ]]; then
    ZONE=$(gc compute instances list --filter="name=('${INSTANCE}')" --format='value(zone)' 2>/dev/null | head -1 || true)
    ZONE="${ZONE##*/}"
fi
if [[ -z "$ZONE" ]]; then
    fail "Instance ${INSTANCE} not found in project ${GCP_PROJECT} — is the gcp stack deployed? (pulumi up --stack gcp-${KAIZEN_ENV})"
    exit 1
fi
REGION="${ZONE%-*}"

if [[ -z "$ENDPOINT" ]]; then
    M4B_IP=$(gc compute addresses describe "${PREFIX}-m4b-ip" --region "$REGION" --format='value(address)' 2>/dev/null || true)
    if [[ -z "$M4B_IP" ]]; then
        fail "Static address ${PREFIX}-m4b-ip not found in ${REGION} — pass --endpoint HOST:PORT explicitly"
        exit 1
    fi
    ENDPOINT="${M4B_IP}:${M4B_PORT}"
fi

INSTANCE_STATUS=$(gc compute instances describe "$INSTANCE" --zone "$ZONE" --format='value(status)' 2>/dev/null || true)
if [[ "$INSTANCE_STATUS" != "RUNNING" ]]; then
    fail "Instance ${INSTANCE} in ${ZONE} is '${INSTANCE_STATUS:-absent}', expected RUNNING"
    exit 1
fi

# The invariant under test depends on MIG autohealing + the stateful disk
# policy — assert both are configured before killing anything.
MIG_JSON=$(gc compute instance-groups managed describe "$MIG" --zone "$ZONE" --format=json 2>/dev/null || true)
if [[ -z "$MIG_JSON" ]]; then
    fail "MIG ${MIG} not found in ${ZONE}"
    exit 1
fi
if ! grep -q '"healthCheck"' <<<"$MIG_JSON"; then
    fail "MIG ${MIG} has no autohealing health check — recovery invariant cannot hold"
    exit 1
fi
if ! grep -q '"m4b-data"' <<<"$MIG_JSON"; then
    fail "MIG ${MIG} has no stateful policy for device m4b-data — RocksDB disk would not reattach"
    exit 1
fi

log "Plan: project=${GCP_PROJECT} zone=${ZONE} instance=${INSTANCE} mig=${MIG}"
log "      endpoint=${ENDPOINT} sla=${RECOVERY_SLA_MS}ms runs=${RUNS} account=${ACTIVE_ACCOUNT}"
ok "Preflight passed: instance RUNNING, autohealing + stateful disk policy present"

if [[ "$DRY_RUN" == "true" ]]; then
    ok "Dry run — no instances were harmed"
    exit 0
fi

# ---------------------------------------------------------------------------
# Phase 1: Pre-crash state — the sample we re-read after recovery
# ---------------------------------------------------------------------------
log "Phase 1: Verifying service is serving and building pre-crash RocksDB state..."

if ! probe_serving; then
    fail "M4b endpoint ${ENDPOINT} is not serving pre-chaos — aborting before any kill."
    fail "Check network reach (private subnet — see docs/runbooks/m4b-gce-chaos.md) and service health."
    exit 1
fi
ok "M4b serving at ${ENDPOINT}"

if [[ -z "$EXPERIMENT_ID" ]]; then
    CREATE_RESULT=$(grpc_call "CreateColdStartBandit" "{
        \"content_id\": \"chaos-gce-$$-$(date +%s)\",
        \"content_metadata\": {\"genre\": \"chaos\", \"source\": \"chaos_kill_m4b_gce\"},
        \"window_days\": 7
    }") || true
    EXPERIMENT_ID=$(extract_experiment_id "$CREATE_RESULT")
    if [[ -z "$EXPERIMENT_ID" ]]; then
        fail "CreateColdStartBandit failed — cannot establish pre-crash state"
        echo "$CREATE_RESULT" >&2
        exit 1
    fi
    ok "Created chaos experiment: ${EXPERIMENT_ID}"

    log "Warming up with ${WARMUP_CALLS} SelectArm calls (exercises RocksDB snapshots)..."
    for i in $(seq 1 "$WARMUP_CALLS"); do
        grpc_call "SelectArm" "{
            \"experiment_id\": \"${EXPERIMENT_ID}\",
            \"user_id\": \"chaos-warmup-${i}\",
            \"context_features\": {\"user_age_bucket\": $(( i % 5 )).0}
        }" 3 >/dev/null 2>&1 || true
    done
    sleep "$WARMUP_SETTLE_SECS"
else
    log "Reusing experiment ${EXPERIMENT_ID} for the state check"
fi

PRE_SELECT=$(grpc_call "SelectArm" "{\"experiment_id\": \"${EXPERIMENT_ID}\", \"user_id\": \"chaos-pre-crash\"}") || true
if ! grep -qE "armId|arm_id" <<<"$PRE_SELECT"; then
    fail "Pre-crash SelectArm for ${EXPERIMENT_ID} returned no arm — cannot establish baseline"
    echo "$PRE_SELECT" >&2
    exit 1
fi
PRE_SNAPSHOT=$(grpc_call "GetPolicySnapshot" "{\"experiment_id\": \"${EXPERIMENT_ID}\"}") || true
echo "$PRE_SNAPSHOT" > "$WORK_DIR/pre_crash_snapshot.json"
SNAPSHOT_HAS_DATA=false
if grep -qE "snapshot|policyData|policy_data" <<<"$PRE_SNAPSHOT"; then
    SNAPSHOT_HAS_DATA=true
fi
ok "Pre-crash state sample recorded (experiment resolves; snapshot data: ${SNAPSHOT_HAS_DATA})"

# ---------------------------------------------------------------------------
# Phases 2-5 per run: kill → detect down → detect recovery → verify state
# ---------------------------------------------------------------------------
declare -a RUN_OUTAGE_MS RUN_FROM_KILL_MS RUN_STATE RUN_RECREATED
OVERALL="PASS"

for run in $(seq 1 "$RUNS"); do
    log "── Run ${run}/${RUNS} ─────────────────────────────────────────────"

    PRE_KILL_ID=$(instance_id)
    T_KILL=$(now_ms)

    log "Phase 2: Killing ${INSTANCE} (mode=${KILL_MODE})..."
    if [[ "$KILL_MODE" == "delete" ]]; then
        if ! gc compute instances delete "$INSTANCE" --zone "$ZONE" --quiet --async \
            >"$WORK_DIR/kill_${run}.log" 2>&1; then
            fail "gcloud compute instances delete failed:"
            cat "$WORK_DIR/kill_${run}.log" >&2
            exit 1
        fi
    else
        # Kernel panic: automaticRestart reboots the VM in place; the sysrq
        # write is backgrounded so the SSH session isn't what kills it.
        timeout 30 gcloud --project "$GCP_PROJECT" compute ssh "$INSTANCE" --zone "$ZONE" \
            --tunnel-through-iap \
            --command 'sudo nohup sh -c "sleep 1; echo 1 > /proc/sys/kernel/sysrq; echo c > /proc/sysrq-trigger" >/dev/null 2>&1 & exit 0' \
            >"$WORK_DIR/kill_${run}.log" 2>&1 || true
    fi

    log "Phase 3: Probing ${ENDPOINT} for loss of service..."
    T_DOWN=""
    DOWN_DEADLINE=$(( T_KILL + DOWN_TIMEOUT_SECS * 1000 ))
    while :; do
        if ! probe_serving; then T_DOWN=$(now_ms); break; fi
        if [[ $(now_ms) -gt $DOWN_DEADLINE ]]; then break; fi
        sleep "$PROBE_INTERVAL"
    done
    if [[ -z "$T_DOWN" ]]; then
        fail "Service never went down within ${DOWN_TIMEOUT_SECS}s of the kill — chaos injection ineffective?"
        exit 1
    fi
    ok "Service down $(( T_DOWN - T_KILL ))ms after kill was issued"

    log "Phase 4: Probing until the endpoint serves again (timeout ${RECOVERY_TIMEOUT_SECS}s)..."
    T_UP=""
    UP_DEADLINE=$(( T_DOWN + RECOVERY_TIMEOUT_SECS * 1000 ))
    while :; do
        if probe_serving; then T_UP=$(now_ms); break; fi
        if [[ $(now_ms) -gt $UP_DEADLINE ]]; then break; fi
        sleep "$PROBE_INTERVAL"
    done
    if [[ -z "$T_UP" ]]; then
        fail "Service did not recover within ${RECOVERY_TIMEOUT_SECS}s."
        fail "Triage: gcloud compute instance-groups managed describe ${MIG} --zone ${ZONE};"
        fail "if the instance was recreated but never serves, verify the boot image /"
        fail "startup automation launches the M4b runtime (docs/runbooks/m4b-gce-chaos.md)."
        exit 1
    fi
    OUTAGE_MS=$(( T_UP - T_DOWN ))
    FROM_KILL_MS=$(( T_UP - T_KILL ))
    ok "Recovered: outage ${OUTAGE_MS}ms (kill→serving ${FROM_KILL_MS}ms)"

    log "Phase 5: Verifying RocksDB state and stateful-disk reattachment..."
    STATE_OK=true

    POST_SELECT=$(grpc_call "SelectArm" "{\"experiment_id\": \"${EXPERIMENT_ID}\", \"user_id\": \"chaos-post-crash-${run}\"}") || true
    if grep -qE "armId|arm_id" <<<"$POST_SELECT"; then
        ok "SelectArm: pre-crash experiment ${EXPERIMENT_ID} survived (arm returned)"
    else
        fail "SelectArm: pre-crash experiment lost — RocksDB state did NOT survive"
        echo "$POST_SELECT" >&2
        STATE_OK=false
    fi

    POST_SNAPSHOT=$(grpc_call "GetPolicySnapshot" "{\"experiment_id\": \"${EXPERIMENT_ID}\"}") || true
    if [[ "$SNAPSHOT_HAS_DATA" == "true" ]] && ! grep -qE "snapshot|policyData|policy_data" <<<"$POST_SNAPSHOT"; then
        fail "GetPolicySnapshot: pre-crash snapshot data missing post-recovery"
        STATE_OK=false
    fi

    POST_DISKS=$(gc compute instances describe "$INSTANCE" --zone "$ZONE" \
        --format='value(disks[].deviceName)' 2>/dev/null || true)
    if grep -q "m4b-data" <<<"$POST_DISKS"; then
        ok "Stateful disk m4b-data attached to the instance"
    else
        fail "Stateful disk m4b-data NOT attached post-recovery (disks: ${POST_DISKS:-none})"
        STATE_OK=false
    fi

    RECREATED="n/a"
    if [[ "$KILL_MODE" == "delete" ]]; then
        POST_KILL_ID=$(instance_id)
        if [[ -n "$POST_KILL_ID" && "$POST_KILL_ID" != "$PRE_KILL_ID" ]]; then
            RECREATED="yes"
            ok "MIG recreated the instance (id ${PRE_KILL_ID} → ${POST_KILL_ID})"
        else
            RECREATED="no"
            warn "Instance id unchanged after delete — recreation not confirmed"
        fi
    fi

    RUN_OUTAGE_MS[run]=$OUTAGE_MS
    RUN_FROM_KILL_MS[run]=$FROM_KILL_MS
    RUN_STATE[run]=$STATE_OK
    RUN_RECREATED[run]=$RECREATED
    if [[ "$STATE_OK" != "true" ]]; then OVERALL="FAIL"; fi

    if [[ "$run" -lt "$RUNS" ]]; then
        wait_mig_stable
        log "Settling ${INTER_RUN_SETTLE_SECS}s before next run..."
        sleep "$INTER_RUN_SETTLE_SECS"
    fi
done

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
echo ""
echo "============================================================="
echo "  CHAOS TEST REPORT: Kill M4b GCE instance"
echo "============================================================="
echo "  Project/zone:   ${GCP_PROJECT} / ${ZONE}"
echo "  Instance/MIG:   ${INSTANCE} / ${MIG}"
echo "  Kill mode:      ${KILL_MODE}    Recovery SLA: ${RECOVERY_SLA_MS}ms"
echo ""
printf "  %-4s %12s %14s %-10s %-6s\n" "Run" "Outage(ms)" "Kill→Serve(ms)" "Recreated" "State"
for run in $(seq 1 "$RUNS"); do
    printf "  %-4s %12s %14s %-10s %-6s\n" "$run" "${RUN_OUTAGE_MS[run]}" \
        "${RUN_FROM_KILL_MS[run]}" "${RUN_RECREATED[run]}" "${RUN_STATE[run]}"
done
echo ""

for run in $(seq 1 "$RUNS"); do
    if [[ "${RUN_OUTAGE_MS[run]}" -le "$RECOVERY_SLA_MS" ]]; then
        ok "Run ${run}: outage ${RUN_OUTAGE_MS[run]}ms <= ${RECOVERY_SLA_MS}ms SLA"
    else
        fail "Run ${run}: outage ${RUN_OUTAGE_MS[run]}ms > ${RECOVERY_SLA_MS}ms SLA"
        OVERALL="FAIL"
    fi
done

if [[ "$OVERALL" == "PASS" ]]; then
    ok "PASS: ${RUNS} run(s) recovered inside SLA with RocksDB state intact"
else
    fail "FAIL: see report above"
fi
echo "============================================================="

[[ "$OVERALL" == "PASS" ]]
