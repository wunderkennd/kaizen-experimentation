# CI/CD Pipeline Design

## Repository Structure

```
experimentation-platform/
├── proto/                          # Protobuf schema (buf toolchain)
│   ├── buf.yaml
│   ├── buf.gen.yaml
│   └── experimentation/            # All .proto files
├── crates/                         # Rust Cargo workspace
│   ├── Cargo.toml                  # Workspace root
│   ├── experimentation-core/
│   ├── experimentation-hash/
│   ├── experimentation-proto/
│   ├── experimentation-stats/
│   ├── experimentation-bandit/
│   ├── experimentation-interleaving/
│   ├── experimentation-ingest/
│   ├── experimentation-ffi/
│   ├── experimentation-assignment/
│   ├── experimentation-analysis/
│   ├── experimentation-pipeline/
│   └── experimentation-policy/
├── services/                       # Go services
│   ├── go.mod
│   ├── management/                 # M5
│   ├── metrics/                    # M3
│   ├── flags/                      # M7
│   └── orchestration/              # M2 Go orchestration layer
├── ui/                             # M6 TypeScript (Next.js)
│   ├── package.json
│   └── src/
├── sdks/                           # Client SDKs
│   ├── web/                        # TypeScript + WASM
│   ├── ios/                        # Swift + UniFFI
│   ├── android/                    # Kotlin + UniFFI
│   ├── server-go/                  # Go + CGo
│   └── server-python/              # Python + PyO3
├── sql/                            # PostgreSQL migrations
├── delta/                          # Delta Lake table definitions
├── kafka/                          # Topic configurations
├── docs/adrs/                      # Architecture Decision Records
├── scripts/                        # CI/CD scripts
└── .github/workflows/              # GitHub Actions
```

## Error Handling

All workflows set `bash -euo pipefail` as the default shell:

```yaml
defaults:
  run:
    shell: bash -euo pipefail {0}
```

- **`-e`**: Any command failure immediately fails the step
- **`-u`**: Unset variable references fail the step (catches typos)
- **`-o pipefail`**: Pipe failures propagate (e.g., `cmd | tee` fails if `cmd` fails)

This applies to all `run` steps across the CI, nightly, and weekly chaos workflows.

## Pipeline Stages

### Stage 1: Schema Validation (proto/)
**Trigger**: Any change to `proto/` directory.
**Runtime**: ~30 seconds.

`proto/buf.yaml` uses v2 config format with the `STANDARD` lint category, which
requires buf CLI v2.x. The setup action uses `version: "latest"` to stay current.

```yaml
schema-lint:
  runs-on: ubuntu-latest
  steps:
    - uses: bufbuild/buf-setup-action@v1
      with:
        version: "latest"  # v2+ required for STANDARD lint category
    - run: buf lint
    - run: buf breaking --against 'https://github.com/org/experimentation-platform.git#branch=main,subdir=proto'
    # Ensures no backward-incompatible changes to published protos.

schema-generate:
  needs: schema-lint
  steps:
    - run: buf generate
    # Outputs: gen/go/, gen/ts/
    # Rust generation handled by tonic-build in Cargo workspace.
    - uses: actions/upload-artifact@v4
      with:
        name: generated-code
        path: gen/
```

### Stage 2: Rust Build & Test (crates/)
**Trigger**: Any change to `crates/` or `proto/`.
**Runtime**: ~8 minutes (with sccache), ~25 minutes cold.

```yaml
rust-build:
  runs-on: ubuntu-latest
  env:
    SCCACHE_GHA_ENABLED: "true"
    RUSTC_WRAPPER: "sccache"
  steps:
    - uses: mozilla-actions/sccache-action@v0.0.4

    # Lint all crates with all features enabled.
    - run: cargo clippy --workspace --all-features -- -D warnings

    # Unit tests (all crates, no features = minimal build).
    - run: cargo test --workspace

    # Property-based tests (experimentation-stats only, extended timeout).
    - run: cargo test --package experimentation-stats --features simd -- --test-threads=1
      timeout-minutes: 15

    # Hash determinism vectors (experimentation-hash).
    - run: cargo test --package experimentation-hash -- hash_vectors

    # Benchmarks (criterion, not run on every PR — nightly only).
    # - run: cargo bench --package experimentation-stats
    # - run: cargo bench --package experimentation-assignment

rust-cross-compile:
  needs: rust-build
  strategy:
    matrix:
      target:
        - name: wasm
          command: "cargo build --package experimentation-hash --target wasm32-unknown-unknown --features wasm"
        - name: ffi
          command: "cargo build --package experimentation-ffi --features ffi"
  steps:
    - run: ${{ matrix.target.command }}
```

### Stage 3: Go Build & Test (services/)
**Trigger**: Any change to `services/` or `proto/`.
**Runtime**: ~5 minutes.

```yaml
go-build:
  runs-on: ubuntu-latest
  needs: schema-generate
  steps:
    - uses: actions/download-artifact@v4
      with: { name: generated-code, path: gen/ }

    - run: go vet ./...
    - run: CGO_ENABLED=1 go test -race -cover ./...

    # Integration tests (require PostgreSQL + Kafka).
    # Uses --wait to ensure services are healthy before tests start.
    # Captures test exit code so cleanup always runs, then propagates
    # the original error.
    - run: |
        docker compose -f docker-compose.test.yml up -d --wait
        test_exit=0
        CGO_ENABLED=1 go test -tags=integration -race ./... || test_exit=$?
        docker compose -f docker-compose.test.yml down
        exit $test_exit
```

### Stage 4: TypeScript Build & Test (ui/)
**Trigger**: Any change to `ui/` or `proto/`.
**Runtime**: ~3 minutes.

```yaml
ui-build:
  runs-on: ubuntu-latest
  needs: schema-generate
  steps:
    - uses: actions/download-artifact@v4
      with: { name: generated-code, path: gen/ }

    - run: npm ci
    - run: npm run lint
    - run: npm run type-check
    - run: npm run test -- --run  # vitest + React Testing Library
    - run: npm run build
```

### Stage 5: Cross-Language Validation
**Trigger**: Any change to `crates/experimentation-hash/` or `sdks/`.
**Runtime**: ~5 minutes.

```yaml
hash-parity:
  needs: [rust-build, rust-cross-compile, go-build]
  steps:
    # Verify identical hash outputs across all targets.
    - run: python scripts/verify_hash_parity.py
    # Runs 10,000 test vectors through:
    #   - Rust native (cargo test)
    #   - WASM (node + wasm-bindgen)
    #   - UniFFI Swift (swift test)
    #   - UniFFI Kotlin (gradle test)
    #   - CGo (go test with CGO_ENABLED=1)
    #   - Python PyO3 (pytest)
    # All must produce identical bucket assignments.
```

### Stage 6: Docker Build & Push
**Trigger**: Merge to main.

```yaml
docker-build:
  needs: [rust-build, go-build, ui-build]
  strategy:
    matrix:
      service:
        - { name: assignment, path: crates/experimentation-assignment, lang: rust }
        - { name: pipeline, path: crates/experimentation-pipeline, lang: rust }
        - { name: analysis, path: crates/experimentation-analysis, lang: rust }
        - { name: policy, path: crates/experimentation-policy, lang: rust }
        - { name: management, path: services/management, lang: go }
        - { name: metrics, path: services/metrics, lang: go }
        - { name: flags, path: services/flags, lang: go }
        - { name: ui, path: ui, lang: ts }
  steps:
    - run: docker build -f ${{ matrix.service.path }}/Dockerfile -t experimentation/${{ matrix.service.name }}:${{ github.sha }} .
    - run: docker push experimentation/${{ matrix.service.name }}:${{ github.sha }}
```

## Build Optimization

### sccache Configuration
```toml
# .cargo/config.toml
[build]
rustc-wrapper = "sccache"

# sccache uses GitHub Actions cache backend.
# Shared across all crate compilations within the workspace.
# Typical hit rate: 70-85% on incremental builds.
```

### Rust Profile-Guided Optimization (PGO)
PGO applied to release builds of M1 (Assignment) and M4b (Bandit Policy) — the two most latency-sensitive services.

```yaml
# Nightly PGO pipeline (not on every PR).
rust-pgo:
  runs-on: ubuntu-latest
  steps:
    # Step 1: Instrumented build.
    - run: RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" cargo build --release --package experimentation-assignment
    # Step 2: Run benchmark workload to generate profile data.
    - run: ./scripts/pgo_workload.sh
    # Step 3: Merge profile data.
    - run: llvm-profdata merge -o /tmp/pgo-data/merged.profdata /tmp/pgo-data
    # Step 4: PGO-optimized build.
    - run: RUSTFLAGS="-Cprofile-use=/tmp/pgo-data/merged.profdata" cargo build --release --package experimentation-assignment
```

### Dependency Caching Strategy
- **Rust**: sccache (compilation artifacts) + cargo-cache (dependency downloads).
- **Go**: actions/cache for `~/go/pkg/mod` and `~/.cache/go-build`.
- **TypeScript**: actions/cache for `node_modules` (keyed on package-lock.json hash).
- **Proto**: buf CLI caches BSR modules locally.

## Nightly Pipelines

### Statistical Validation (nightly)
Extended property-based test suite that would be too slow for PR builds:

```yaml
nightly-stats-validation:
  schedule: "0 3 * * *"  # 3 AM UTC
  steps:
    # proptest with 10,000 cases (vs 256 default in PR builds).
    - run: PROPTEST_CASES=10000 cargo test --package experimentation-stats --features simd
    # GST boundary validation against R gsDesign package.
    - run: python scripts/validate_gst_boundaries.py
    # Bootstrap CI coverage check (93-97% on 1000 synthetic datasets).
    - run: cargo test --package experimentation-stats -- bootstrap_coverage --ignored
```

### Benchmark Tracking (nightly)
```yaml
nightly-benchmarks:
  schedule: "0 4 * * *"
  steps:
    - run: cargo bench --package experimentation-assignment -- --output-format bencher | tee bench_results.txt
    - run: cargo bench --package experimentation-stats -- --output-format bencher | tee -a bench_results.txt
    # Upload to benchmark tracking dashboard. Alert if p99 regresses > 10%.
```

### Chaos Engineering (weekly)
```yaml
weekly-chaos:
  schedule: "0 2 * * 0"  # Sunday 2 AM
  steps:
    # Deploy to staging.
    - run: ./scripts/deploy_staging.sh
    # Kill M1 under load, verify recovery < 2 seconds.
    - run: ./scripts/chaos_kill_assignment.sh
    # Kill M4b under load, verify recovery < 10 seconds.
    - run: ./scripts/chaos_kill_policy.sh
    # Kill M2 under load, verify recovery < 1 second.
    - run: ./scripts/chaos_kill_ingestion.sh
    # Verify no data loss or inconsistency.
    - run: ./scripts/chaos_verify_integrity.sh
```

## Deployment Strategy
- **Staging**: Auto-deploy on merge to main. Full integration test suite runs post-deploy.
- **Production**: Manual promotion from staging via GitHub Actions workflow_dispatch. Canary deployment (10% traffic) for 30 minutes before full rollout.
- **Rollback**: One-click rollback to previous Docker image tag. Rust services (crash-only) recover immediately. M4b restores from RocksDB snapshot.
