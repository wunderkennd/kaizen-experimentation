# Runbook: M1 p99 Latency Load Test

**Validates two things with one script**:

1. **ADR-031 §4.1 pilot vs baseline comparison** — pilot p99 within ±10% of the tonic baseline is a load-bearing gate on the ADR-031 fleet-wide adoption decision (#645 HITL).
2. **#500 M1/M7 Cloud Run smoke** — p99 < 5ms against the deployed M1 service in the sprint-I.3 infrastructure gate.

Same script, different environment variables.

## Script

`scripts/loadtest/m1-p99.js` — k6, ~200 lines. Handles both gRPC (tonic baseline, port 50051) and Connect (pilot, port 50161, or Cloud Run over HTTPS) via `PROTOCOL` env var (auto-detected from URL). Steady state, constant VUs, single 60 s stage. Global p99 threshold configurable via `P99_TARGET_MS`.

## Pass/fail gate

The k6 threshold `rpc_latency_ms { p(99) < ${P99_TARGET_MS} }` is the load-bearing assertion. k6 exits non-zero on threshold breach so CI wiring is trivial:

```bash
k6 run scripts/loadtest/m1-p99.js  # exit 0 = PASS
```

For the ADR-031 comparison run, capture the summary JSON from both variants:

```bash
k6 run --summary-export=/tmp/baseline.json scripts/loadtest/m1-p99.js
# ... start pilot binary ...
k6 run --summary-export=/tmp/pilot.json scripts/loadtest/m1-p99.js

# Compare p99 (bash + jq):
b=$(jq -r '.metrics.rpc_latency_ms.values["p(99)"]' /tmp/baseline.json)
p=$(jq -r '.metrics.rpc_latency_ms.values["p(99)"]' /tmp/pilot.json)
python3 -c "
b, p = float('$b'), float('$p')
delta = (p - b) / b * 100
print(f'baseline p99 = {b:.2f} ms')
print(f'pilot    p99 = {p:.2f} ms')
print(f'delta        = {delta:+.1f}% (must be within ±10% per ADR-031 §4.1)')
"
```

## Running: local (ADR-031 pilot comparison)

Dev-config-only path; no cloud creds needed.

```bash
# Terminal 1 — tonic baseline
cargo run --release -p experimentation-assignment
# → listens on 0.0.0.0:50051

# Terminal 2 — measure baseline
TARGET_URL=http://127.0.0.1:50051 k6 run \
  --summary-export=/tmp/baseline.json scripts/loadtest/m1-p99.js

# Terminal 1 — restart with pilot feature
cargo run --release -p experimentation-assignment --features connectrpc
# → adds a Connect listener on 0.0.0.0:50161 alongside tonic

# Terminal 2 — measure pilot
TARGET_URL=http://127.0.0.1:50161 k6 run \
  --summary-export=/tmp/pilot.json scripts/loadtest/m1-p99.js
```

Feed the two p99 values into the briefing at `docs/coordination/adr-031-pilot-evaluation.md` §4.1 to fill the last outstanding blank for the HITL decision on #645.

## Running: Cloud Run smoke (#500)

Once M1 is deployed to Cloud Run (blocked by #488/#495 infra tasks — see #500 spec):

```bash
TARGET_URL=https://m1-assignment-<hash>-<region>.a.run.app \
  DURATION=60s \
  P99_TARGET_MS=5 \
  k6 run scripts/loadtest/m1-p99.js
```

CI wiring belongs to infra-4 per the #500 label. Expected shape: scheduled workflow that runs this script against the current M1 URL, uploads the k6 summary as an artifact, gates the sprint-I.3 milestone.

## Tuning knobs

| Env | Default | Purpose |
| --- | --- | --- |
| `TARGET_URL` | (required) | Base URL. gRPC when port is in the 5005x range, Connect otherwise. |
| `PROTOCOL` | auto | Force `grpc` or `connect` when the port heuristic is wrong. |
| `DURATION` | `60s` | k6 stage duration. |
| `VUS` | `20` | Concurrent virtual users. Throughput ceiling ≈ `VUS × 100` req/s. |
| `P99_TARGET_MS` | `5` | Threshold value. |
| `CONFIG_PATH` | (built-in) | JSON `{experimentIds, slateIds, interleavedIds}` corpus override. |

## What this does NOT cover

- **Startup time / cold-start** — not measured here (Cloud Run cold-start is a separate concern; ADR-031 pilot pass criteria don't include it).
- **M7 Flags** — same script pattern will work but the RPC method names differ; #500 SHOULD cover both, so infra-4's CI wiring will need a sibling `m7-p99.js` (out of scope for this scaffold).
- **Streaming p99** — `StreamConfigUpdates` isn't a unary RPC; measuring streaming latency needs a different methodology (delivery lag from server publish → client receive), tracked separately if the pilot passes.
