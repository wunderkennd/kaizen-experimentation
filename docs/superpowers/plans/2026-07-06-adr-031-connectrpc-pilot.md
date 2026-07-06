# ADR-031: ConnectRPC (Rust) pilot on M1 Assignment — implementation (#718)

**Status:** Design lock — RFC for review.
**Plan-review:** pending — required before `just prime-issue 718` (see
[plan-review](../../guides/plan-review.md)); this plan touches M1, SDKs, and CI, so the
reviewer must not be its author. The review note lands on
[#718](https://github.com/wunderkennd/kaizen-experimentation/issues/718) and this line
then links it.
**Issue:** [#718](https://github.com/wunderkennd/kaizen-experimentation/issues/718) — P1,
agent-1 (M1 Assignment) + SDK maintainers (server-go slice); no sprint cohort assigned yet.
**Blocked by:** — none (ADR-031 Accepted 2026-06-23 via #634; M1 not mid-refactor).

---

## Summary

ADR-031 pilots the Tower-based `connectrpc` runtime (buffa codegen, **not** prost) on M1
`AssignmentService`: all five RPCs served over Connect + gRPC + gRPC-Web from one
feature-gated listener, one SDK (`sdks/server-go`) migrated to a generated Connect
client, then a fleet go/no-go decision against the ADR's explicit success/kill criteria.
The pilot's **primary measurement is the coexistence cost of a second protobuf type
system** (buffa alongside prost — the prost `experimentation-proto` crate is referenced
from 56 files / 225 sites per the ADR's baseline); latency, LOC, and CI deltas are the
other gates. The hand-rolled surface this retires is already demonstrably drifting: the
JSON shim and the Go SDK both silently drop `GetAssignmentResponse.block_index` (proto
field 6, required by M4a switchback analysis), while the landed Connect bridge carries it
(`connect_server.rs:78`).

This plan was written against live state (`main` @ `e4ec72a`, 2026-07-06), and live state
has moved past #718's framing: **a first pilot slice already landed** (in-tree references
say #641). What exists on `main` today: the containment crate
`crates/experimentation-proto-connect/` (workspace member #14; buffa/connectrpc/
connectrpc-build all pinned 0.7; crate-level edition 2024 / rust-version 1.88 while the
workspace stays 2021 / 1.80), the `connectrpc` Cargo feature on
`experimentation-assignment` (default build unchanged), a `GetAssignment`-only bridge in
`src/connect_server.rs` (other four RPCs return Unimplemented, pointing at #642/#643), a
feature-gated e2e suite (`tests/connect_server_e2e.rs`), a per-PR CI lane
(`ci.yml:195–196`), and an opt-in server-go round-trip test
(`connect_pilot_e2e_test.go`, gated on `KAIZEN_M1_CONNECT_URL`). The Rust codegen chain
is therefore **already exercised in CI**; the probes below target only what is not.

Two live defects shape the phasing. First, the committed `gen/go/` module has **no
assignment bindings** — `connect_pilot_e2e_test.go` imports packages that do not exist in
the tree, and since no workflow or justfile recipe runs `go test ./sdks/server-go/...`,
the break is invisible to the merge path. Phase A repairs this before anything else
dispatches. Second, the pilot listener today is a **third** listener (`CONNECTRPC_ADDR`,
default :50061) beside tonic :50051 and http_json :8080 — the ADR's "two listeners
collapse to one" is the *fleet-phase* payoff, not a pilot deliverable; the pilot proves
protocol parity on the additive listener while tonic stays the shipped default
throughout (kill switch: build without the feature).

Trustworthiness constraints carried through every phase: wire compatibility with the
existing exposure contract is the acceptance bar (the three existing contract-test
suites must pass over all three protocols, unchanged); `assert_finite!` semantics on
`assignment_probability` / `probability` are preserved through the bridge (enforced
inside `AssignmentServiceImpl`, asserted by parity tests); and no `.proto` file changes
during the pilot — both codegen paths consume the same schema.

### Non-goals (v1 of #718)

- **No fleet-wide adoption.** A successful pilot produces a *separate* superseding ADR
  for the Rust side of ADR-010; nothing outside M1 + server-go changes here.
- **No android / ios / web / server-python migrations** — fleet phase only
  (ADR-031 §3).
- **No `http_json.rs` deletion inside the pilot.** Retirement is a gated follow-up
  (#718.1) after the Phase F verdict plus a drift-free window — never same-day (L7).
- **No single-listener collapse and no graceful-shutdown hardening of the pilot
  listener** — post-pilot (documented in `main.rs`; see Follow-ups).
- **No change to M1's tonic client toward M4b** (`bandit_client.rs`) — explicitly out of
  scope per ADR-031 §Integration.
- **No `.proto` changes** — if one becomes necessary, the pilot pauses and the change
  ships through the normal buf lint/breaking path first (L13).

---

## Platform assumptions & probes

The Rust-side codegen bets were retired by the landed slice — those rows carry evidence
links instead of probes. Four capabilities remain unexercised on this infrastructure and
get numbered P0 probes (Phase A) with decision matrices. **No implementation phase
(B–E) dispatches before its governing probe verdict is recorded in this table.**

| # | Assumption | Exercised here before? | Probe (task + command) | Verdict |
|---|---|---|---|---|
| PA1 | buffa + connectrpc codegen chain (`connectrpc-build` 0.7) compiles in this workspace and CI | yes — `crates/experimentation-proto-connect/` is a workspace member built by every `cargo test --workspace`; feature lane `ci.yml:195–196` green on `main` @ `e4ec72a` | — | **confirmed** |
| PA2 | Crate-level edition 2024 / MSRV 1.88 coexists with the workspace's edition 2021 / rust 1.80 under the floating `stable` toolchain | yes — landed crate declares exactly this split (its `Cargo.toml` NOTE) and CI (`dtolnay/rust-toolchain@stable`) builds it | — | **confirmed** (watch item: a `stable` regression would surface in the existing CI lane, not silently) |
| PA3 | `connectrpc` 0.7 server-streaming can express `StreamConfigUpdates` (M5-fed config stream) with Connect-protocol streaming a Go/web client can consume — kill criterion #3 | no — the landed bridge stubs it (`connect_server.rs:99–107`) | P0.1 (commands below) | pending |
| PA4 | The pilot listener actually negotiates **gRPC**, **gRPC-Web**, and **binary** payloads, not just the Connect+JSON path the e2e suite covers | no — only Connect+JSON exercised (`connect_server_e2e.rs`) | P0.2 (commands below) | pending |
| PA5 | connect-go codegen for the assignment package lands in the committed `gen/go/` module and `sdks/server-go` compiles against it | no — `gen/go/` holds only metrics/common today; the pilot's Go test imports **missing** packages; server-go runs in **no CI lane** | P0.3 (commands below) | pending |
| PA6 | The `nightly-loadtest` harness can target the Connect listener for the ±10% p99 gate | no — harness has never pointed at :50061 | P0.4 (commands below) | pending |

### P0.1 — streaming spike (gates Phase C)

On a scratch branch, replace the `stream_config_updates` stub with a minimal
`ServiceStream` that emits two hardcoded `ConfigUpdate` frames, then:

```
cargo test -p experimentation-assignment --features connectrpc   # bridge compiles
cargo run  -p experimentation-assignment --features connectrpc &
curl -sN --http1.1 -H 'content-type: application/connect+json' \
  -d '{"lastKnownVersion":0}' \
  http://127.0.0.1:50061/experimentation.assignment.v1.AssignmentService/StreamConfigUpdates
```

Decision matrix: **two framed messages + EndStream arrive** → PA3 confirmed, Phase C
dispatches. **`ServiceStream` cannot be constructed from an async source (M5-fed
watch)** → escalate on #718 before B/C dispatch; this is kill criterion #3 territory —
the verdict is recorded, not worked around. **Frames arrive but framing is
non-conformant** (a connect-go client errors in P0.3's environment) → pin the exact
0.7.x patch, retest once; still failing → same escalation.

### P0.2 — protocol matrix (gates Phase D)

Against the same running feature build:

```
grpcurl -plaintext -d '{"userId":"test-user-1","experimentId":"exp_dev_001","sessionId":"s1"}' \
  127.0.0.1:50061 experimentation.assignment.v1.AssignmentService/GetAssignment   # gRPC
curl -s -H 'content-type: application/proto' --data-binary @get_assignment.bin \
  http://127.0.0.1:50061/experimentation.assignment.v1.AssignmentService/GetAssignment  # Connect+binary
npx tsx probe_grpcweb.ts   # @connectrpc/connect-web client, transport: gRPC-Web, same call
```

Decision matrix: **all three succeed** → PA4 confirmed. **gRPC fails** → likely h2c
negotiation on the hyper listener; fix inside the feature (server builder config) and
retest — if unfixable in 0.7, escalate (protocol parity is success criterion #1).
**gRPC-Web fails** → same escalation path; do not ship Phase D's CI lane asserting a
protocol the runtime cannot serve.

### P0.3 — connect-go bindings into `gen/go/` (gates Phase E; repairs live break D2)

```
cd proto && buf generate --path experimentation/assignment/v1/assignment_service.proto \
                         --path experimentation/common/v1
cp -r proto/gen/go/experimentation/assignment ../gen/go/experimentation/
cd ../gen/go && go build ./...
cd ../sdks/server-go && go build ./... && go vet ./...
```

Decision matrix: **compiles** → commit the bindings in Phase A (generated tree,
size-gate exempt). **Import paths disagree** with
`github.com/org/experimentation/gen/go/experimentation/assignment/v1` (the proto's
`go_package` option predates the managed-mode prefix — buf.gen.yaml's managed override
must win) → adjust `buf.gen.yaml` managed-mode config only; **never** hand-edit
generated files. **Module-level conflict** (go.mod requires) → resolve in `gen/go/go.mod`
(it already carries `connectrpc.com/connect v1.17.0` for metrics), escalate only if
connect-go's minimum protobuf-go clashes with the pinned `v1.36.11`.

### P0.4 — loadtest harness recon (gates the Phase F p99 gate)

Read `.github/workflows/nightly-loadtest.yml` and its harness entrypoint; identify how
the M1 target address is provided; dry-run the smallest M1 scenario locally against a
feature build with the target pointed at :50061; record the exact command line in this
plan. Decision matrix: **harness parameterizes the target** → Phase D adds a
Connect-target lane. **Target hardcoded to :50051** → Phase D adds an input/env knob to
the harness (counts toward the CI-tooling delta the ADR says must be documented and
judged acceptable). **Harness cannot drive HTTP/1.1+h2c Connect at all** → p99 gate is
measured with the harness's gRPC mode against :50061 (same listener, gRPC protocol) and
the limitation is recorded in the F memo.

---

## Locks — binding for implementers

| # | Lock | One-line answer | Decided (owner, date) |
|---|---|---|---|
| L1 | Codegen containment | All buffa/Connect codegen lives in `crates/experimentation-proto-connect` (assignment/v1 + common/v1 only); `experimentation-proto` (prost/tonic) is untouched in every pilot PR | ADR-031 (#634), 2026-06-23 — landed |
| L2 | Feature gate | `connectrpc` feature on `experimentation-assignment`; default build byte-identical in behavior (no buffa/connectrpc/tower in the default dependency tree — guarded by the L9 `cargo tree` check) | ADR-031 (#634), 2026-06-23 — landed |
| L3 | Pilot client slice | `sdks/server-go` only; android/ios/web/python are fleet-phase | ADR-031 (#634), 2026-06-23 |
| L4 | tonic stays shippable | The tonic path ships on `main` at every step; pilot kill = build without the feature (no revert of business logic needed) | ADR-031 (#634), 2026-06-23 |
| L5 | Version pins | Rust: `connectrpc`/`buffa`/`connectrpc-build` = 0.7.x exact via Cargo.lock (charter pin: buffa 0.7); Go: `connectrpc.com/connect v1.17.0`. Bumps land only as dedicated PRs; a breaking 0.x bump inside the pilot window triggers kill criterion #2 review on #718 | ADR-031 (#634) + agent-1 charter, 2026-07-04 |
| L6 | Pilot topology | The Connect listener is **additive** on `CONNECTRPC_ADDR` (default :50061); ports/wire of tonic :50051 and http_json :8080 do not change during the pilot | agent-1 (this plan, matches landed `main.rs:90–105`), 2026-07-06 |
| L7 | `http_json.rs` retirement | Not part of any pilot phase. Follow-up #718.1, gated on the Phase F GO verdict **and** ≥1 drift-free sprint of the E1 conformance check — never same-day with a replacement landing | agent-1 (this plan; template replacement rule), 2026-07-06 |
| L8 | server-go cutover shape | E1 ships the generated-client provider ALONGSIDE the hand-rolled path with a byte-parity drift test; the hand-rolled deletion is E2, a separate PR gated on a clean E1 window (≥5 drift-free days) | agent-1 (this plan), 2026-07-06 |
| L9 | Coexistence-cost measurement (the pilot's primary measurement) | Acceptance: (a) across ALL pilot PRs, `git diff --stat` touches **zero** prost-referencing files outside the pilot surfaces (pilot surfaces = `crates/experimentation-proto-connect/`, `crates/experimentation-assignment/{Cargo.toml,src/lib.rs,src/main.rs,src/connect_server.rs,tests/connect_server_e2e.rs}`, `sdks/server-go/`, `gen/go/`, CI/loadtest lanes); (b) `cargo tree -p experimentation-assignment` (default features) lists no `buffa`/`connectrpc`/`tower` — asserted in the Phase D CI lane; (c) prost surface re-measured at F with `grep -rlE '\bexperimentation_proto::' crates --include='*.rs'` (files) and the `-o … \| wc -l` variant (sites) against the ADR baseline 56/225; (d) per-PR LOC ledger (added/deleted counted lines, generated trees excluded) appended to #718 by every phase | agent-1 (this plan), 2026-07-06 |
| L10 | Phase ↔ issue mapping | B=#642, C=#643, E=#644 (the numbers the landed stubs and go.mod already reference); plan-review re-verifies those issues exist/are open (this executor has no `gh`) and files any that are missing; one phase = one issue = one worker session = one PR | agent-1 (this plan), 2026-07-06 |
| L11 | Protocol-parity acceptance | All five RPCs pass `assignment_test.rs`, `m1m4b_contract_test.rs`, `m1m4b_slate_contract_test.rs` semantics over Connect, gRPC, and gRPC-Web before Phase F renders a verdict (success criterion #1, verbatim) | ADR-031 (#634), 2026-06-23 |
| L12 | Generated-code posture | Rust: generated at build time via `connectrpc-build` + `include_generated!` (never checked in) — mirrors `experimentation-proto`. Go: generated code IS checked into `gen/go/` (existing repo pattern, metrics precedent); regenerated only via `buf generate`, never hand-edited | agent-1 (this plan; both halves landed as precedent), 2026-07-06 |
| L13 | Schema freeze | No `.proto` changes during the pilot; both codegen paths consume the identical schema. A required proto change pauses the pilot until it clears buf lint/breaking on `main` | agent-1 (this plan; ADR-031 §Proto Schema), 2026-07-06 |

---

## Cross-phase artifacts

| Artifact | Producer phase / task | Consumer phase / task | Lock # | Status |
|---|---|---|---|---|
| P0 probe verdicts written into this plan's PA table + #718 comment | A / P0.1–P0.4 | B, C, D, E dispatch gates; F memo | L10 | pending |
| Baseline re-run record (`cargo test -p experimentation-assignment`, both feature states) in `progress.log.md` | A / P0.0 | F build/CI delta accounting | L9 | pending |
| `gen/go/experimentation/assignment/v1/**` (+ `assignmentv1connect`) committed | A / A2 | E (generated client); also un-breaks the landed `connect_pilot_e2e_test.go` | L12 | pending |
| server-go CI lane (`go test ./sdks/server-go/...`) | A / A3 | E, F (Go-side evidence visible to merge path) | L8 | pending |
| Full 5-RPC bridge in `connect_server.rs` | B (3 unary) + C (stream) | D contract matrix; E round trip; F LOC ledger | L11 | pending |
| Cross-protocol contract harness (three suites × three protocols) | D / D1 | F criteria evidence | L11 | pending |
| `cargo tree` default-feature purity assertion in CI | D / D2 | F coexistence report | L9 | pending |
| Loadtest lane targeting `CONNECTRPC_ADDR` + captured tonic baseline | D / D2 | F p99 gate (±10%) | — | pending |
| E1 conformance drift test (generated client vs hand-rolled path, byte parity) | E / E1 | E2 deletion gate; #718.1 retirement gate | L7, L8 | pending |
| Per-PR LOC ledger comments on #718 | every phase / final task | F net-LOC criterion | L9 | pending |
| Fleet decision memo (`docs/superpowers/specs/` note + ADR-031 status update) | F / F3 | post-pilot superseding ADR or Rejected mark; #718.1 trigger | — | pending |

---

## Phase A — P0 probes + Go toolchain repair (maps to #718 directly)

**Executor:** local/multiclaude only — needs cargo + go + buf + `npx`/grpcurl for probes,
and it edits `.github/workflows/` (a `claude-code-action` session cannot push workflow
files; its `GITHUB_TOKEN` lacks the `workflows` scope).
**Size budget:** ~180 counted lines / 7 counted files (generated `gen/go/**` and this
plan's verdict edits are exempt; ci.yml lane ~15 lines).

### Task A1: probes P0.0–P0.2, P0.4

- [ ] **Step 1 (P0.0):** baseline `cargo test -p experimentation-assignment` (default,
  then `--features connectrpc`) — append results to `progress.log.md` (the design
  session's executor could not run cargo; this is the first recorded local baseline)
- [ ] **Step 2:** run P0.1 (streaming spike — scratch, not merged), P0.2 (protocol
  matrix), P0.4 (loadtest recon) exactly as specified above
- [ ] **Step 3:** record verdicts in this plan's PA table + a numbered comment on #718;
  any red verdict STOPS dependent phases and escalates on #718 (kill-criteria language,
  not workarounds)

### Task A2: P0.3 + commit assignment bindings

- [ ] **Step 1:** run P0.3; commit `gen/go/experimentation/assignment/v1/**` (+ any
  common deps buf emits) — files: `gen/go/**` (generated-exempt), `gen/go/go.mod` if
  needed
- [ ] **Step 2:** `cd sdks/server-go && go build ./... && go test ./...` (pilot e2e still
  env-gated → skips; the point is compilation is restored)

### Task A3: server-go CI lane

- [ ] **Step 1:** add `go test ./sdks/server-go/...` (and `go vet`) to the Go stage of
  `.github/workflows/ci.yml`, path-filtered on `sdks/server-go/**` + `gen/go/**` —
  closes the D2 blind spot so Phases E1/E2 are merge-path-visible

---

## Phase B — remaining unary bridges (#642)

**Executor:** any (cargo only; no `gh`, no workflow edits).
**Size budget:** ~300 counted lines / 4 files (`connect_server.rs`,
`connect_server_e2e.rs`, possibly a shared test helper, Cargo.toml only if a dev-dep is
missing).

### Task B1: bridge `GetAssignments`, `GetSlateAssignment`, `GetInterleavedList`

- [ ] **Step 1:** replace the three Unimplemented stubs with domain delegation, same
  shape as `get_assignment` (buffa views ⇄ domain at the trait boundary; no business
  logic in the bridge) — file: `crates/experimentation-assignment/src/connect_server.rs`
- [ ] **Step 2:** preserve fail-fast semantics: `assignment_probability` /
  `SlotProbability.probability` flow through untouched (`assert_finite!` lives in
  `AssignmentServiceImpl`); `is_uniform_random` and interleaving `provenance` map
  round-trip losslessly

### Task B2: parity e2e per RPC

- [ ] **Step 1:** extend `tests/connect_server_e2e.rs`: happy path + NotFound per RPC,
  asserting the same JSON contract the http_json / tonic paths serve (reuse
  `dev/config.json` inputs, as the landed tests do)
- [ ] **Step 2:** update the Unimplemented test to cover only `StreamConfigUpdates`
  (still stubbed until C); LOC-ledger comment on #718

---

## Phase C — `StreamConfigUpdates` over Connect streaming (#643)

**Executor:** any (cargo only). **Dispatch gate: PA3 confirmed.**
**Size budget:** ~260 counted lines / 5 files.

### Task C1: streaming bridge

- [ ] **Step 1:** implement `stream_config_updates` bridging the M5-fed config
  source (same source the tonic path serves; see `stream_client.rs` /
  `config_cache.rs`) into `connectrpc::ServiceStream<ConfigUpdate>` — honoring
  `last_known_version` resume and `is_deletion` — file: `src/connect_server.rs`
- [ ] **Step 2:** map stream-teardown errors to Connect codes consistently with the
  tonic path (`tonic_status_to_connect` already handles the unary side)

### Task C2: streaming e2e + kill-criterion checkpoint

- [ ] **Step 1:** e2e: subscribe, receive an initial snapshot + a pushed update +
  EndStream over Connect protocol — file: `tests/connect_server_e2e.rs`
- [ ] **Step 2:** record the kill-criterion-#3 checkpoint on #718 (parity reached /
  not) — this is the pilot's riskiest technical bet; the verdict is explicit either way

---

## Phase D — cross-protocol contract matrix + CI/perf lanes

**Executor:** local/multiclaude only (edits `.github/workflows/`; captures Actions
evidence). **Dispatch gate: PA4 confirmed; B and C merged.**
**Size budget:** ~380 counted lines / 9 files. If the harness alone exceeds ~300, split:
D1 (harness) and D2 (lanes) as two PRs — declared here so the split is not improvised.

### Task D1: contract tests over three protocols

- [ ] **Step 1:** a feature-gated test harness that runs the assertions of
  `assignment_test.rs`, `m1m4b_contract_test.rs`, `m1m4b_slate_contract_test.rs`
  against the Connect listener over Connect, gRPC, and gRPC-Web (transport-parameterized
  client helper; the suites' semantics are the bar — L11) — files:
  `tests/connect_contract_matrix.rs` + helper
- [ ] **Step 2:** wire into the existing `rust-features` CI lane (extends
  `ci.yml:195–196`, no new job)

### Task D2: purity + perf lanes

- [ ] **Step 1:** CI assertion: `cargo tree -p experimentation-assignment` (default
  features) contains no `buffa`/`connectrpc`/`tower` (L9-b)
- [ ] **Step 2:** loadtest lane per the P0.4 verdict; capture the tonic :50051 baseline
  and the :50061 Connect run in the same night's harness config; publish both numbers to
  #718

---

## Phase E — server-go migration (#644)

**Executor:** E1 any (go + cargo to run the pilot server for conformance); E2 any (go).
**Dispatch gate: PA5 confirmed (A2/A3 merged); B merged (client needs ≥ the unary
RPCs).**
**Size budget:** E1 ~350 counted lines / 6 files; E2 ~230 changed lines / 4 files
(mostly deletions). **Two PRs, per L8 — never one.**

### Task E1: generated-client provider alongside the hand-rolled path

- [ ] **Step 1:** add a Connect-backed provider using
  `assignmentv1connect.NewAssignmentServiceClient` (per ADR-007 the provider chain is
  the seam: `RemoteProvider` keeps the hand-rolled JSON internals; the new provider is
  selectable) — file: `sdks/server-go/experimentation.go` (+ small new file if cleaner)
- [ ] **Step 2:** conformance drift test: both providers against the running pilot
  server AND against the `mockAssignmentServer` fixture (`experimentation_test.go:275`),
  asserting field-for-field parity of `Assignment` results — including `block_index`
  once the generated path carries it (documenting the hand-rolled path's known drop is
  part of the evidence)
- [ ] **Step 3:** convert `connect_pilot_e2e_test.go` from env-gated skip into the
  conformance entrypoint (still env-gated in CI until a served instance exists in the
  lane; local run instructions in the file header stay)

### Task E2: delete the hand-rolled JSON path (separate PR, gated)

- [ ] **Step 1:** after ≥5 drift-free days of E1's check (L8): remove the hand-rolled
  JSON request/response structs + POST internals from `RemoteProvider`, pointing it at
  the generated client; convert remaining `mockAssignmentServer`-based tests to the
  generated types
- [ ] **Step 2:** LOC-ledger comment on #718 (this is the pilot's largest deletion
  besides the gated `http_json.rs`)

---

## Phase F — Convergence: measurement + fleet verdict

**Executor:** local/multiclaude only (needs `gh` for issue evidence + Actions history;
files follow-ups).
**Size budget:** ~120 counted lines / 4 files (memo + ADR status edit are
markdown-exempt; a small measurement script is counted).

### Task F1: Acceptance-criteria mapping

| Issue AC (#718 scope) | Test/file location | Cross-phase artifact row |
|---|---|---|
| New crate `experimentation-proto-connect`, `experimentation-proto` untouched | landed: `crates/experimentation-proto-connect/` (build in every workspace test); L9-a diff audit at F | Fleet decision memo |
| `connectrpc` feature: one Tower listener serving Connect+gRPC+gRPC-Web (binary+JSON) across all five RPCs; default build unchanged | `tests/connect_server_e2e.rs` (B, C) + `tests/connect_contract_matrix.rs` (D) + `cargo tree` purity lane (D2) | Full 5-RPC bridge; contract harness; purity assertion |
| server-go on a generated Connect client; hand-rolled POST path deleted; round-trip test → conformance check | E1 conformance test; E2 deletion diff | E1 drift test; LOC ledger |
| Measure against ADR success/kill criteria; fleet decision writeup | F2 evidence table + F3 memo | Fleet decision memo |

### Task F2: criteria evidence table + full-suite regression

- [ ] **Step 1:** assemble the evidence table — every ADR-031 success criterion (5) and
  kill criterion (4) with its measured value and source link: contract matrix runs (L11),
  net-LOC ledger (with the gated `http_json.rs` 350 LOC itemized as *pledged, not yet
  deleted* — #718.1), p99 delta (D2 lane, ±10% gate), build-time/CI delta (rust-features
  wall-clock + workspace cold-build with the member crate vs without), 0.x churn log
  (L5), coexistence audit (L9-a/b/c with the re-measured prost surface vs 56/225)
- [ ] **Step 2:** regression:

```
cargo test --workspace
cargo test -p experimentation-assignment --features connectrpc
go test ./sdks/server-go/...
just test-hash
python3 scripts/check_docs.py
```

### Task F3: verdict + memo + follow-ups

- [ ] **Step 1:** fleet decision memo at
  `docs/superpowers/specs/2026-MM-DD-adr-031-fleet-verdict.md`: GO → draft the
  superseding "ConnectRPC for Rust" ADR (fleet scope, ADR-010 supersession) as a
  *proposal*, and file #718.1 (`http_json.rs` retirement, L7-gated) + fleet SDK issues;
  NO-GO → mark ADR-031 Rejected with the evidence table inline, keep ADR-010 as-is,
  file the feature/crate removal chore
- [ ] **Step 2:** update ADR-031's Status line + CLAUDE.md pilot references; final PR
  `feat(assignment): ADR-031 pilot convergence — criteria evidence + fleet verdict`
  with `Closes #718`, linking this table with every Cross-phase row at `verified`

---

## Test plan summary

| Phase | Test files | Count target |
|---|---|---|
| A | probe transcripts on #718; `go build`/`go vet` restored for `sdks/server-go` | 4 probe verdicts recorded |
| B | `tests/connect_server_e2e.rs` (extended) | ≥6 new cases (2 per unary RPC) |
| C | `tests/connect_server_e2e.rs` (streaming) | ≥3 cases (snapshot, push, resume) |
| D | `tests/connect_contract_matrix.rs`; `cargo tree` lane; loadtest lane | 3 suites × 3 protocols green; purity lane green |
| E | server-go conformance drift test; converted pilot e2e | ≥4 cases incl. `block_index` parity |
| F | criteria evidence table; full-suite regression | 9/9 criteria rows evidenced |

---

## Risks + rollback

| Risk | Severity | Mitigation |
|---|---|---|
| Pre-1.0 churn: a 0.x bump of connectrpc/buffa breaks the build mid-pilot | high | L5 exact pins via Cargo.lock; bumps only as dedicated PRs; a break = kill criterion #2 review on #718, not a silent fix |
| `StreamConfigUpdates` cannot reach parity (kill criterion #3) | high | P0.1 probes it BEFORE C dispatches; C2 records the checkpoint explicitly; feature stays default-off so `main` is never exposed |
| Bridge outgrows the shim (kill criterion #1: bridge LOC > 350 deleted) | med | per-PR LOC ledger (L9-d) makes the trend visible at every phase, not at F |
| buffa/prost coexistence leaks (duplicate symbols, plugin conflicts, buf friction — kill criterion #4) | med | L1 containment (landed); L9-b `cargo tree` purity lane; L13 schema freeze; L9-a zero-touch diff audit |
| server-go latent break recurs (generated module drifts from proto) | med | A3's CI lane makes `sdks/server-go` + `gen/go` merge-path-visible for the first time |
| Floating `stable` toolchain regresses against edition-2024 crate | low | PA2 watch item; existing CI lane fails loudly; escape hatch = pin `rust-toolchain.toml` temporarily (its own PR) |
| Loadtest harness can't drive Connect (PA6 red) | low | P0.4 fallback: gRPC-protocol run against the same listener, limitation documented in the F memo |

**Rollback for any phase:** A — revert (bindings + CI lane are additive); B/C — revert
the bridge commits (feature-gated; default build untouched by L2/L9-b); D — revert lanes
(tests are feature-gated); E1 — revert the provider (hand-rolled path still primary);
E2 — revert restores the hand-rolled path from git; F — docs revert. **Pilot-wide
kill:** one PR deletes the feature + crate + lanes; L1 containment bounds the blast
radius to the pilot surfaces listed in L9-a.

**Replacement rule (graduated cutover):** every replacement in this plan ships the new
path ALONGSIDE the old one with a drift check, and the deletion is a separate, later PR
gated on a clean window — never same-day. Concretely: `http_json.rs` retirement is
**not in the pilot at all** (L7 — #718.1, gated on the F verdict + ≥1 drift-free sprint
of E1's conformance check); the server-go hand-rolled path deletion is E2, gated on ≥5
drift-free days of E1 (L8). Precedent: #680 P1→P3.

---

## Follow-ups

| Item | Trigger | Owner |
|---|---|---|
| **#718.1** — retire `http_json.rs` on the pilot path (350 LOC) + drop the :8080 listener | F verdict = GO **and** ≥1 drift-free sprint of E1's conformance check (L7) | agent-1 |
| **#718.2** — superseding "ConnectRPC for Rust" fleet ADR (ADR-010 Rust half) | F verdict = GO | agent-1 + agent-0 |
| **#718.3** — single-listener collapse + graceful shutdown for the Connect listener (main.rs NOTE) | fleet ADR accepted | agent-1 |
| **#718.4** — android / ios / web / server-python generated-client migrations (re-enable `connect-swift`/`connect-kotlin`) | fleet ADR accepted | agent-1 + SDK maintainers |
| **#718.5** — chore: CLAUDE.md says "13 Rust crates"; workspace has 14 (`experimentation-proto-connect`); also record the pilot in the crate table | immediately (doc drift exists today) | any |
| **#718.6** — justfile recipe for the pilot (`just test-connect` = feature tests + server-go suite) | after Phase B merges | any |

---

## Branch + PR conventions

- Phase workers use canonical names: `agent-1/feat/adr-031-<phase-slug>` (e.g.
  `agent-1/feat/adr-031-unary-bridges`); server-go phases may use
  `agent-1/feat/adr-031-server-go-client`. This design session itself ships from a
  harness-pinned `claude/...` ref (tolerated family; attribution rides PR metadata per
  CLAUDE.md § Branch-naming).
- Commits: Conventional Commits — `feat(assignment):` for B/C/D, `feat(sdks):` or
  `feat(server-go):` for E, `chore(ci):` where a lane is the whole diff.
- Intermediate phase PRs use `Refs #718` (+ `Closes #642/#643/#644` per L10 where the
  phase completes that sub-issue). Only the Phase F convergence PR carries
  `Closes #718`.
- Every phase PR ends with the LOC-ledger comment on #718 (L9-d).
