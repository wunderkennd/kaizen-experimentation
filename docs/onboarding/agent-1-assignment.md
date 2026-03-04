# Agent-1 Quickstart: M1 Assignment Service (Rust)

## Your Identity

| Field | Value |
|-------|-------|
| Module | M1: Assignment Service |
| Language | Rust |
| Crates you own | `experimentation-assignment` (binary), `experimentation-hash`, `experimentation-interleaving` |
| Proto package | `experimentation.assignment.v1` |
| Infra you own | None (stateless, crash-only) |
| Primary SLA | p99 < 5ms for `GetAssignment`, p99 < 15ms for `GetInterleavedList` |

## Read These First (in order)

1. **Design doc v5.1** — Sections 1.4 (module strategy), 2.1 (crate layering), 2.6 (SDK provider abstraction), 2.9 (bucket reuse), 4 (M1 specification)
2. **ADR-001** (language selection), **ADR-006** (Cargo workspace), **ADR-007** (SDK provider), **ADR-009** (bucket reuse), **ADR-010** (ConnectRPC)
3. **Proto files** — `assignment_service.proto`, `experiment.proto`, `layer.proto`, `interleaving.proto`, `targeting.proto`
4. **Mermaid diagrams** — `system_architecture.mermaid` (your position in the topology), `sdk_provider.mermaid` (client fallback chain), `crate_graph.mermaid` (your crate dependencies)

## Who You Depend On (upstream)

| Module | What you need from them | Blocks you? |
|--------|------------------------|-------------|
| M5 (Agent-5) | Experiment configs streamed via `StreamConfigUpdates` RPC | Yes — you cache configs in-process. Until M5 exists, use a local JSON config file for dev. |
| M4b (Agent-4) | `SelectArm` RPC for bandit experiments | Partially — only for bandit types. Mock with random selection initially. |
| M7 (Agent-7) | Feature flag evaluation for targeting rules | No — targeting evaluation is self-contained using cached rules. |

## Who Depends on You (downstream)

| Module | What they need from you | Impact if you're late |
|--------|------------------------|----------------------|
| M2 (Agent-2) | Exposure events reference your variant assignments | They can generate synthetic exposures; not truly blocked. |
| M6 (Agent-6) | Assignment debugging UI reads your assignment logs | Not blocked until Phase 2. |
| All SDKs | `GetAssignment` / `GetAssignments` RPCs | SDK development blocked on your RPC contract being stable. |
| M4a (Agent-4) | Consistent hash ensures SRM check works | They validate against your hash vectors. |

## Your First PR: Hash Library + Deterministic Bucketing

**Goal**: A working `experimentation-hash` crate that passes the 10,000-entry test vector file.

```
crates/experimentation-hash/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Public API: bucket(user_id, salt, total_buckets) -> u32
│   ├── murmur3.rs      # MurmurHash3 x86_128 implementation
│   └── vectors.rs      # Load and run test vector file
└── tests/
    └── determinism.rs   # All 10,000 vectors produce expected buckets
```

**Acceptance criteria**:
- `bucket("user_123", "salt_abc", 10000)` returns the same value every time, across: native Rust, WASM (wasm-pack test --node), and C FFI (cbindgen headers + C test harness).
- All 10,000 test vectors pass.
- `cargo test --package experimentation-hash --all-features` passes.

**Why this first**: The hash function is the foundation of deterministic assignment. Every other agent who touches bucketing (M7 via CGo, SDKs via WASM/UniFFI) depends on this crate producing identical results. Getting this right and locked down in CI unblocks everything.

## Phase-by-Phase Deliverables

### Phase 0 (Week 1)
- [ ] `experimentation-hash` crate with MurmurHash3, all feature flags stubbed (`wasm`, `uniffi`, `ffi`, `python`)
- [ ] Test vector validation (10,000 entries)
- [ ] `experimentation-interleaving` crate stub (types only, no algorithms yet)
- [ ] `experimentation-assignment` binary stub (tonic gRPC server, health check endpoint)

### Phase 1 (Weeks 2–7)
- [ ] `GetAssignment` RPC — static user bucketing (hash → layer → bucket → variant)
- [ ] `GetAssignments` bulk RPC — all active experiments for a user
- [ ] Config cache — subscribe to M5 `StreamConfigUpdates` (or poll fallback)
- [ ] Targeting rule evaluation — evaluate `TargetingRule` predicates against user attributes
- [ ] Layer-aware assignment — respect `LayerAllocation` boundaries
- [ ] Session-level assignment — hash `session_id` when experiment type = SESSION_LEVEL
- [ ] WASM build of `experimentation-hash` (feature: `wasm`)
- [ ] CGo build via `experimentation-ffi` (feature: `ffi`, cbindgen headers)
- [ ] Load test: p99 < 5ms at 10K rps

### Phase 2 (Weeks 6–11)
- [ ] `GetInterleavedList` RPC — Team Draft algorithm
- [ ] Optimized interleaving (Radlinski & Craswell 2010)
- [ ] Bandit delegation — call M4b `SelectArm` for MAB/CONTEXTUAL_BANDIT types
- [ ] Log `assignment_probability` in exposure event for IPW analysis

### Phase 3 (Weeks 10–17)
- [ ] Multileave interleaving (3+ algorithms)
- [ ] Cumulative holdout priority assignment (holdout allocation before layer)
- [ ] Cold-start bandit integration with M4b `CreateColdStartBandit`
- [ ] UniFFI bindings (feature: `uniffi`) for iOS/Android SDKs

### Phase 4 (Weeks 16–22)
- [ ] PGO-optimized release build
- [ ] Chaos engineering: kill -9 under load, verify recovery < 2 seconds
- [ ] p99 < 5ms validation at 50K rps

## Local Development

```bash
# Clone and build
git clone <repo>
cd crates
cargo build --package experimentation-assignment

# Run unit tests (fast, no external deps)
cargo test --package experimentation-hash
cargo test --package experimentation-interleaving
cargo test --package experimentation-assignment

# Run the server locally (connects to local M5 or uses file config)
EXPERIMENT_CONFIG_PATH=./dev/config.json cargo run --package experimentation-assignment

# WASM build
cargo build --package experimentation-hash --target wasm32-unknown-unknown --features wasm
wasm-pack test --node crates/experimentation-hash

# C FFI build (for M7 CGo bridge)
cargo build --package experimentation-ffi --features ffi
# Headers generated at: target/experimentation_ffi.h
```

## Testing Expectations

- **Unit tests**: >90% coverage. Use Rust built-in `#[test]`. Test every assignment path: static, session, bandit delegation, interleaving, targeting miss.
- **Property-based**: proptest for hash distribution uniformity (chi-squared test: buckets are uniformly distributed within 3 sigma).
- **Integration**: Docker Compose with a mock M5 config server. Verify assignment consistency across server restarts.
- **Benchmarks**: criterion benchmarks for `GetAssignment` latency. Run nightly, alert on >10% regression.

## Common Pitfalls

1. **Hash endianness**: MurmurHash3 output depends on byte order. Our reference implementation uses little-endian. WASM also runs little-endian, but verify with test vectors.
2. **Bucket boundary off-by-one**: `hash % total_buckets` must produce values in `[0, total_buckets)`. Layer allocations use inclusive ranges `[start_bucket, end_bucket]`, so the check is `bucket >= start && bucket <= end`.
3. **Config staleness**: If M5 config stream disconnects, serve from the last known good config — never return "no experiments." The crash-only principle means restart recovers the stream; stale configs are preferable to empty configs.
4. **Bandit timeout**: If M4b `SelectArm` takes > 10ms, fall back to uniform random selection. Log the timeout. Don't let bandit latency propagate to your p99.
5. **Interleaving provenance**: Every item in the merged list must have provenance metadata (which algorithm contributed it). This is not optional — M3 uses it for credit assignment.
