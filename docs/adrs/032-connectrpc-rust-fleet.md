# ADR-032: ConnectRPC for All Rust Services (Fleet-Wide)

**Status**: Proposed
**Date**: 2026-10-04
**Deciders**: Agent-0 (cross-cutting RPC / coordination), Agent-1 (M1), Agent-2 (M2 ingest), Agent-4 (M4a/M4b), Agent-5 (M5 Rust), Agent-7 (M7), SDK maintainers
**Cluster**: — (cross-cutting RPC infrastructure; supersedes the Rust half of ADR-010)

---

## Context

[ADR-010](010-connectrpc.md) chose **"ConnectRPC for Go, tonic for Rust"** because no
production-grade Rust Connect runtime existed. [ADR-031](031-connectrpc-rust-assignment-pilot.md)
piloted [`connectrpc`](https://github.com/anthropics/connect-rust) (on the `buffa`
protobuf runtime) on M1 Assignment behind an optional Cargo feature. The pilot was
evaluated in #645 using the briefing in
`docs/coordination/adr-031-pilot-evaluation.md` (build-time numbers and p99 scaffold
in PR #773). The owner accepted it for the **whole Rust fleet** on 2026-10-04.

### Pilot outcome against ADR-031's criteria

| ADR-031 criterion | Outcome |
| --- | --- |
| 5/5 RPCs over Connect, gRPC, gRPC-Web | Met (#739, #740) |
| `sdks/server-go` on a generated Connect client | Met (#743) |
| Net-negative LOC | **Missed on a strict count (+995).** +209 when the cfg-gated `http_json.rs` path is counted as retired. Accepted anyway: the excess is one-time first-adopter cost (codegen crate, bridge scaffolding, replacement e2e tests) |
| p99 within ±10% of tonic | **Not yet measured.** Becomes gate G1 below |
| Build-time delta acceptable | Total compile work +1.0%; per-crate warm +15.4%; cold workspace wall clock +54.5% (single sample, 8-core laptop, from worse parallelism behind the codegen `build.rs`). Judged acceptable; re-measured on CI under G3 |
| Kill: pre-1.0 bump breaks the build | Not triggered, **but untested**: main still pins connectrpc 0.7.0 / buffa 0.7.1 while 0.9.x has shipped. Becomes gate G2 |
| Kill: streaming parity | Not triggered (#740) |
| Kill: buffa/prost coexistence | Not triggered (separate crates, no doubled types) |

**LOC accounting rule for later evaluations: retire-as-delete.** Code removed from
the build by a `cfg` gate counts as deleted, so every per-service migration below is
judged the same way.

### Why the whole fleet, not just client-facing services

- **The UI is broken against tonic today** (#758). Analysis, bandit and assignment
  pages cannot talk to M4a/M4b/M1; Flags works only through a temporary gRPC
  translation in the M6 BFF (#766, `GRPC_BRIDGED` in
  `ui/src/app/api/rpc/[module]/[...rpc]/route.ts`).
- **One wire story.** Go services already serve connect-go. Rust-only tonic keeps two
  server stacks, two codegen paths and two sets of interceptors/health/reflection
  conventions. A partial migration would keep both forever.
- **Hand-rolled clients end.** Five SDKs hand-roll Connect JSON against M1; with a
  real Connect server everywhere they can use generated clients.
- **The ecosystem goal.** The Kaizen integration effort standardizes every service on
  ConnectRPC over shared contracts. Rust is the remaining gap.

---

## Decision

Every Rust service binary in this workspace serves **Connect + gRPC + gRPC-Web on one
`connectrpc` Tower listener**, and every Rust-to-service client call uses a
generated `connectrpc` client. `tonic`, `tonic-web`, `tonic-build`, `prost` and the
hand-rolled `http_json.rs` shim are removed from the workspace when the last service
lands. ADR-010's Go half is unchanged.

### 1. Gates before any fan-out PR merges

| Gate | What | Fails if |
| --- | --- | --- |
| **G1** p99 | Run `scripts/loadtest/m1-p99.js` (PR #773) against tonic and the pilot listener | Any unary RPC's Connect p99 regresses by more than 10% vs tonic |
| **G2** churn | Upgrade connectrpc / buffa / connectrpc-build from 0.7 to the current 0.9.x on the pilot | The upgrade needs more than mechanical changes, or breaks a contract test |
| **G3** CI build time | 3-sample CI measurement of the cold workspace build, default vs `connectrpc` | Cold CI wall clock grows by more than 25% with no fix in sight (e.g. splitting the codegen crate) |

If G1 or G2 fails, this ADR returns to the owner before any fan-out: the pilot
stays opt-in and ADR-010 remains authoritative.

### 2. One generated-code crate, per-package modules

`experimentation-proto-connect` grows from `assignment/v1` to every package under
`proto/experimentation/`, one module per package, behind per-package Cargo features
so each service compiles only what it serves or calls. It replaces
`experimentation-proto` once the last prost consumer is gone.

### 3. buffa-native handlers, not long-lived bridges

The pilot bridged buffa types to the existing prost-based handlers. That works for
M1's small messages but scales badly: the nested `Experiment` message in
`ConfigUpdate` was left unbridged, and M5 Rust references the prost crate from 14
files / 136 sites. So:

- **Bridging is a migration step only.** Each service may land behind a bridge, but
  its migration is not complete until its handlers, stores and clients use buffa
  types directly and its prost imports are gone.
- **No hand-written field-by-field conversion for messages over ~10 fields.** Convert
  those call sites to buffa types instead.
- `assert_finite!()` is preserved on every floating-point field crossing the new
  types (CLAUDE.md fail-fast rule).

### 4. Rollout order

Each step is a feature-gated migration with tonic still shippable, then a default
flip, then tonic removal for that crate.

| Order | Service | Why here |
| --- | --- | --- |
| 0 | **M1** Assignment | Upgrade per G2, make `connectrpc` the default, delete `http_json.rs` and tonic paths; migrate M1's tonic client to M4b |
| 1 | **M7** Flags | Unblocks deleting the M6 BFF bridge (#758); 7 RPCs; small proto surface |
| 2 | **M4a** Analysis | Restores UI analysis pages; 10 RPCs; read-heavy |
| 3 | **M4b** Policy | Restores UI bandit pages; 7 RPCs; LMAX core untouched, only the transport edge moves; depends on #749 for a dev service |
| 4 | **M2** Ingest (Rust half) | 8 RPCs; high-throughput event path, so its p99 is re-checked with the G1 harness before the default flips |
| 5 | **M5** Management (Rust) | Largest surface (26 RPCs, 136 prost sites); sequenced last and coordinated with ADR-025's remaining phases (#590) |
| 6 | Workspace cleanup | Remove `experimentation-proto`, tonic, tonic-web, tonic-build, prost; collapse remaining dual listeners |

SDKs migrate in parallel once M1 flips its default: android (connect-kotlin),
ios (connect-swift), web (connect-es), server-python (connect-python), replacing
their hand-rolled JSON paths. `sdks/server-go` is already done.

### 5. Toolchain

`connectrpc`/`buffa` require edition 2024 / MSRV 1.88. The pilot crate overrides the
workspace (edition 2021 / 1.80). Each service crate moves to edition 2024 / 1.88 as
it migrates; the workspace default moves at step 6.

### 6. Pinning and upgrades

Pin connectrpc / buffa / connectrpc-build to an exact minor across the workspace via
`[workspace.dependencies]`. Upgrades are one PR for the whole fleet, never per
service, so all services stay on one runtime version.

---

## Consequences

### Benefits

1. **The M6 BFF becomes a pass-through.** `GRPC_BRIDGED` and its per-request
   decode/re-encode are deleted; every UI page reaches every backend the same way.
2. **One protobuf runtime and one transport stack in Rust** once step 6 lands,
   instead of prost/tonic plus a buffa island.
3. **gRPC-Web, binary Connect and JSON on every service** with no hand-written
   routes, CORS or serde shims.
4. **SDKs on generated clients**, so wire drift becomes a compile error.
5. **Health and reflection** come from `connectrpc-health` / `connectrpc-reflection`
   uniformly instead of per-crate `tonic-health` wiring (#755, #771).

### Trade-offs

1. **Pre-1.0 dependency across the whole fleet.** Every minor bump touches every
   service. Mitigated by G2 and one-PR-for-the-fleet upgrades; still real risk until
   1.0.
2. **Large migration.** Six services, five SDKs, and every prost call site. M5 Rust
   alone is a substantial port.
3. **Coexistence period.** buffa and prost both live in the workspace until step 6.
   The build pays for both codegen paths in the meantime.
4. **Toolchain bump.** Edition 2024 / MSRV 1.88 for every service crate.
5. **Cold-build wall clock** may grow during coexistence (G3 tracks it).
6. **Contracts still live here.** kaizen-rosetta has no Rust generation target yet;
   this ADR keeps codegen on `proto/experimentation/` and does not decide when Rust
   consumes rosetta.

---

## Implementation Details

### Proto Schema

No `.proto` changes. Codegen moves from `tonic-build` to `connectrpc-build` for each
package as its service migrates.

### Crate Layout / Public API

- `experimentation-proto-connect`: per-package modules and features
  (`assignment`, `flags`, `analysis`, `bandit`, `pipeline`, `management`,
  `metrics` for the Go-served M3 client side), each re-exporting buffa types plus the
  generated service trait and client.
- Each service crate: a `connect_server.rs` implementing the generated trait,
  mounted on one listener with health and reflection; the existing tonic server
  stays behind the inverse feature until that service's default flips.

### Integration

| Area | Change |
| --- | --- |
| M6 UI BFF | Delete each `GRPC_BRIDGED` entry as its backend flips; plain pass-through after M7/M4a/M4b/M1 |
| Go services | Unchanged; connect-go already interoperates over all three protocols |
| Infra | One port per service; health checks move to Connect health; Cloud Map / load-balancer configs drop the separate HTTP/JSON port where M1 had one |
| CI | `connectrpc-build` in place of `tonic-build`; cross-protocol contract tests per service |

---

## Validation

### Unit Tests / Proptest Invariants

- Round-trip tests for every request/response type while a bridge exists.
- Existing proptest suites in experimentation-stats are unaffected (no transport
  code there).

### Integration / Contract Tests

- Each service's existing contract tests run over Connect, gRPC and gRPC-Web before
  its default flips (the bar ADR-031 set for M1).
- Pair integration suites stay green at every step.
- Each SDK gets a conformance test against M1 modeled on
  `sdks/server-go/connect_pilot_e2e_test.go`.

### Performance

- G1 harness extended per service; each default flip requires p99 within ±10% of
  that service's tonic baseline.

---

## Dependencies

- **ADR-010** (ConnectRPC for Go, tonic for Rust): **superseded for Rust** on
  acceptance. Go half unchanged.
- **ADR-031** (M1 pilot): evidence base; marked Accepted and Implemented with the
  outcome recorded.
- **ADR-025** (M5 Rust port): step 5 coordinates with its remaining phases (#590).
- **ADR-006** (Cargo workspace): `experimentation-proto` is eventually replaced by
  `experimentation-proto-connect`.
- **Enables**: deleting the M6 BFF gRPC bridge (#758); generated Connect clients in
  all SDKs.

---

## Rejected Alternatives

| Alternative | Reason Rejected |
|-------------|-----------------|
| **Client-facing services only (M1, M7, M4a, M4b)** | Leaves tonic and prost in the workspace indefinitely for M2 and M5; two stacks forever. The owner chose the whole fleet |
| **Reject; keep tonic and extend the BFF gRPC bridge** | Fixes the UI with no Rust changes, but keeps hand-rolled SDK clients and a translating BFF, and keeps the Rust/Go split |
| **`tonic-web` everywhere** | Adds gRPC-Web only; still no Connect protocol, so SDKs and the BFF keep special cases |
| **Long-lived prost⇄buffa bridges** | Field-by-field conversion code grows with message size; the pilot already deferred `Experiment` for this reason |
| **Big-bang migration of all services at once** | No per-service rollback; contradicts the parallel-implementation precedent of ADR-024/025 |
| **Wait for connectrpc 1.0** | Keeps the UI broken and the hand-rolled tax; G2 plus fleet-wide pinning manages the 0.x risk |

---

## References

- ADR-031 and `docs/coordination/adr-031-pilot-evaluation.md` (pilot briefing; build-time and p99 scaffold in PR #773)
- #645 (pilot decision), #648 (pilot goal), #718 (pilot tracker), #758 (UI bridge), #766 (interim BFF gRPC bridge)
- [anthropics/connect-rust](https://github.com/anthropics/connect-rust), [`buffa`](https://github.com/anthropics/buffa)
