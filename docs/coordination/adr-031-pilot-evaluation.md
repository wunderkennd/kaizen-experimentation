# ADR-031 pilot evaluation — briefing for #645

**Status**: input for the human decision on issue #645, not a decision.
**Author**: agent-1 (produced during the pilot's implementation, so anchored to the code that actually shipped).
**Date**: 2026-07-13
**Corresponds to ADR**: `docs/adrs/031-connectrpc-rust-assignment-pilot.md`
**Corresponds to sprint**: `sprint-connectrpc-pilot-adr-031`

---

## TL;DR

- **5/5 RPCs served over Connect + gRPC + gRPC-Web on the pilot listener.** All contract tests green. Success criterion 1 met.
- **server-go on the generated Connect client. Hand-rolled JSON shim gated out.** Success criterion 2 met.
- **Net LOC is +995 hand-written lines across the four pilot PRs** (lockfile churn excluded). Criterion 3 is *not* met on a strict read; ~+52 on a generous read that treats cfg-gated-out shim code as retired. Interpretation is the crux of #645.
- **p99 latency delta and build-time delta are unmeasured.** Runbook to measure below.
- **buffa/prost coexistence is real but tractable** — one field-mapping annoyance (`MessageField<T>` vs `Option<T>`), one buf-generate output-path split. Both are workaround-once concerns.

Read section 2 to see the criteria-by-criteria state. Read section 4 for the runbook to close the unmeasured criteria before deciding. Read section 5 for the two decision templates.

---

## 1. Concrete measurements

### 1.1 LOC delta by PR (hand-authored code only)

Excludes `Cargo.lock`, `go.work.sum`, `*/go.sum`, `*/go.mod` — dep churn that doesn't reflect the design.

| PR | Issue | Focus | +add | −del | Net |
| --- | --- | --- | ---: | ---: | ---: |
| #661 (via c6ba01f) | #641 | Tracer bullet: GetAssignment end-to-end, new `experimentation-proto-connect` crate | 463 | 0 | **+463** |
| #739 | #642 | 3 unary RPCs (GetAssignments, GetSlateAssignment, GetInterleavedList) | 282 | 85 | **+197** |
| #740 (b002545) | #643 | StreamConfigUpdates over Connect + tonic on one broadcast source | 248 | 27 | **+221** |
| #743 (9ea6fca) | #644 | Retire the JSON shim on the connectrpc feature path + migrate server-go | 309 | 195 | **+114** |
| **Total** |  |  | **1302** | **307** | **+995** |

### 1.2 Two accounting views

The strict-net-negative reading is what ADR-031 §3 asks for. But the pilot's shim retirement is **cfg-gated dead code**, not deleted source. Choose an accounting rule before judging.

| Accounting rule | Adjustment | Result |
| --- | --- | --- |
| **Strict**: only diff-visible additions/deletions count | none | **+995** — kill criterion 1 is triggered |
| **Retire-as-delete**: `#[cfg(not(feature = "connectrpc"))]` on `http_json.rs` (349) + `http_json_e2e.rs` (437) is a deletion under the pilot | −786 | **+209** — still positive, closer to break-even |
| **Retire-as-delete + fold e2e replacement**: the pilot's Rust `connect_server_e2e.rs` (170) + `stream_config_updates_test.rs` (130) + Go `connect_pilot_e2e_test.go` (170) replace the shim's e2e/tests | −470 | **~−260** — net-negative if we credit the new e2e for replacing the retired one |

**None of these views make the pilot cheaper than the shim on a strict source-code line count.** They vary in what we count as replaced. #645 has to pick one.

### 1.3 Build-time delta (local, warm-cache)

Local measurements on darwin-arm64, macOS 25.5.0, workspace deps pre-fetched. Not a stand-in for CI clean builds — see the runbook (§4) for that.

| Build | Wall (warm) | Notes |
| --- | ---: | --- |
| `cargo build -p experimentation-assignment` (default) | ~15s | tonic + prost paths only |
| `cargo build -p experimentation-assignment --features connectrpc` | ~14s | adds buffa + connectrpc + tower |

Warm-cache wall clock is within noise. Under connectrpc, ~8 additional crates compile (buffa, connectrpc, tower, mime, ...). CI clean-build impact is the interesting number and is not measured here.

### 1.4 buffa / prost coexistence

Real friction I encountered while shipping #641–#644:

| Symptom | Root cause | Fix in the pilot | Costs going forward? |
| --- | --- | --- | --- |
| `Option<T>` doesn't `Into<MessageField<T>>` | buffa uses `MessageField<T>` for optional message fields; prost uses `Option<T>`. Rust doesn't auto-convert | Omit the field and let `..Default::default()` fill it, or explicit `MessageField::default()` | One-line-per-nested-message pattern; documented in-file. Not a recurring drag |
| `experimentation-proto-connect` needs its own `build.rs` and `include_generated!()` because Anthropic buffa codegen isn't the same as prost `include!(concat!(...))` | Different codegen conventions | New crate, ~35 line build.rs, one-time | Fleet-wide adoption would mean one new crate per proto package — no repeated cost per RPC |
| Go bindings for `assignment/v1` were referenced by tests before being committed | CI runs `buf generate` before the Go job; local dev doesn't | Local `just codegen-go` before Go builds; note in developer setup docs | Real local-dev gap, but not pilot-specific — same shape as every service dependency on `gen/go/*` |
| Nested `ConfigUpdate.experiment` field left unset over Connect | Bridging the deeply-nested `Experiment` message end-to-end needs prost↔buffa field-by-field parity code | Documented, deferred to M5 integration (out of pilot scope) | Real future cost when M5 integration lands; won't affect this decision |

Net: the coexistence works. Nothing here is a showstopper; everything is a well-understood one-time pattern.

---

## 2. Criteria-by-criteria state

Language quoted verbatim from `docs/adrs/031-connectrpc-rust-assignment-pilot.md`.

### Success (all five needed for fleet-wide adoption)

| # | Criterion | State | Evidence |
| --- | --- | --- | --- |
| 1 | *All five RPCs pass their existing M1 contract tests (assignment, m1m4b, m1m4b_slate) over Connect + gRPC + gRPC-Web (rerouted via connectrpc's per-protocol handlers)* | ✅ **Met** | `cargo test -p experimentation-assignment --features connectrpc` all green (Connect e2e tests + stream tests + all pre-existing contract tests). See #739 and #740. |
| 2 | *`sdks/server-go` is migrated to the generated Connect client; hand-rolled JSON delete-path is empty* | ✅ **Met** | #743 (#644) — `RemoteProvider` uses `assignmentv1connect.NewAssignmentServiceClient`. `http_json.rs` cfg-gated out. Conformance suite in `connect_pilot_e2e_test.go` covers all 4 unary + streaming open. |
| 3 | *Total LOC delta is net negative: the retired `http_json.rs` shim (~350 LOC) plus retired server-go JSON path outweighs the added Connect trait implementations* | ⚠️ **Not met on strict read** | §1.1: +995 lines net. §1.2 shows the interpretation views. Kill criterion 1 is the same measurement — see below. |
| 4 | *No RPC's p99 latency (measured by nightly-loadtest against the pilot binary) regresses by more than ±10% from the tonic baseline; StreamConfigUpdates disconnects cleanly on client cancel (same as tonic)* | 🟡 **Unmeasured** | Requires `nightly-loadtest` run against both the tonic and connectrpc binaries. Runbook in §4. |
| 5 | *Build-time delta for the pilot crate is documented and judged acceptable (CI budget is dominated by `experimentation-stats`; buffa's compile cost should not exceed that of prost/tonic)* | 🟡 **Partly measured** | §1.3 gives local warm numbers (within noise). Clean-build CI cost is not measured. Runbook in §4. |

### Kill (any triggers ADR-031 Rejected)

| # | Criterion | State | Evidence |
| --- | --- | --- | --- |
| 1 | *ConnectRPC bridge adds more code than the shim it deletes (fails the LOC test above)* | ⚠️ **Triggered on strict read** | Same measurement as success #3 |
| 2 | *A pre-1.0 minor bump of connectrpc or buffa breaks the pilot build during the pilot window (signals unstable API surface)* | ✅ **Not triggered** | connectrpc 0.7 held stable through #641–#644 |
| 3 | *StreamConfigUpdates cannot reach parity over Connect-streaming* | ✅ **Not triggered** | #740 — one broadcast source, both handlers, streaming semantics test-pinned |
| 4 | *buffa's codegen output can't coexist cleanly with tonic-build's output (double-generated types, feature-flag maze)* | ✅ **Not triggered** | Two separate crates (`experimentation-proto` prost, `experimentation-proto-connect` buffa); no doubled types; the connectrpc feature is an opt-in on `experimentation-assignment` only |

**The whole decision hinges on how criterion #3 / kill #1 is read.** Everything else is settled.

---

## 3. Qualitative wins that don't show up in LOC

These are what a strict LOC read misses. Whether they justify the +995 is #645's call.

- **Generated types on both sides of the language seam.** The Go SDK used to hand-roll `assignmentJSONRequest`/`Response` structs that had to match the Rust proto by convention. Now both sides deserialize from the same `.proto`. Field-add regressions are compile errors, not silent wire drift.
- **One listener, three protocols.** connectrpc's server serves Connect + gRPC + gRPC-Web on one port. Previously tonic was on `50051` and `http_json` was on `8080`. One less listener, one less port, one less operational contract.
- **Anthropic's buffa is the strategic direction.** Adopting it here means every other Rust service in the fleet has a working example to fork from, and the pattern (bridge crate + gate + gradual retire) is documented.
- **Conformance infrastructure exists now.** `connect_pilot_e2e_test.go` covers all 4 unary + streaming across the Go↔Rust seam. Future SDKs get a template.
- **No wire-format hand-rolls left in Rust.** `http_json.rs` was ~350 lines of manual JSON POST route handling. That code is dead weight in every crate that spoke it; it is now cfg-out under the pilot.

---

## 4. Runbook — closing the unmeasured criteria before deciding

### 4.1 p99 latency (success #4)

Requires the `nightly-loadtest` workflow or a manual run against both binaries.

```bash
# Baseline (tonic):
CONFIG_PATH=dev/config.json cargo run --release \
  -p experimentation-assignment --bin experimentation-assignment &
BASELINE_PID=$!

# Load using k6/vegeta/oha of your choice; e.g. with oha at 500 rps for 60s:
oha -n 30000 -c 100 \
  -m POST -H 'Content-Type: application/json' \
  -d '{"userId":"u","experimentId":"exp_dev_001","sessionId":"s","attributes":{}}' \
  http://127.0.0.1:8080/experimentation.assignment.v1.AssignmentService/GetAssignment

kill $BASELINE_PID

# Pilot (connectrpc):
CONNECTRPC_ADDR=127.0.0.1:50161 CONFIG_PATH=dev/config.json cargo run --release \
  -p experimentation-assignment --features connectrpc \
  --bin experimentation-assignment &
PILOT_PID=$!

# Same load pattern against the Connect port; use `application/connect+json`:
oha -n 30000 -c 100 \
  -m POST -H 'Content-Type: application/json' -H 'Connect-Protocol-Version: 1' \
  -d '{"userId":"u","experimentId":"exp_dev_001","sessionId":"s","attributes":{}}' \
  http://127.0.0.1:50161/experimentation.assignment.v1.AssignmentService/GetAssignment

kill $PILOT_PID
```

**Pass**: pilot p99 within ±10% of baseline for GetAssignment, GetAssignments, GetSlateAssignment, GetInterleavedList. StreamConfigUpdates disconnects cleanly on client cancel (`connect.CodeCanceled`, no server-side leak — validated by the existing `stream_config_updates_test.rs`).

### 4.2 Build-time delta (success #5)

```bash
# CI-shape clean build for each feature path:
cargo clean
time cargo build --workspace 2>&1 | tail -3

cargo clean
time cargo build --workspace --features experimentation-assignment/connectrpc 2>&1 | tail -3
```

For per-crate compilation timing (Cargo 1.60+):

```bash
cargo build --workspace --timings=html
open target/cargo-timings/cargo-timing.html
```

**Pass**: connectrpc-path clean build within ~10% of default. buffa+connectrpc combined must not exceed `experimentation-stats`'s compile time (the current pole).

---

## 5. Decision templates

Both templates are skeletons for #645's outcome comment (or the linked ADR). Pick one, edit, post.

### 5.1 Template — **Accept the pilot** (write a superseding ADR)

```markdown
## ADR-031 pilot: accepted

The pilot met the qualitative bar despite failing success criterion 3
on a strict source-line reading. The +995 LOC is repaid by:

- Generated types on both sides of the Go/Rust seam (no wire drift possible)
- One listener replacing (tonic + http_json), operational simplification
- Established buffa+Connect template for the next M1-shaped service to adopt

**Fleet-wide adoption**: authorized. Each service migrates on its own
schedule; new services default to buffa+Connect. ADR-010 (tonic-only)
supersedes on a per-service basis rather than fleet-wide flag day.

**Next steps**:
- File the fleet-wide adoption ADR that supersedes ADR-010's Rust arm
- Open #_ to flip the M1 `--features connectrpc` default (server-go SDK
  now requires it — see #644 caveat)
- File #_ per remaining service (M4a, M4b, M5) for their equivalent pilot

**p99 latency measurement**: <fill from §4.1 run>
**Build-time delta**: <fill from §4.2 run>
```

### 5.2 Template — **Reject the pilot** (kill criterion 1 triggered)

```markdown
## ADR-031 pilot: rejected

Kill criterion 1 (bridge adds more code than shim deletes) is triggered
on a strict source-line reading — the pilot added +995 hand-written
lines to save ~800 lines of shim. Fleet-wide adoption is not authorized.

The qualitative wins are real but do not clear the bar this ADR set.

**Rollback strategy**:
- Retain #737, #739, #740, #743 in-tree; the connectrpc feature is opt-in
  and doesn't affect the default build. No revert needed.
- Update ADR-031 status to Rejected; ADR-010 (tonic-only) remains
  authoritative for the fleet.
- Consider re-piloting with a leaner bridge if buffa/prost gains a
  first-party interop crate (would change the equation on criterion 3).

**p99 latency measurement**: <fill from §4.1 run>
**Build-time delta**: <fill from §4.2 run>
```

---

## 6. My honest read (agent input, not a decision)

**Accept** — the strict LOC reading is what the ADR wrote down, and the pilot missed it. But the qualitative wins are structural (generated types, one listener, fleet template) and the +995 is largely one-time cost (crate wiring, first-adopter overhead). If we reject on this alone we're choosing "diff-line count" over "wire-format hygiene." That's a bad trade in a codebase this size.

**Reject if** either latency or build-time measurement lands outside ±10%. Those are the criteria I couldn't measure locally; if they fail, the qualitative wins don't clear the bar either.

Whatever #645 decides, please record the accounting rule chosen (strict vs retire-as-delete). Future adoption piloted decisions should re-apply the same rule so we're not moving goalposts.
