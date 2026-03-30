# Agent-7: Feature Flag Service

You own Module 7 (Feature Flag Service). Your Phase 5 work is focused entirely on ADR-024: porting M7 from Go to Rust and eliminating the `experimentation-ffi` crate.

Language: Go → **Rust** (ADR-024)
Current: `services/flags/` (Go)
Target: `crates/experimentation-flags/` (Rust)
Service port: 50057

## Phase 5 ADR Responsibilities

### Primary Owner — ADR-024 (M7 Rust Port)

**Phase 1: Scaffold + CRUD** (Sprint 5.0)
- Create `crates/experimentation-flags/` in workspace.
- tonic service with tonic-web for JSON HTTP mode (ConnectRPC JSON wire-format compatibility).
- PostgreSQL via sqlx (async, compile-time checked queries).
- Migrate 5 SQL migration files from Go `database/sql` to sqlx.
- Flag CRUD RPCs: CreateFlag, GetFlag, ListFlags, UpdateFlag, DeleteFlag.

**Phase 2: Business Logic** (Sprint 5.1)
- Percentage rollout with monotonic guarantee — same algorithm, now native Rust via direct `experimentation_hash::murmur3_x86_32()` call. No FFI.
- Multi-variant traffic fraction allocation.
- PromoteToExperiment: tonic client calling M5.
- Audit trail: PostgreSQL insert on each mutation.
- Stale flag detection: SQL view (>90 days unchanged at 100%).

**Phase 3: Kafka + Reconciler** (Sprint 5.1)
- Kafka consumer for experiment conclusion events (rdkafka crate).
- Reconciler: on CONCLUDED event, resolve the associated flag.

**Phase 4: Validation + Cutover** (Sprint 5.1)
- Port all 13 chaos tests (kill -9 under load, restart recovery).
- Port k6 load test (20K rps target, p99 < 5ms — improvement from current 10ms).
- Wire-format contract tests: verify JSON responses match Go service for all RPCs.
- Shadow traffic: run Rust M7 alongside Go M7 for 24 hours, compare responses.
- **Delete `experimentation-ffi` crate** and all CGo build infrastructure.
- Remove `just test-flags-cgo` recipe.
- Update Go SDK Server-Go: pure-Go MurmurHash3 fallback becomes primary.

## Coding Standards
- Run `cargo test -p experimentation-flags` before creating PR.
- Wire-format parity: every RPC must produce byte-identical JSON to the Go implementation.
- Performance target: p99 < 5ms at 20K rps (verify with k6).
- Chaos test parity: all 13 existing chaos tests must pass on Rust service.
- sqlx compile-time query checking: `DATABASE_URL` must be set for `cargo check`.
## Work Tracking
Find your assigned work via GitHub Issues:
```bash
gh issue list --label "agent-7" --state open
gh issue view <number>
```
When starting work, comment on the Issue. When creating a PR, include `Closes #<number>`.
If blocked, add the `blocked` label and comment explaining the blocker.

## Dependencies on Other Agents
- Agent-Proto: No proto changes needed (existing `flags/v1/` package unchanged).
- Agent-5 (M5): PromoteToExperiment RPC target. StreamConfigUpdates consumer pattern (same as M1).
- Agent-6 (M6): Wire-format contract tests — M6 is the primary consumer.
- Agent-1 (M1): After FFI crate deletion, verify Go SDK pure-Go fallback still works.

## What Gets Deleted After Cutover
| Artifact | Location | Lines |
| --- | --- | --- |
| `experimentation-ffi` crate | `crates/experimentation-ffi/` | ~400 |
| M7 Go service | `services/flags/` | ~2,500 |
| CGo build tags in CI | `.github/workflows/` | ~50 |
| `just test-flags-cgo` | `justfile` | ~10 |
| Go SDK CGo bridge | `sdks/server-go/internal/ffi/` | ~150 |

## Contract Tests to Write
- M7 (Rust) ↔ M6: All flag RPCs wire-format parity (100% coverage)
- M7 (Rust) ↔ M5: PromoteToExperiment roundtrip
- M7 (Rust) ↔ Go M7: Shadow traffic comparison (24h, response-level diff)
