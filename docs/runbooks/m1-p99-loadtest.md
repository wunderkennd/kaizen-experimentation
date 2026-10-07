# Runbook: M1 p99 Latency Load Test

**Validates two things with one script**:

1. **ADR-032 gate G1** (ADR-031 §4.1) — Connect p99 within ±10% of the tonic baseline on every unary RPC, before any fleet fan-out PR merges (#849).
2. **#500 M1/M7 Cloud Run smoke** — p99 < 5ms against the deployed M1 service in the sprint-I.3 infrastructure gate.

Same script, different environment variables.

## Script

`scripts/loadtest/m1-p99.js` — k6, ~230 lines. Handles both gRPC and Connect JSON via the `PROTOCOL` env var (auto-detected from the URL: dev ports 5005x mean gRPC, anything else Connect). Since the ADR-032 default flip, M1 serves both protocols on one listener (50051 in dev, 8080 on Cloud Run). Steady state, constant VUs, single 60 s stage. p99 threshold configurable via `P99_TARGET_MS`.

Latency is k6's built-in per-request metric for the protocol: `grpc_req_duration` on gRPC, `http_req_duration` on Connect. Each request is tagged with its RPC name, so `--summary-export` carries a p99 per RPC (`grpc_req_duration{rpc:GetAssignment}` and so on) as well as the overall `{scenario:steady}` series.

## Pass/fail gate

The k6 threshold `p(99) < ${P99_TARGET_MS}` on the latency metric (overall and per RPC) is the load-bearing assertion. k6 exits non-zero on threshold breach so CI wiring is trivial:

```bash
k6 run scripts/loadtest/m1-p99.js  # exit 0 = PASS
```

For the ADR-032 G1 comparison, set `P99_TARGET_MS` high (e.g. `1000`) so the
local SLA threshold doesn't fail the run; the comparison between runs is the gate.

## Running: local (ADR-032 G1 comparison)

Dev-config-only path; no cloud creds needed. Main no longer builds a tonic M1:
the default flip (ADR-032 step 0) deleted it. To compare against tonic, build
the baseline from `db507c3`, the last commit before the flip, where the
default build is tonic and `--features connectrpc` adds the pilot listener on
:50061. To compare the current server against an earlier Connect build, run
both on :50051 with `PROTOCOL=grpc` and `PROTOCOL=connect`.

Compare like with like:

| Run | Binary | Target | What it isolates |
| --- | --- | --- | --- |
| tonic gRPC (baseline) | `db507c3`, default build | `http://127.0.0.1:50051`, `PROTOCOL=grpc` | — |
| connectrpc gRPC | `db507c3` with `--features connectrpc` (pilot listener) or current main | `http://127.0.0.1:50061` (pilot) / `:50051` (main), `PROTOCOL=grpc` | server stack only (same wire protocol, same k6 client) |
| connectrpc Connect JSON | same as above | same as above, `PROTOCOL=connect` | what browsers and SDKs actually send |

The Connect JSON run uses k6's HTTP client and the gRPC runs use k6's gRPC
client, so the JSON row compares protocols as well as servers. G1 is decided
on the gRPC-to-gRPC row; the JSON row shows the client-facing number.

```bash
# Pre-flip commit: tonic by default, the pilot listener behind a feature.
git worktree add /tmp/m1-preflip db507c3
(cd /tmp/m1-preflip && cargo build --release -p experimentation-assignment \
  && cp target/release/experimentation-assignment /tmp/m1-tonic \
  && cargo build --release -p experimentation-assignment --features connectrpc \
  && cp target/release/experimentation-assignment /tmp/m1-connect)

# Pin the server and k6 to different cores so they don't steal from each other.
CONFIG_PATH=$PWD/dev/config.json taskset -c 0,1 /tmp/m1-tonic &
TARGET_URL=http://127.0.0.1:50051 PROTOCOL=grpc P99_TARGET_MS=1000 \
  taskset -c 2,3 k6 run --summary-export=/tmp/tonic-grpc.json scripts/loadtest/m1-p99.js
kill %1

CONFIG_PATH=$PWD/dev/config.json taskset -c 0,1 /tmp/m1-connect &   # Connect listener on :50061
TARGET_URL=http://127.0.0.1:50061 PROTOCOL=grpc P99_TARGET_MS=1000 \
  taskset -c 2,3 k6 run --summary-export=/tmp/connect-grpc.json scripts/loadtest/m1-p99.js
TARGET_URL=http://127.0.0.1:50061 PROTOCOL=connect P99_TARGET_MS=1000 \
  taskset -c 2,3 k6 run --summary-export=/tmp/connect-json.json scripts/loadtest/m1-p99.js
kill %1

# Per-RPC p99 (ms) from each run
for f in /tmp/tonic-grpc.json /tmp/connect-grpc.json; do
  jq -r --arg f "$f" '.metrics | to_entries[]
    | select(.key | startswith("grpc_req_duration{"))
    | "\($f)\t\(.key)\t\(.value["p(99)"])"' "$f"
done
```

Run at least three interleaved rounds (tonic, connectrpc, tonic, ...) with a
short discarded warm-up before each, and compare medians: single 60 s samples
on a shared machine move by several percent run to run.

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
- **M7 Flags** — same script pattern will work but the RPC method names differ; #500 SHOULD cover both, so infra-4's CI wiring will need a sibling `m7-p99.js` (out of scope for this scaffold). ADR-032 re-checks each service's p99 with this harness before its default flips.
- **Server-side latency** — k6 measures from the client. On a small machine the k6 gRPC client's own CPU cost dominates absolute p99, which is why G1 compares runs rather than reading the absolute number.
- **Streaming p99** — `StreamConfigUpdates` isn't a unary RPC; measuring streaming latency needs a different methodology (delivery lag from server publish → client receive), tracked separately if the pilot passes.
