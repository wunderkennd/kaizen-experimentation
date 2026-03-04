# Agent-7 Quickstart: M7 Feature Flag Service (Go)

## Your Identity

| Field | Value |
|-------|-------|
| Module | M7: Feature Flag Service |
| Language | Go (with CGo bridge to Rust hash library) |
| Go packages you own | `services/flags/` |
| Proto package | `experimentation.flags.v1` |
| Infra you own | PostgreSQL flag configs (shared schema with M5) |
| Primary SLA | `EvaluateFlag` p99 < 10ms, `PromoteToExperiment` creates experiment atomically |

## Read These First (in order)

1. **Design doc v5.1** — Sections 11 (M7 specification), 2.6 (SDK provider abstraction — flags share the SDK)
2. **ADR-007** (SDK provider abstraction), **ADR-010** (ConnectRPC)
3. **Proto files** — `flags_service.proto`, `experiment.proto` (for PromoteToExperiment)
4. **PostgreSQL DDL** — `sql/001_schema.sql` (flag tables would be added alongside experiment tables)

## The Key Relationship: Flags → Experiments

Feature flags are the on-ramp to experimentation. Teams start with a boolean flag ("show new player"), add percentage rollout ("show to 10% of users"), and when they want to measure impact, they "promote" the flag to a tracked experiment. This is M7's signature capability: `PromoteToExperiment` creates an experiment in M5 from an existing flag configuration, preserving the flag's targeting rules and variant definitions.

## Who You Depend On (upstream)

| Module | What you need from them | Blocks you? |
|--------|------------------------|-------------|
| M1 (Agent-1) | `experimentation-hash` Rust crate via CGo/FFI for deterministic flag evaluation | **Yes for rollout consistency** — percentage rollouts must use the same hash as M1. Use the C headers from `experimentation-ffi`. |
| M5 (Agent-5) | `CreateExperiment` API for PromoteToExperiment | Yes for flag graduation. Mock initially. |

## Who Depends on You (downstream)

| Module | What they need from you | Impact if you're late |
|--------|------------------------|----------------------|
| Client SDKs | `EvaluateFlag` / `EvaluateFlags` RPCs | SDKs can't evaluate flags without you. But SDK teams can use M1 for experiment assignments independently. |
| M6 (Agent-6) | Flag management UI pages | UI delayed; not critical path. |

## Your First PR: Boolean Flag CRUD + Evaluation

**Goal**: Create, evaluate, and update a boolean feature flag with percentage rollout.

```
services/flags/
├── cmd/
│   └── main.go              # connect-go server
├── internal/
│   ├── handlers/
│   │   ├── flag.go          # CRUD RPCs
│   │   ├── evaluate.go      # EvaluateFlag, EvaluateFlags
│   │   └── promote.go       # PromoteToExperiment
│   ├── hash/
│   │   └── bridge.go        # CGo bridge to experimentation-ffi
│   └── store/
│       └── postgres.go      # Flag persistence
├── cgo/
│   ├── experimentation_ffi.h  # Copied from Rust cbindgen output
│   └── bridge.c               # Thin C wrapper (if needed)
```

**Acceptance criteria**:
- `CreateFlag` with `type: BOOLEAN`, `default_value: "false"`, `rollout_percentage: 0.1`.
- `EvaluateFlag` for user "user_123" → deterministic result (same user always gets same result).
- Changing `rollout_percentage` from 0.1 to 0.2 → users in the original 10% still get `true` (monotonic rollout).
- Hash output from CGo bridge matches Rust native output for all 10,000 test vectors.
- `PromoteToExperiment` creates an experiment in M5 with matching variants and targeting.

**Why this first**: Feature flags are the simplest, most immediately useful capability. A working flag service with CGo hash bridge validates the cross-language interop that underpins the entire platform.

## Phase-by-Phase Deliverables

### Phase 0 (Week 1)
- [ ] Go module skeleton with connect-go server
- [ ] CGo bridge to `experimentation-ffi` (compile Rust crate, link C headers)
- [ ] Hash test vector validation through CGo bridge

### Phase 1 (Weeks 2–7)
- [ ] Boolean flag CRUD
- [ ] String, numeric, JSON flag types
- [ ] Percentage rollout (MurmurHash3 via CGo)
- [ ] Monotonic rollout: increasing percentage never removes users from treatment
- [ ] Multi-variant flags with traffic fractions
- [ ] Targeting rule integration (shared with M5)
- [ ] `EvaluateFlag` + `EvaluateFlags` (bulk) RPCs
- [ ] Flag enable/disable toggle
- [ ] `PromoteToExperiment` RPC: create experiment in M5, link flag to experiment

### Phase 2 (Weeks 6–11)
- [ ] Flag audit trail (who changed what, when)
- [ ] Flag dependency tracking (which flags reference which targeting rules)
- [ ] Stale flag detection: flags unchanged for >90 days with 100% rollout → suggest cleanup

### Phase 3 (Weeks 10–17)
- [ ] PromoteToExperiment for all experiment types (not just A/B — support interleaving, session-level, QoE)
- [ ] Flag-experiment linkage: when promoted experiment concludes, auto-update flag based on winner

### Phase 4 (Weeks 16–22)
- [ ] Load test: p99 < 10ms for EvaluateFlag at 20K rps
- [ ] CGo overhead measurement: verify hash call < 1μs per evaluation
- [ ] Concurrent flag update test: 50 simultaneous updates, verify no race conditions

## Local Development

```bash
# Build the Rust FFI library first
cd crates
cargo build --package experimentation-ffi --features ffi
# Output: target/debug/libexperimentation_ffi.{so,dylib,dll}
# Headers: target/experimentation_ffi.h

# Copy artifacts for CGo
cp target/debug/libexperimentation_ffi.* ../services/flags/cgo/
cp target/experimentation_ffi.h ../services/flags/cgo/

# Build and test Go service
cd services/flags
CGO_ENABLED=1 \
CGO_LDFLAGS="-L./cgo -lexperimentation_ffi" \
CGO_CFLAGS="-I./cgo" \
go test -race -cover ./...

# Run server
CGO_ENABLED=1 \
CGO_LDFLAGS="-L./cgo -lexperimentation_ffi" \
POSTGRES_DSN=postgres://localhost/experimentation \
go run cmd/main.go
```

## Testing Expectations

- **Hash parity**: Run all 10,000 test vectors through the CGo bridge. Every single one must match the Rust native output. This is your highest-priority test.
- **Monotonic rollout**: Create a flag at 10% rollout. Record which of 10,000 test users get `true`. Increase to 20%. All original `true` users must still get `true`. New `true` users must only come from the previously-`false` pool.
- **PromoteToExperiment**: Create a flag with 3 variants and targeting. Promote. Verify the resulting experiment in M5 has matching variants, traffic fractions, and targeting rule.
- **Concurrent evaluation**: 100 goroutines evaluating the same flag simultaneously. No data races (go test -race).

## Common Pitfalls

1. **CGo overhead**: CGo calls have ~100ns overhead per call. For bulk evaluation (`EvaluateFlags` with 50 flags), batch the hash computations — don't call CGo 50 times. Consider computing all hashes in a single CGo call with a batch API.
2. **Monotonic rollout implementation**: Don't use `hash % 100 < rollout_pct`. Use `hash % 10000 < rollout_pct * 10000`. The finer granularity (10,000 buckets matching layer bucket count) ensures consistency with M1 assignment.
3. **PromoteToExperiment atomicity**: This creates an experiment in M5 and updates the flag to reference it. If M5 creation succeeds but flag update fails, you have an orphaned experiment. Use a transaction or saga pattern.
4. **Flag type validation**: A BOOLEAN flag's `default_value` must be "true" or "false". A NUMERIC flag's values must parse as float64. Validate at creation time, not evaluation time.
5. **CGo in CI**: GitHub Actions runners need the Rust FFI library built and available. Your CI job must depend on the Rust build job and copy the `.so`/`.dylib` artifact before running `go test`.
