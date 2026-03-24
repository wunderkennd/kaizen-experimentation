# ADR-024: M7 Feature Flags Service — Rust Port (Unconditional)

**Status**: Accepted and Implemented
**Date**: 2026-03-24
**Deciders**: Agent-7
**Cluster**: F — Language Migration

---

## Context

M7 (Feature Flags) was implemented in Go and depended on the `experimentation-ffi` CGo bridge to call into `experimentation-hash` (MurmurHash3) for deterministic user bucketing. This created three problems:

1. **CGo build complexity**: cross-compilation required a C toolchain, bloated the Docker image, and added a native `.so` dependency that had to be versioned alongside the Rust crate.
2. **Hash divergence risk**: the CGo bridge is a seam where a Go/Rust version mismatch could silently cause bucket assignments in M7 to diverge from M1 (Assignment), breaking experiment integrity guarantees.
3. **Operational overhead**: a second language runtime (Go GC) alongside the Rust workers increased memory and added GC pause unpredictability.

Unlike M5 (ADR-025), M7's migration is **unconditional**: there is no algorithmic complexity or M5 lifecycle integration that would make a Go-first approach preferable, and M7 is the primary consumer of `experimentation-hash`.

---

## Decision

Port M7 from Go to Rust as `crates/experimentation-flags` (binary crate with a library for testability). Delete the Go service (`services/flags/`) and the CGo bridge (`crates/experimentation-ffi/`).

---

## Architecture

### Crate Layout

```
crates/experimentation-flags/
  src/
    lib.rs          — module declarations
    main.rs         — startup: PgPool, AuditStore, Reconciler, Kafka, gRPC
    config.rs       — FlagsConfig from env vars
    store.rs        — FlagStore (sqlx + PgPool, async CRUD)
    grpc.rs         — tonic FeatureFlagService impl + serve()
    audit.rs        — AuditStore (flag_audit_trail, stale flag detection)
    reconciler.rs   — Polling reconciler (M5 GetExperiment, ResolutionAction)
    admin.rs        — Internal axum HTTP: /internal/flags/{audit,stale,promoted,resolve}
    kafka.rs        — rdkafka lifecycle consumer (experiment_lifecycle topic)
  tests/
    contract_test.rs — Wire-format contract tests vs Go M7 (13 tests)
    chaos_test.rs    — Fault injection tests (13 tests, ported from Go chaos suite)
```

### Dependencies Added

| Crate | Version | Purpose |
|-------|---------|---------|
| `tonic` | 0.12 (workspace) | gRPC server |
| `tonic-web` | 0.12 (workspace) | gRPC-Web / HTTP1 support |
| `sqlx` | 0.8 (workspace) | Async PostgreSQL (runtime-tokio, tls-rustls) |
| `axum` | 0.7 | Admin HTTP server (separate port) |
| `rdkafka` | 0.36 (workspace) | Kafka lifecycle consumer |

### Proto Mapping

`proto/experimentation/flags/v1/flags_service.proto` defines all RPCs. The Go comment "Uses CGo" was not updated in-proto (proto is language-agnostic), but the Rust implementation uses `experimentation_hash::bucket()` directly — no CGo.

### Bucketing Parity

`evaluate_flag()` in `grpc.rs` calls `experimentation_hash::bucket(user_id, &f.salt, 10_000)` directly. The key format `"{user_id}\x00{salt}"` is identical to the Go pure-Go fallback (`hash_pure.go`) — this is validated by the `evaluation_bucket_matches_go_parity_vector` contract test.

---

## Implementation Phases

### Phase 1 — Scaffold + Flag CRUD + EvaluateFlag

- `FlagStore`: `create_flag`, `get_flag`, `update_flag`, `delete_flag`, `list_flags` (cursor-based, base64 token), `get_all_enabled_flags`.
- `FlagsServiceHandler`: tonic `FeatureFlagService` implementation.
- `validate_flag()`: name required, type not unspecified, `rollout_percentage` in [0,1], boolean default_value must be "true"/"false", variant fractions must sum to 1.0.
- `evaluate_flag()`: direct `experimentation_hash::bucket()` call — no CGo.
- `EvaluateFlagResponse`, `EvaluateFlagsResponse` RPCs.
- `serve()`: `tonic-web` enabled for gRPC-Web (HTTP/1.1) support.
- **13 wire-format contract tests**: proto binary round-trip, zero-value handling, enum encoding, pagination token, evaluation parity vectors.

### Phase 2 — PromoteToExperiment + Audit + Admin HTTP

- `PromoteToExperiment` RPC: fetches flag, builds `Experiment` proto from flag variants, delegates to M5 `CreateExperiment`, links `promoted_experiment_id` back to the flag.
- `apply_type_config()`: sets `InterleavingConfig`, `BanditConfig`, `SessionConfig` based on `ExperimentType`.
- `AuditStore`: inserts into `flag_audit_trail`; queries stale flags (100% rollout, unpromoted, unchanged > N days).
- Per-mutation audit: "create", "enable", "disable", "rollout_change", "update", "promote_to_experiment" — non-fatal on failure.
- `AdminState` + axum router: `GET /internal/flags/audit`, `GET /internal/flags/stale`, `GET /internal/flags/promoted`, `POST /internal/flags/resolve`.

### Phase 3 — Kafka Reconciler + Polling Reconciler

- `Reconciler`: polls M5 `GetExperiment` for all promoted-but-unresolved flags; auto-resolves on `CONCLUDED` or `ARCHIVED` state.
- `ResolutionAction`: `RolloutFull` (set 100% + enable), `Rollback` (set 0% + disable), `Keep` (mark resolved_at only).
- `FlagStore::resolve_flag()`: one-shot SQL UPDATE per action.
- `FlagStore::get_promoted_flags()`: lists `promoted_experiment_id IS NOT NULL` flags.
- Kafka consumer (`rdkafka`, consumer group `flags-reconciler`, topic `experiment_lifecycle`): triggers ad-hoc reconciliation on lifecycle events.
- `main.rs`: shared `PgPool` across FlagStore / AuditStore / gRPC, spawns Reconciler + Kafka consumer as tokio tasks.

### Phase 4 — Chaos Tests + FFI Deletion

- 13 chaos tests (`tests/chaos_test.rs`): `MockFlagStore` + `ChaosStore` per-operation fault injection, ported from `services/flags/internal/handlers/chaos_test.go`.
- Scenarios covered: partial-write prevention, audit non-fatality, concurrent CRUD under races, store recovery, task leak prevention, mock restart persistence.
- `crates/experimentation-ffi/` deleted (CGo bridge eliminated).
- `services/flags/` Go service deleted.
- `Cargo.toml` workspace updated to remove `experimentation-ffi` member.
- Go SDK: `hash_cgo.go` deleted, `hash_pure.go` promoted to primary (build tag removed).

---

## Consequences

### Positive

- **No CGo**: Rust-only build, no C toolchain required, no `.so` versioning.
- **Hash parity guaranteed by construction**: `experimentation_hash::bucket()` is the same call site as M1 — divergence is structurally impossible.
- **26 tests**: 13 contract tests (wire-format + bucketing parity) + 13 chaos tests (fault injection).
- **Unified runtime**: eliminates Go GC pauses in the flags hot path.
- **tonic-web**: HTTP/1.1 gRPC-Web support for browser/CDN-compatible clients at no extra configuration.

### Negative / Trade-offs

- Go team loses familiarity advantage; oncall needs Rust debugging skills for M7 incidents.
- `services/flags/` Go code is deleted and not recoverable from this branch (preserved in git history).

### Risks Mitigated

- Hash divergence: eliminated by direct crate dependency.
- CGo panic propagation: eliminated (no CGo).
- Partial-write on CreateFlag: sqlx transaction ensures atomicity.

---

## Rejected Alternatives

| Alternative | Reason Rejected |
|-------------|----------------|
| Keep Go, fix CGo | Doesn't fix hash divergence risk; CGo build cost remains |
| Keep Go, call Rust via gRPC sidecar | Adds network hop + another process to M7 pod |
| Conditional port like M5 (ADR-025) | M7 has no ADR-level feature complexity gating decision |

---

## SQL Migrations Referenced

- `sql/migrations/002_feature_flags.sql` — `feature_flags`, `flag_variants` tables
- `sql/migrations/003_flag_audit_trail.sql` — `flag_audit_trail`, `stale_flags` view
- `sql/migrations/004_flag_experiment_linkage.sql` — `promoted_experiment_id`, `promoted_at` columns
- `sql/migrations/005_flag_resolved_at.sql` — `resolved_at` column

---

## Test Coverage

| Test Suite | Count | What It Verifies |
|------------|-------|-----------------|
| `contract_test.rs` | 13 | Proto binary round-trip, zero values, enum encoding, pagination tokens, bucketing parity vs Go |
| `chaos_test.rs` | 13 | Fault injection: partial state prevention, audit non-fatality, concurrent safety, store recovery |
| **Total** | **26** | All pass on `cargo test -p experimentation-flags` |
