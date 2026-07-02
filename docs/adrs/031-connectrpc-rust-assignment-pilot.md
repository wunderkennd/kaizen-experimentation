# ADR-031: ConnectRPC (Rust) Pilot on M1 Assignment Service

**Status**: Accepted
**Date**: 2026-06-22 (proposed); 2026-06-23 (accepted via #634 — pilot approved; fleet-wide adoption remains gated on the success criteria below)
**Deciders**: Agent-1 (M1 Assignment), Agent-0 (cross-cutting RPC / coordination), SDK maintainers
**Cluster**: — (cross-cutting RPC infrastructure; scoped revisit of ADR-010)

---

## Context

[ADR-010](010-connectrpc.md) (Accepted, 2026-03-03) settled the wire story as
**"ConnectRPC for Go, tonic for Rust, shared proto contracts."** At that time there
was no production-grade Connect runtime for Rust, so Rust services standardised on
`tonic` and any browser/SDK-facing Connect compatibility was hand-rolled.

Two things have changed since:

1. **A real Rust Connect runtime now exists.** [`connectrpc`](https://github.com/anthropics/connect-rust)
   (anthropics/connect-rust) is a Tower-based runtime that serves Connect, gRPC, and
   gRPC-Web from one `tower::Service`, in both binary and JSON protobuf. It passes the
   full ConnectRPC conformance suite (3,600 server + 6,872 client tests across the
   three protocols). It is **pre-1.0** (0.x), MSRV 1.88, edition 2024.

2. **We have quietly re-implemented Connect by hand — six times.** The server side
   hand-rolls Connect-unary in `crates/experimentation-assignment/src/http_json.rs`
   (350 LOC), and **every** SDK client hand-rolls the matching `POST
   /experimentation.assignment.v1.AssignmentService/<Method>` JSON call:
   `sdks/android` (`ExperimentClient.kt:96`), `sdks/ios` (`connect-swift` present but
   commented out), `sdks/web`, `sdks/server-python` (`providers.py:68`),
   `sdks/server-go`. The `connect-swift`/`connect-kotlin` dependencies sit commented
   out precisely because the hand-rolled path was cheaper than wiring a real Connect
   client.

The hand-rolled surface is also **incomplete and drifting**. `AssignmentService` has
five RPCs; the JSON shim covers three:

| RPC | Kind | gRPC (tonic) | Hand-rolled JSON shim |
|-----|------|:---:|:---:|
| `GetAssignment` | unary | ✅ | ✅ |
| `GetAssignments` | unary | ✅ | ✅ |
| `GetSlateAssignment` | unary | ✅ | ✅ |
| `GetInterleavedList` | unary | ✅ | ❌ no JSON path |
| `StreamConfigUpdates` | server-streaming | ✅ | ❌ no JSON path |

M1 today runs **two listeners** — plain gRPC (`main.rs:81`, no `tonic_web`) and a
separate hand-rolled HTTP/JSON server (`http_json::serve`). Adding a sixth RPC means
hand-writing serde types + a route + CORS again, in the server and in five clients.

This ADR does **not** propose reversing ADR-010. It proposes a **time-boxed,
single-service pilot** to gather the evidence needed to decide whether a fleet-wide
adoption (which *would* supersede ADR-010 for Rust) is worth its cost. M1 is chosen
because it is the most client-facing service, it owns the hand-rolled shim, and it
exercises both unary and streaming.

### The central cost, stated up front

`connectrpc` is built on [`buffa`](https://github.com/anthropics/buffa) (Anthropic's
protobuf runtime) with its own codegen (`protoc-gen-buffa` + `protoc-gen-connect-rust`),
**not** `prost`. The entire platform's generated types live in the prost/tonic
`experimentation-proto` crate, referenced from **56 files / 225 sites**. Adoption is
therefore not a transport swap — it introduces a second protobuf type system. The
pilot's primary job is to measure how painful that coexistence actually is on one
service before committing the fleet.

---

## Decision

Stand up M1 `AssignmentService` on `connectrpc` **in parallel** with the existing
tonic stack, behind a Cargo feature, serve all five RPCs over Connect + gRPC +
gRPC-Web from one listener, retire `http_json.rs` on that path, and migrate **one**
SDK client to a generated Connect client to prove the round trip. Then decide
fleet-wide adoption against explicit success/kill criteria.

### 1. Contain codegen blast radius to a new crate

Generate buffa + Connect code for the **assignment package only** in a new crate,
leaving `experimentation-proto` (prost/tonic) untouched:

```
crates/experimentation-proto-connect/
├── build.rs          # connectrpc-build → protoc-gen-buffa + protoc-gen-connect-rust,
│                     #   scoped to proto/experimentation/assignment/v1 + common deps
├── Cargo.toml
└── src/lib.rs        # re-exports generated buffa types + AssignmentService trait
```

This mirrors the existing `experimentation-proto/build.rs` pattern (rung: reuse the
established codegen shape) but swaps the plugin set. No existing crate changes its
generated types.

### 2. Feature-gate the M1 server path

`experimentation-assignment` gains a `connectrpc` feature. Default build is unchanged
(tonic gRPC + `http_json`). With `--features connectrpc`, the binary instead mounts a
single `connectrpc` Tower service (Connect + gRPC + gRPC-Web, binary + JSON) on one
port, covering **all five** RPCs including `StreamConfigUpdates` and the
currently-JSON-less `GetInterleavedList`.

The business logic (`AssignmentServiceImpl`) is **not** rewritten. It already operates
on domain inputs (`assign(&experiment_id, &user_id, …)`), so the Connect handlers
bridge `buffa` request/response views ⇄ domain calls at the trait boundary — the same
boundary `http_json.rs` already bridges serde ⇄ domain. The bridge code is the unit of
cost we are measuring.

### 3. Prove the client side (the actual payoff)

Migrate **`sdks/server-go`** from its hand-rolled JSON `POST` to a generated Connect
client against the pilot server, and **delete** the hand-rolled path in that SDK.
Server-go is chosen over mobile because it has no app-store/release-cycle latency and
already has an HTTP round-trip test (`experimentation_test.go:283`) to convert into a
real conformance check. If server-go succeeds, android/ios/web follow in the
fleet-wide phase (not this pilot).

### 4. Keep tonic shippable throughout

The tonic path is **not removed** during the pilot. M1 remains releasable on `main` at
every step; the pilot lives behind the feature flag and a dedicated branch.

### 5. Success criteria (→ write a superseding ADR for ADR-010, Rust side)

- All 5 `AssignmentService` RPCs pass the existing contract tests
  (`assignment_test.rs`, `m1m4b_contract_test.rs`, `m1m4b_slate_contract_test.rs`)
  over **all three** protocols (Connect, gRPC, gRPC-Web).
- `sdks/server-go` runs on a generated Connect client with its hand-rolled JSON path
  deleted; wire-compatible with the existing exposure contract.
- **Net LOC is negative**: deletions (`http_json.rs` 350 LOC + server-go hand-rolled
  client) exceed additions (new crate + bridge + buffa codegen config).
- p99 latency within **±10%** of the tonic baseline under the existing
  `nightly-loadtest` harness.
- Build-time and CI-tooling delta documented and judged acceptable.

### 6. Kill criteria (→ mark this ADR Rejected; keep ADR-010 as-is)

- The buffa⇄domain bridge adds more code than `http_json.rs` deletes.
- A pre-1.0 `connectrpc`/`buffa` minor bump breaks the build within the pilot window
  (signals unacceptable churn risk for fleet adoption).
- `StreamConfigUpdates` cannot reach parity over Connect-streaming.
- The buffa and prost codegen cannot coexist cleanly in one workspace/CI (protoc
  plugin conflicts, duplicate-symbol issues, `buf` integration friction).

---

## Consequences

### Benefits

1. **Two listeners collapse to one.** M1's gRPC server + hand-rolled JSON server
   become a single Connect/gRPC/gRPC-Web endpoint with binary *and* JSON negotiation —
   capabilities (gRPC-Web, binary Connect) M1 lacks today.
2. **The shim's coverage gap closes for free.** `GetInterleavedList` and
   `StreamConfigUpdates` gain a browser/SDK-reachable path without hand-writing it.
3. **SDK clients can drop to real Connect libraries.** The end state deletes
   hand-rolled JSON in five languages and re-enables the commented-out
   `connect-swift`/`connect-kotlin` dependencies.
4. **Evidence, not vibes.** The pilot produces real LOC/latency/build numbers to
   settle the fleet-wide question instead of relitigating it abstractly.

### Trade-offs

1. **A second protobuf runtime enters the workspace** (`buffa` alongside `prost`),
   even if scoped to one crate. This is the thing the pilot exists to risk-test, but it
   is real complexity while both coexist.
2. **Pre-1.0 dependency.** `connectrpc` 0.x may break across minor versions; pin
   exactly and budget for churn.
3. **New build tooling.** CI must provide `protoc-gen-buffa` and
   `protoc-gen-connect-rust` plugins; the assignment crate gains an out-of-cargo
   toolchain dependency at build time.
4. **Temporary duplication.** During the pilot M1 carries both transports and (until
   server-go flips) both client styles.
5. **MSRV/edition pull.** `connectrpc` declares MSRV 1.88 / edition 2024; confirm the
   workspace toolchain satisfies it.

---

## Implementation Details

### Proto Schema

No `.proto` changes. `proto/experimentation/assignment/v1/assignment_service.proto`
is the input to *both* codegen paths during the pilot: prost/tonic (existing) and
buffa/connect (new crate). Shared `common/` messages are pulled in as codegen deps.

### Crate Layout / Public API

- New `crates/experimentation-proto-connect` — buffa types + the generated
  `AssignmentService` Connect trait for the assignment package, via
  `connectrpc-build` in `build.rs` (analogous to `experimentation-proto/build.rs`).
- `experimentation-assignment` — new `connectrpc` Cargo feature; under it, a
  `connect_server.rs` implements the generated trait by delegating to the existing
  `AssignmentServiceImpl` and mounts the Tower service (optionally with
  `connectrpc-health` + `connectrpc-reflection` for `grpcurl`/mesh probes).

### Integration

| Module | Integration |
|--------|-------------|
| M1 Assignment (server) | Pilot target — Connect server behind a feature flag; `http_json.rs` retired on that path. |
| M1 Assignment (client→M4b) | M1's client call to M4b `BanditPolicyService` **stays on tonic** for the pilot; out of scope. |
| M4b Policy | Unchanged. Contract is exercised by M1's existing tonic client. |
| SDKs | `sdks/server-go` migrates to a generated Connect client; android/ios/web/python deferred to fleet phase. |
| CI | Add buffa/connect protoc plugins to the toolchain; run conformance + contract tests across all three protocols. |

---

## Validation

### Unit Tests / Proptest Invariants

- The buffa⇄domain bridge round-trips every `AssignmentService` request/response type
  losslessly (mirror the existing serde round-trip tests in `http_json.rs`).
- `assert_finite!` is preserved on `assignment_probability` / `probability` fields
  through the new bridge (CLAUDE.md fail-fast rule).

### Conformance / Cross-Protocol Tests

- Existing M1 contract tests (`assignment_test.rs`, `m1m4b_contract_test.rs`,
  `m1m4b_slate_contract_test.rs`) run unchanged against the Connect server over
  Connect, gRPC, and gRPC-Web — wire compatibility is the acceptance bar.
- A new `sdks/server-go` test calls the pilot server through the generated Connect
  client and asserts byte-for-byte agreement with the recorded exposure contract
  (converted from `experimentation_test.go:283`).

### Performance

- `nightly-loadtest` run against the Connect endpoint vs the tonic baseline; p99 within
  ±10% is the gate.

---

## Dependencies

- **ADR-010** (ConnectRPC for Go, tonic for Rust): this ADR **scopedly revisits** the
  "tonic for Rust" half. It does **not** supersede ADR-010; a successful pilot would be
  followed by a separate superseding ADR covering fleet-wide adoption.
- **ADR-006** (Cargo workspace): adds one crate (`experimentation-proto-connect`).
- **ADR-007** (SDK provider abstraction): the `server-go` client migration runs
  through the existing provider fallback chain.
- **ADR-024 / ADR-025** (M7/M5 language ports): precedent that transport/language
  changes are ADR-gated and rolled out behind parallel implementations, not big-bang.
- **Enables** (if successful): a fleet-wide "ConnectRPC for Rust" ADR retiring
  `http_json.rs`, the four `tonic_web::enable` wrappers, and the hand-rolled SDK
  clients.

---

## Rejected Alternatives

| Alternative | Reason Rejected |
|-------------|-----------------|
| **Full fleet migration now** | 56-file / 225-site prost→buffa ripple across 6 services with no evidence it pays off. The pilot is the cheap way to buy that evidence. |
| **Keep hand-rolling Connect** | Works today but is incomplete (2 of 5 M1 RPCs have no JSON path) and duplicated across 1 server + 5 SDK clients; each new RPC re-pays the cost six times. |
| **Adopt `connectrpc` only to replace the JSON shim, keep gRPC on tonic** | Two stacks on one service for no net simplification; misses the point (real Connect clients, unified listener). |
| **Wait for `connectrpc` 1.0** | The pilot is explicitly designed to absorb 0.x risk on one non-critical surface; waiting forgoes the information and keeps paying the hand-rolled tax. |
| **Migrate the whole `experimentation-proto` crate to buffa first** | Highest-blast-radius option; would touch every service before we know coexistence is even viable. Inverts the risk order. |

---

## References

- [anthropics/connect-rust](https://github.com/anthropics/connect-rust) — `connectrpc`
  runtime, `connectrpc-build`, `connectrpc-health`, `connectrpc-reflection`; conformance
  suite (3,600 server + 6,872 client tests).
- [`buffa`](https://github.com/anthropics/buffa) — protobuf runtime backing `connectrpc`.
- [ConnectRPC protocol](https://connectrpc.com/) — wire spec the hand-rolled shim approximates.
- `crates/experimentation-assignment/src/http_json.rs` — the 350-LOC hand-rolled shim retired by the pilot.
- `crates/experimentation-assignment/src/main.rs:81` — current tonic gRPC bootstrap.
- `proto/experimentation/assignment/v1/assignment_service.proto` — the 5-RPC service under pilot.
- SDK hand-rolled clients: `sdks/android/.../ExperimentClient.kt:96`,
  `sdks/server-python/src/experimentation/providers.py:68`, `sdks/server-go/`,
  `sdks/web/`, `sdks/ios/Package.swift:12`.
- ADR-010, ADR-006, ADR-007, ADR-024, ADR-025 — related decisions.
