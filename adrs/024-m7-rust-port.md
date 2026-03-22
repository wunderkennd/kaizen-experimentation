# ADR-024: Port M7 Feature Flag Service from Go to Rust

- **Status**: Proposed
- **Date**: 2026-03-20
- **Author**: Agent-7 (Feature Flags) / Platform Architecture
- **Supersedes**: Partially supersedes ADR-001 language selection (for M7 only)

## Context

The Feature Flag Service (M7) is the simplest Go service in the platform — 8 features, 1 external dependency (CGo hash bridge), and a well-bounded API surface (CRUD, EvaluateFlag, EvaluateFlags, PromoteToExperiment, reconciler). It exists in Go primarily because ConnectRPC was the initial Go-side RPC framework choice (ADR-010), and the hash bridge was simpler to build as CGo FFI than as a standalone Rust service at the time.

M7's Go implementation carries three ongoing costs:

**1. The `experimentation-ffi` crate exists solely for M7.** The FFI crate (`crates/experimentation-ffi/`) uses cbindgen to generate C headers from `experimentation-hash`, which M7 then calls via CGo. This is the only consumer of the FFI crate. The entire CGo build toolchain in CI — the cross-compilation setup, the `!cgo || !has_ffi` build tag fallback in the Go SDK, the 10,000-vector parity validation between Rust and CGo — exists because of this single dependency. Eliminating the FFI crate removes a build-time complexity layer, a CI matrix dimension, and a category of potential hash divergence bugs.

**2. CGo has a non-trivial call overhead and debugging cost.** The 280ns/call overhead is acceptable in production, but CGo complicates debugging (stack traces cross the language boundary), profiling (Go's pprof cannot see into CGo calls), and memory safety analysis (Go's garbage collector cannot track Rust-allocated memory). At 20K rps load test, the CGo bridge accounts for ~5.6ms/second of CPU time — small but not zero.

**3. M7 is the only Go service that needs hash parity with Rust.** M3 (Metric Computation) and M5 (Experiment Management) have no hash dependency. M2-Orch (Event Pipeline orchestration) has no hash dependency. If M7 were in Rust, hash parity would be trivially guaranteed by importing `experimentation-hash` as a workspace crate dependency — the same source code, compiled once.

The Go SDK's Server-Go implementation has a pure-Go MurmurHash3 fallback (behind the `!cgo || !has_ffi` build tag) for environments where CGo is unavailable. This fallback was validated against Rust test vectors and works correctly. The SDK's hash needs are independent of M7's language — the SDK calls M1 (Rust) for authoritative assignment and uses local hashing only as a cache/fallback.

M7's feature set is small and stable:

| Feature | Complexity | Go Implementation |
|---------|-----------|-------------------|
| Boolean/String/Numeric/JSON flag CRUD | Low | ConnectRPC handlers + PostgreSQL |
| Percentage rollout (monotonic) | Low | Hash-based bucketing (CGo bridge) |
| Multi-variant flags | Low | Traffic fraction allocation |
| PromoteToExperiment | Medium | Creates experiment via M5 RPC |
| Flag audit trail | Low | PostgreSQL insert on each mutation |
| Stale flag detection | Low | SQL view (>90 days unchanged at 100%) |
| Experiment reconciler | Medium | Kafka consumer + M5 RPC to resolve flag |
| EvaluateFlag/EvaluateFlags | Low | Hash + config lookup |

Total: ~2,500 lines of Go (excluding generated proto code), 13 chaos tests, 20K rps load test passing.

## Decision

Port M7 from Go to Rust. The new `experimentation-flags` service binary joins the existing Cargo workspace, directly depending on `experimentation-hash`, `experimentation-proto`, and `experimentation-core`. Delete the `experimentation-ffi` crate and its CGo build infrastructure.

### Architecture

```
                    BEFORE                          AFTER
                    ------                          -----
experimentation-hash                    experimentation-hash
        │                                       │
   [cbindgen]                              [direct dep]
        │                                       │
experimentation-ffi (C headers)         experimentation-flags (Rust binary)
        │                                       │
   [CGo bridge]                            [tonic-web]
        │                                       │
   M7 Go binary                           M7 Rust binary
   (connect-go)                           (tonic + tonic-web)
                                          JSON HTTP compatible
```

### Implementation Plan

**Phase 1: Rust service scaffold** (~1 week)
- Create `crates/experimentation-flags/` in the workspace.
- Implement tonic service with tonic-web for JSON HTTP mode (matching ConnectRPC JSON wire format that M6 and SDKs already use).
- PostgreSQL via sqlx (async, compile-time checked queries) — migrate the 5 SQL migration files.
- Flag CRUD: CreateFlag, GetFlag, ListFlags, UpdateFlag, DeleteFlag.
- EvaluateFlag/EvaluateFlags: direct `experimentation_hash::murmur3_x86_32()` call — no FFI.

**Phase 2: Business logic** (~1 week)
- Percentage rollout with monotonic guarantee (same algorithm, now native Rust).
- Multi-variant traffic fraction allocation.
- PromoteToExperiment: tonic client calling M5's management service.
- Audit trail: PostgreSQL insert on each mutation (same schema).
- Stale flag detection: SQL view, unchanged.

**Phase 3: Async consumers and reconciler** (~3 days)
- Kafka consumer for experiment conclusion events (rdkafka crate, same as M4b's reward consumer).
- Reconciler: on CONCLUDED event, resolve the associated flag via internal RPC.

**Phase 4: Validation and cutover** (~3 days)
- Port all 13 chaos tests (kill -9 under load, restart recovery).
- Port k6 load test (20K rps target, p99 < 10ms).
- Wire-format contract tests: verify JSON responses match the Go service exactly for all existing M6 and SDK consumers.
- Shadow traffic: run Rust M7 alongside Go M7, compare responses for 24 hours.
- Delete `experimentation-ffi` crate, CGo bridge, and related CI configuration.

### Wire-Format Compatibility

M6 and SDKs communicate with M7 via JSON HTTP (ConnectRPC JSON mode). tonic-web produces identical JSON wire format for the same proto definitions. The endpoint pattern (`POST /experimentation.flags.v1.FeatureFlagService/EvaluateFlag` with `Content-Type: application/json`) is preserved. No client changes required.

### What Gets Deleted

| Artifact | Lines | Purpose | Replacement |
|----------|-------|---------|-------------|
| `crates/experimentation-ffi/` | ~400 | cbindgen C header generation | Direct crate import |
| `services/flags/` (Go) | ~2,500 | M7 Go implementation | `crates/experimentation-flags/` (Rust) |
| CGo build tags in CI | ~50 | Cross-compilation for CGo | Removed entirely |
| `just test-flags-cgo` recipe | ~10 | CGo parity validation | Unnecessary — same binary |
| Go SDK CGo bridge | ~150 | Hash computation in Server-Go SDK | Pure-Go fallback becomes primary (already validated) |

Total removed: ~3,100 lines of Go/C/build config.
Total added: ~2,000 lines of Rust (estimate — Rust is more concise for this workload due to sqlx compile-time checks and tonic codegen).

Net: ~1,100 fewer lines and elimination of an entire language boundary.

## Consequences

### Positive

- **Eliminates the `experimentation-ffi` crate entirely** — the only reason it exists is M7's CGo bridge. This removes a build-time dependency, a CI matrix dimension, and a class of hash divergence bugs.
- **Hash parity is guaranteed by construction** — `experimentation-flags` imports `experimentation-hash` as a workspace dependency. Same source, same binary, zero parity validation needed.
- **Simplified CI** — no CGo cross-compilation, no cbindgen step, no CGo-specific test targets. The `just test-flags-cgo` recipe and its 10K-vector validation are deleted.
- **Uniform Rust toolchain** for all hot-path services (M1, M2, M4a, M4b, M7). Profiling, debugging, and observability use a single tool ecosystem.
- **Lower p99 latency** — Rust's EvaluateFlag will be faster than Go+CGo due to eliminated bridge overhead and allocation-free hash path. Expected p99 < 5ms (vs current 10ms target).
- **Workspace-level dependency management** — M7's dependencies (tonic, sqlx, rdkafka) are already used by other Rust services, so the workspace `Cargo.toml` already pins compatible versions.

### Negative

- **Rewrite effort**: ~3 weeks for a service that already works. This is opportunity cost against Phase 5 ADR implementation.
- **Loss of Go ecosystem tools for M7**: Go's pprof, race detector, and net/http/pprof are well-suited for M7's workload. Rust equivalents exist (tokio-console, miri, criterion) but require different workflows.
- **rdkafka dependency**: M7's reconciler needs a Kafka consumer. `rdkafka` (librdkafka Rust bindings) is already a dependency of M4b, so this adds no new external dependency to the workspace — but it does add librdkafka linkage to M7's binary.
- **sqlx compile-time query checking** requires a live database connection during `cargo check`. This is already the case for M4a's `AnalysisStore`; the same `DATABASE_URL` setup applies.

### Risks

- **Wire-format regression**: If tonic-web's JSON serialization differs from connect-go in any edge case (e.g., proto3 zero-value handling, int64 string encoding), M6 or SDKs could break. Mitigation: 100% wire-format contract test coverage before cutover, plus 24-hour shadow traffic comparison.
- **Reconciler Kafka consumer offset management**: The Go service uses Sarama for Kafka; the Rust service will use rdkafka. Consumer group offset semantics must be verified during migration to prevent message loss or duplication. Mitigation: deploy Rust M7 with a new consumer group, process in parallel for 24 hours, then decommission Go consumer group.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Keep M7 in Go (status quo) | No work; service is stable | FFI crate, CGo bridge, and parity validation burden persist indefinitely | The ongoing maintenance cost exceeds the one-time rewrite cost |
| Pure-Go hash (eliminate CGo, keep Go) | Removes CGo bridge; simpler | Requires maintaining a separate Go MurmurHash3 implementation with perpetual parity risk against Rust | Still two implementations to maintain; parity risk never goes to zero |
| Shared library (.so/.dylib) instead of CGo | Lower overhead than CGo | Still a language boundary; deployment complexity (library versioning); dlopen failure modes | Same fundamental problem as CGo, slightly different failure mode |
| WASM bridge (compile Rust hash to WASM, call from Go) | No CGo; portable | WASM call overhead (~10μs per invocation); requires wasm runtime in Go binary | Higher overhead than CGo; adds a dependency (wasmtime-go or wazero) |

## References

- ADR-001 (Language selection — this ADR narrows the Go surface)
- ADR-010 (ConnectRPC — tonic-web replaces connect-go for M7)
- `crates/experimentation-ffi/` — the artifact this ADR eliminates
- M7 load test results: 20K rps, p99 < 10ms (PR #129)
- M7 chaos tests: 13 tests (PR #129)
