# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build, Test, and Lint Commands

This project uses `just` as its primary task runner. Run `just --list` to see all recipes.

### Full setup (first time)
```bash
cp .env.example .env
just setup    # infra + codegen + deps + seed + test
```

### Testing
```bash
just test                          # All: Rust + Go + TS + hash parity
just test-rust                     # cd crates && cargo test --workspace
just test-go                       # cd services && go test -race -cover ./...
just test-ts                       # cd ui && npm test -- --passWithNoTests
just test-hash                     # Cross-language hash parity (10K vectors)
just test-crate experimentation-hash  # Single Rust crate
just test-integration              # Go integration tests against Docker infra
just test-flags-cgo                # Build Rust FFI + run Go CGo parity tests
```

### Linting
```bash
just lint                          # All linters
just lint-rust                     # cd crates && cargo clippy --workspace --all-features -- -D warnings
just lint-go                       # cd services && go vet ./...
just lint-ts                       # cd ui && npm run lint
just lint-proto                    # cd proto && buf lint
just fmt                           # Format Rust code
just fmt-check                     # Check Rust formatting without modifying
```

### Benchmarks
```bash
just bench                         # All Rust benchmarks
just bench-crate experimentation-stats  # Single crate
```

### Code generation (protobuf)
```bash
just codegen                       # Generate Go + TypeScript from proto
```
Rust proto types are auto-generated via `tonic-build` during `cargo build`.

### Infrastructure
```bash
just dev                           # Start Docker infra + load seed data
just infra                         # Start Docker infra only
just infra-down                    # Stop (preserve volumes)
just infra-reset                   # Stop + destroy volumes
just seed                          # Load seed data into Postgres
just db                            # Open psql shell
just monitoring                    # Start Grafana + Prometheus + Jaeger
```

### Golden files (statistical tests)
```bash
UPDATE_GOLDEN=1 cargo test --workspace   # Regenerate golden files after intentional changes
```

## Architecture

SVOD experimentation platform with 7 modules across 3 languages:

| Module | Language | Purpose |
|--------|----------|---------|
| M1 Assignment | Rust | Deterministic user bucketing, interleaving, bandit arm delegation |
| M2 Pipeline | Rust + Go | Event ingestion → validation → dedup → Kafka |
| M3 Metrics | Go + Spark SQL | Metric computation orchestration |
| M4a Analysis | Rust | Statistical tests (t-test, mSPRT, GST, CUPED, bootstrap) |
| M4b Bandit | Rust | Thompson Sampling, LinUCB, LMAX single-threaded policy core |
| M5 Management | Go | Experiment CRUD, lifecycle state machine, guardrail auto-pause |
| M6 UI | TypeScript | Next.js dashboards (UI only — no statistical computation) |
| M7 Flags | Go | Feature flags with experiment promotion via CGo hash bridge |

### Rust workspace (13 crates in `crates/`)

Layered dependency structure — lower layers cannot depend on upper layers:

1. **Foundation**: `experimentation-core` (errors, timestamps), `experimentation-hash` (MurmurHash3), `experimentation-proto` (generated types)
2. **Algorithms**: `experimentation-stats`, `experimentation-bandit`, `experimentation-interleaving`
3. **Infrastructure**: `experimentation-ingest` (validation, Bloom filter dedup), `experimentation-ffi` (C headers for CGo)
4. **Services** (binaries): `experimentation-assignment`, `experimentation-pipeline`, `experimentation-analysis`, `experimentation-policy`

### Go services (`services/`)

Four services: `management/`, `metrics/`, `flags/`, `orchestration/`. All use ConnectRPC (`connectrpc.com/connect`) for RPC handlers.

### Proto schema (`proto/experimentation/`)

All inter-service contracts defined in protobuf. Packages: `common/v1/`, `assignment/v1/`, `pipeline/v1/`, `metrics/v1/`, `analysis/v1/`, `bandit/v1/`, `management/v1/`, `flags/v1/`. The `buf` toolchain enforces lint rules and breaking change detection.

### Key infrastructure

- **Kafka topics**: `exposures`, `metric_events`, `reward_events`, `qoe_events`, `guardrail_alerts` (config in `kafka/topic_configs.sh`)
- **PostgreSQL**: Schema in `sql/migrations/001_schema.sql`, seed data in `scripts/seed_dev.sql`
- **Delta Lake**: Table definitions in `delta/delta_lake_tables.sql`
- **Hash test vectors**: 10K vectors in `test-vectors/hash_vectors.json` ensuring Rust/Go/WASM/FFI parity

## Key Design Patterns

**Crash-only design**: No graceful shutdown code paths. Startup = recovery. Stateless services restart instantly; M4b (bandit) snapshots to RocksDB and replays from last offset.

**Fail-fast data integrity**: All floating-point computations use `assert_finite!()` from `experimentation-core`. NaN/Infinity triggers immediate panic, not silent propagation.

**LMAX single-threaded core** (M4b): All bandit policy state mutations happen on a single thread via bounded mpsc channels. No locks or shared mutable state on the core thread.

**Hash determinism**: MurmurHash3 x86 32-bit, little-endian. `bucket = hash(user_id, salt) % total_buckets`. Bucket allocations use inclusive ranges: `bucket >= start && bucket <= end`.

## Conventions

**Branches**: `agent-N/<type>/<description>` (e.g., `agent-1/feat/wasm-hash-binding`). Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.

**Commits**: Conventional Commits with module/crate scope: `feat(m2): ...`, `fix(experimentation-stats): ...`. Breaking changes use `!`: `feat(m1)!: ...`.

**Rust style**: `rustfmt` defaults, `clippy --all-features -- -D warnings`, `thiserror` for library crates, `anyhow` only in binaries.

**Go style**: `gofmt`, `go vet`, `slog` for structured logging, always propagate `context.Context`.

**TypeScript**: ESLint + Prettier, strict mode, `@connectrpc/connect-web` for API calls.

**Proto changes**: Additive changes (new fields/RPCs) need no coordination. Breaking changes require an ADR and cross-agent PR review. `buf breaking` enforces this in CI.

**Testing**: Rust uses `proptest` for property-based testing (extended in nightly CI). Go uses `testify/assert` with `-race` flag. Golden files in `crates/experimentation-stats/tests/golden/`.

## CI/CD

| Pipeline | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | Every PR | Lint, build, test (Rust/Go/TS), hash parity, proto breaking check |
| `nightly.yml` | Daily 3AM UTC | Extended proptest (10K cases), benchmark comparison |
| `weekly-chaos.yml` | Sunday 2AM UTC | Kill services under load, verify crash recovery |

## Multi-Agent Development

This repository is developed by 7 specialized agents, each owning specific modules. Agent ownership boundaries are documented in `docs/onboarding/agent-{N}-*.md`. Current status is tracked in `docs/coordination/status.md`. ADRs in `adrs/` document settled architectural decisions.
