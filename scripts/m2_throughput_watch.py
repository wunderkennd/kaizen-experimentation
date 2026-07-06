#!/usr/bin/env python3
"""M2 throughput run: Redpanda offset/lag sampler + pass/fail gate (issue #502).

Subcommands:
  sample       Poll Redpanda via rpk, appending one JSONL sample per interval:
                 {"ts": epoch, "hwm": {topic: sum_high_watermark}, "hwm_total": n,
                  "lag": {group: total_lag_or_null}, "errors": [...]}
  evaluate     Combine samples + the k6 summary (loadtest_m2_throughput.js) into
               the Phase 4 pass/fail verdict. Exit 0 = PASS, 1 = FAIL.
  parse-topics Parse `rpk topic describe -p` text from stdin (debug/tests).
  parse-group  Parse `rpk group describe` text from stdin (debug/tests).

Connection env (mirrors infra/pkg/streaming/redpanda.go provisioning):
  BROKERS, KAFKA_SASL_USER, KAFKA_SASL_PASS,
  KAFKA_SASL_MECHANISM (default SCRAM-SHA-512), KAFKA_TLS_ENABLED, RPK_BIN

Stdlib only — no third-party dependencies. rpk text output is parsed instead
of --format json because the text tables are stable across the rpk versions
deployed here while JSON key casing is not.
"""

import argparse
import json
import os
import subprocess
import sys
import time


def rpk_base_args():
    args = [os.environ.get("RPK_BIN", "rpk")]
    xopts = ["brokers=" + os.environ.get("BROKERS", "localhost:9092")]
    if os.environ.get("KAFKA_TLS_ENABLED", "").lower() in ("1", "true"):
        xopts.append("tls.enabled=true")
    user = os.environ.get("KAFKA_SASL_USER", "")
    if user:
        xopts.append("user=" + user)
        xopts.append("pass=" + os.environ.get("KAFKA_SASL_PASS", ""))
        xopts.append("sasl.mechanism=" + os.environ.get("KAFKA_SASL_MECHANISM", "SCRAM-SHA-512"))
    for x in xopts:
        args.extend(["-X", x])
    return args


def run_rpk(cmd):
    """Run one rpk command, returning (stdout, error_string_or_None)."""
    try:
        proc = subprocess.run(
            rpk_base_args() + cmd, capture_output=True, text=True, timeout=20
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return "", f"rpk {' '.join(cmd)}: {exc}"
    if proc.returncode != 0:
        return proc.stdout, f"rpk {' '.join(cmd)}: exit {proc.returncode}: {proc.stderr.strip()[:200]}"
    return proc.stdout, None


def column_from_right(header_tokens, name):
    """Offset of a column counted from the row end. Counting from the right
    survives the REPLICAS column, whose bracketed value ([1 2 3]) breaks
    left-indexed whitespace splitting."""
    return len(header_tokens) - 1 - header_tokens.index(name)


def parse_topic_hwm(text):
    """Sum HIGH-WATERMARK over the PARTITIONS table of `rpk topic describe -p`."""
    total, rows, from_right = 0, 0, None
    for line in text.splitlines():
        tokens = line.split()
        if not tokens:
            from_right = None
            continue
        if "HIGH-WATERMARK" in tokens and "PARTITION" in tokens:
            from_right = column_from_right(tokens, "HIGH-WATERMARK")
            continue
        if from_right is not None and tokens[0].isdigit():
            value = tokens[len(tokens) - 1 - from_right]
            if value.lstrip("-").isdigit():
                total += int(value)
                rows += 1
    if rows == 0:
        raise ValueError("no partition rows with HIGH-WATERMARK found")
    return total


def parse_group_lag(text):
    """Total lag from `rpk group describe`: sum the per-partition LAG column,
    falling back to the TOTAL-LAG summary line (rows may be absent while a
    group is rebalancing)."""
    total, rows, lag_idx, total_lag = 0, 0, None, None
    for line in text.splitlines():
        tokens = line.split()
        if not tokens:
            lag_idx = None
            continue
        if tokens[0] == "TOTAL-LAG" and len(tokens) > 1 and tokens[1].isdigit():
            total_lag = int(tokens[1])
        if tokens[0] == "TOPIC" and "LAG" in tokens:
            lag_idx = tokens.index("LAG")  # left-indexed: columns before LAG are single tokens
            continue
        if lag_idx is not None and len(tokens) > lag_idx:
            value = tokens[lag_idx]
            if value.isdigit():
                total += int(value)
                rows += 1
    if rows > 0:
        return total
    if total_lag is not None:
        return total_lag
    raise ValueError("no LAG rows or TOTAL-LAG line found")


def take_sample(topics, groups):
    sample = {"ts": time.time(), "hwm": {}, "lag": {}, "errors": []}
    for topic in topics:
        out, err = run_rpk(["topic", "describe", topic, "-p"])
        if err is None:
            try:
                sample["hwm"][topic] = parse_topic_hwm(out)
            except ValueError as exc:
                err = f"{topic}: {exc}"
        if err:
            sample["errors"].append(err)
    sample["hwm_total"] = sum(sample["hwm"].values()) if len(sample["hwm"]) == len(topics) else None
    for group in groups:
        out, err = run_rpk(["group", "describe", group])
        if err is None:
            try:
                sample["lag"][group] = parse_group_lag(out)
                continue
            except ValueError as exc:
                err = f"{group}: {exc}"
        sample["lag"][group] = None
        sample["errors"].append(err)
    return sample


def cmd_sample(args):
    topics = [t for t in args.topics.split(",") if t]
    groups = [g for g in args.groups.split(",") if g]
    if args.once:
        sample = take_sample(topics, groups)
        json.dump(sample, sys.stdout, indent=2)
        print()
        return 1 if (sample["hwm_total"] is None) else 0
    with open(args.out, "a", encoding="utf-8") as fh:
        while True:  # runs until SIGTERM/SIGINT from the orchestrator
            started = time.time()
            fh.write(json.dumps(take_sample(topics, groups)) + "\n")
            fh.flush()
            time.sleep(max(0.0, args.interval - (time.time() - started)))


def load_samples(path):
    samples = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                samples.append(json.loads(line))
    return sorted(samples, key=lambda s: s["ts"])


def cmd_evaluate(args):
    with open(args.k6_summary, encoding="utf-8") as fh:
        k6 = json.load(fh)
    samples = load_samples(args.samples)
    valid = [s for s in samples if s.get("hwm_total") is not None]
    steady = [s for s in valid if args.steady_start <= s["ts"] <= args.steady_end]

    checks, warnings = [], []

    # -- Measurement 2: producer offset advance = sustained through-Redpanda rate
    if len(steady) >= 2:
        span = steady[-1]["ts"] - steady[0]["ts"]
        overall = (steady[-1]["hwm_total"] - steady[0]["hwm_total"]) / span if span > 0 else 0.0
        window_rates = []
        for a, b in zip(steady, steady[1:]):
            if b["ts"] > a["ts"]:
                window_rates.append((b["hwm_total"] - a["hwm_total"]) / (b["ts"] - a["ts"]))
        floor = args.bucket_floor * args.target_eps
        ok = overall >= args.target_eps and all(r >= floor for r in window_rates)
        checks.append((
            "sustained_throughput", ok,
            f"offset-advance {overall:,.0f} ev/s over {span:.0f}s steady state "
            f"(target ≥ {args.target_eps:,.0f}); slowest sample window "
            f"{min(window_rates):,.0f} ev/s (floor {floor:,.0f})" if window_rates else
            f"offset-advance {overall:,.0f} ev/s (target ≥ {args.target_eps:,.0f})",
        ))
    else:
        checks.append(("sustained_throughput", False,
                       f"insufficient steady-state samples ({len(steady)}; need ≥ 2)"))

    # -- Measurement 1 vs 2: zero message loss (accepted events all reached Redpanda)
    if valid:
        produced = valid[-1]["hwm_total"] - valid[0]["hwm_total"]
        accepted = k6.get("events_accepted", 0)
        invalid = k6.get("events_invalid", 0)
        lost = max(0, accepted - produced)
        ok = lost == 0 and invalid == 0
        detail = f"accepted {accepted:,}, offset advance {produced:,} (incl. drain), invalid {invalid:,}"
        if lost:
            detail += f" — {lost:,} accepted event(s) never reached Redpanda"
        if produced > accepted:
            warnings.append(
                f"offset advance exceeds accepted by {produced - accepted:,} "
                "(concurrent producers? run against a quiesced stack)")
        checks.append(("zero_message_loss", ok, detail))
    else:
        checks.append(("zero_message_loss", False, "no valid offset samples"))

    # -- Measurement 3: downstream consumer lag bounded
    lag_window = [s for s in samples if s["ts"] >= args.steady_start]
    groups = sorted({g for s in samples for g in s.get("lag", {})})
    for group in groups:
        series = [s["lag"][group] for s in lag_window if s.get("lag", {}).get(group) is not None]
        if not series:
            message = f"consumer group '{group}' not observable (no lag data)"
            if args.require_groups:
                checks.append((f"consumer_lag_bounded[{group}]", False, message))
            else:
                warnings.append(message + " — check skipped")
            continue
        peak, final = max(series), series[-1]
        checks.append((
            f"consumer_lag_bounded[{group}]", peak <= args.lag_threshold,
            f"peak lag {peak:,}, final {final:,} (threshold {args.lag_threshold:,}, "
            "parity with PipelineConsumerLag alert)",
        ))
    if not groups:
        warnings.append("no consumer groups configured — lag criterion not exercised")

    # -- Generator health: the load actually offered what the gate assumes
    dropped = k6.get("dropped_iterations", 0)
    err_rate = k6.get("error_rate", 0)
    checks.append((
        "generator_health", dropped == 0 and err_rate < 0.001,
        f"dropped_iterations {dropped:,}, gRPC error rate {err_rate:.5f} (< 0.001)",
    ))
    if k6.get("events_duplicate", 0):
        warnings.append(f"dedup dropped {k6['events_duplicate']:,} events — event_id uniqueness bug?")

    all_pass = all(ok for _, ok, _ in checks)
    print("=" * 72)
    print("  M2 THROUGHPUT GATE — 100K events/sec via Redpanda (issue #502)")
    print("=" * 72)
    for name, ok, detail in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {name}: {detail}")
    for warning in warnings:
        print(f"  [warn] {warning}")
    print("=" * 72)
    print(f"  RESULT: {'PASS' if all_pass else 'FAIL'}")
    print("=" * 72)

    if args.report:
        with open(args.report, "w", encoding="utf-8") as fh:
            json.dump({
                "pass": all_pass,
                "checks": [{"name": n, "pass": ok, "detail": d} for n, ok, d in checks],
                "warnings": warnings,
                "k6": k6,
                "steady_start": args.steady_start,
                "steady_end": args.steady_end,
            }, fh, indent=2)
    return 0 if all_pass else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_sample = sub.add_parser("sample")
    p_sample.add_argument("--out", default="/dev/stdout")
    p_sample.add_argument("--interval", type=float, default=5.0)
    p_sample.add_argument("--topics", default="exposures,metric_events,reward_events,qoe_events")
    p_sample.add_argument("--groups", default="bandit-policy-service")
    p_sample.add_argument("--once", action="store_true", help="single sample to stdout (preflight)")

    p_eval = sub.add_parser("evaluate")
    p_eval.add_argument("--samples", required=True)
    p_eval.add_argument("--k6-summary", required=True)
    p_eval.add_argument("--target-eps", type=float, required=True)
    p_eval.add_argument("--steady-start", type=float, required=True)
    p_eval.add_argument("--steady-end", type=float, required=True)
    p_eval.add_argument("--lag-threshold", type=int, default=100_000)
    p_eval.add_argument("--bucket-floor", type=float, default=0.95)
    p_eval.add_argument("--require-groups", action="store_true")
    p_eval.add_argument("--report", default="")

    sub.add_parser("parse-topics")
    sub.add_parser("parse-group")

    args = parser.parse_args()
    if args.cmd == "sample":
        return cmd_sample(args)
    if args.cmd == "evaluate":
        return cmd_evaluate(args)
    if args.cmd == "parse-topics":
        print(parse_topic_hwm(sys.stdin.read()))
        return 0
    print(parse_group_lag(sys.stdin.read()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
