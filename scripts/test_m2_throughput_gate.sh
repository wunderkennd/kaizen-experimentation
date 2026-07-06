#!/usr/bin/env bash
# Offline tests for scripts/m2_throughput_watch.py — rpk parsers + gate verdicts.
# No cluster, k6, or rpk required; fixtures model rpk text output and run data.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WATCH="$HERE/m2_throughput_watch.py"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS + 1)); echo "  ✓ $1"; }
bad()  { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }
check(){ if eval "$2"; then ok "$1"; else bad "$1"; fi; }

# --- rpk output parsers ------------------------------------------------------
cat > "$TMP/topic.txt" <<'EOF'
SUMMARY
NAME        exposures
PARTITIONS  3
REPLICAS    3

PARTITIONS
PARTITION  LEADER  EPOCH  REPLICAS  LOG-START-OFFSET  HIGH-WATERMARK
0          1       2      [1 2 3]   0                 1000
1          2       2      [1 2 3]   0                 2500
2          3       2      [1 2 3]   10                500
EOF
HWM=$(python3 "$WATCH" parse-topics < "$TMP/topic.txt")
check "parse-topics sums high watermarks across bracketed REPLICAS" "[ '$HWM' = '4000' ]"

cat > "$TMP/group.txt" <<'EOF'
GROUP        bandit-policy-service
COORDINATOR  1
STATE        Stable
BALANCER     range
MEMBERS      1
TOTAL-LAG    9999

TOPIC          PARTITION  CURRENT-OFFSET  LOG-END-OFFSET  LAG  MEMBER-ID  CLIENT-ID  HOST
reward_events  0          100             120             20   m-1        rdkafka    10.0.0.1
reward_events  1          50              80              30   m-1        rdkafka    10.0.0.1
EOF
LAG=$(python3 "$WATCH" parse-group < "$TMP/group.txt")
check "parse-group prefers per-partition LAG rows over TOTAL-LAG" "[ '$LAG' = '50' ]"

cat > "$TMP/group-empty.txt" <<'EOF'
GROUP        bandit-policy-service
STATE        Empty
TOTAL-LAG    123
EOF
LAG2=$(python3 "$WATCH" parse-group < "$TMP/group-empty.txt")
check "parse-group falls back to TOTAL-LAG when the table is absent" "[ '$LAG2' = '123' ]"

# --- gate fixtures -------------------------------------------------------------
# Scenario timeline: baseline ts=90, warmup ends 100, steady 100..130 @ 1000 ev/s,
# drain samples 140/150. Gate: target 1000 eps, floor 0.95, lag threshold 100000.
mk_samples() { # file, then "ts:hwm_total:lag" triples (lag may be null)
    local f="$1"; shift
    : > "$f"
    for triple in "$@"; do
        IFS=: read -r t h l <<< "$triple"
        echo "{\"ts\": $t, \"hwm\": {\"all\": $h}, \"hwm_total\": $h, \"lag\": {\"bandit-policy-service\": $l}, \"errors\": []}" >> "$f"
    done
}
mk_k6() { # file accepted invalid dropped
    cat > "$1" <<EOF
{"events_sent": $2, "events_accepted": $2, "events_duplicate": 0,
 "events_invalid": $3, "dropped_iterations": $4, "error_rate": 0.0}
EOF
}
gate() { # samples k6 extra-args... -> sets RC, output in $TMP/gate.out
    local s="$1" k="$2"; shift 2
    python3 "$WATCH" evaluate --samples "$s" --k6-summary "$k" \
        --target-eps 1000 --steady-start 100 --steady-end 130 \
        --lag-threshold 100000 --bucket-floor 0.95 "$@" > "$TMP/gate.out" 2>&1
    RC=$?
}

mk_samples "$TMP/pass.jsonl" 90:0:0 100:5000:100 110:15000:400 120:25000:300 130:35000:200 140:36000:50 150:36000:0
mk_k6 "$TMP/pass.json" 36000 0 0
gate "$TMP/pass.jsonl" "$TMP/pass.json"
check "clean run passes (exit 0)" "[ $RC -eq 0 ]"
check "clean run report says PASS" "grep -q 'RESULT: PASS' '$TMP/gate.out'"

mk_samples "$TMP/dip.jsonl" 90:0:0 100:5000:100 110:15000:100 120:17000:100 130:27000:100 150:28000:0
mk_k6 "$TMP/dip.json" 27000 0 0
gate "$TMP/dip.jsonl" "$TMP/dip.json"
check "mid-run rate dip fails the sustained check" "[ $RC -eq 1 ] && grep -q '\[FAIL\] sustained_throughput' '$TMP/gate.out'"

mk_k6 "$TMP/loss.json" 36500 0 0   # 500 accepted events never advanced an offset
gate "$TMP/pass.jsonl" "$TMP/loss.json"
check "message loss fails the gate" "[ $RC -eq 1 ] && grep -q 'never reached Redpanda' '$TMP/gate.out'"

mk_samples "$TMP/lag.jsonl" 90:0:0 100:5000:100 110:15000:250000 120:25000:300 130:35000:200 150:36000:0
gate "$TMP/lag.jsonl" "$TMP/pass.json"
check "lag above threshold fails the bounded-lag check" "[ $RC -eq 1 ] && grep -q '\[FAIL\] consumer_lag_bounded' '$TMP/gate.out'"

mk_k6 "$TMP/invalid.json" 36000 10 0
gate "$TMP/pass.jsonl" "$TMP/invalid.json"
check "invalid events fail the zero-loss check" "[ $RC -eq 1 ] && grep -q '\[FAIL\] zero_message_loss' '$TMP/gate.out'"

mk_k6 "$TMP/dropped.json" 36000 0 42
gate "$TMP/pass.jsonl" "$TMP/dropped.json"
check "dropped k6 iterations fail generator_health" "[ $RC -eq 1 ] && grep -q '\[FAIL\] generator_health' '$TMP/gate.out'"

mk_samples "$TMP/nogroup.jsonl" 90:0:null 100:5000:null 110:15000:null 120:25000:null 130:35000:null 150:36000:null
gate "$TMP/nogroup.jsonl" "$TMP/pass.json"
check "unobservable consumer group warns but passes by default" "[ $RC -eq 0 ] && grep -q 'not observable' '$TMP/gate.out'"
gate "$TMP/nogroup.jsonl" "$TMP/pass.json" --require-groups
check "unobservable consumer group fails with --require-groups" "[ $RC -eq 1 ]"

mk_samples "$TMP/sparse.jsonl" 90:0:0 110:15000:100 150:36000:0
gate "$TMP/sparse.jsonl" "$TMP/pass.json"
check "insufficient steady-state samples fail the gate" "[ $RC -eq 1 ] && grep -q 'insufficient steady-state samples' '$TMP/gate.out'"

echo ""
echo "=== $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
