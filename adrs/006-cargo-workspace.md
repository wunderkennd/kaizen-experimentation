# ADR-006: Cargo Workspace with Crate Layering

## Status
Accepted

## Date
2026-03-03

## Context
NautilusTrader organizes its Rust codebase into focused crates with explicit dependency boundaries and feature flags gating optional functionality. Our platform has four Rust service binaries that share significant domain logic (hashing, statistics, bandit algorithms). Without shared crates, we'd either duplicate code or create monolithic binaries.

## Decision
Structure all Rust code as a single Cargo workspace with 13 crates across four layers:

- **Foundation**: `experimentation-core` (timestamps, errors, tracing), `experimentation-hash` (MurmurHash3, features: wasm/uniffi/ffi/python), `experimentation-proto` (tonic-build generated types).
- **Algorithms**: `experimentation-stats` (bootstrap, CUPED, mSPRT, GST, novelty, interference; features: simd/python), `experimentation-bandit` (Thompson, LinUCB, Neural; features: gpu), `experimentation-interleaving` (Team Draft, Optimized).
- **Infrastructure**: `experimentation-ingest` (event validation, dedup), `experimentation-ffi` (cbindgen C headers for Go CGo bridge).
- **Services**: `experimentation-assignment`, `experimentation-analysis`, `experimentation-pipeline`, `experimentation-policy` (thin binary shells wrapping algorithm crates).

Feature flags control cross-language binding generation: `--features wasm` produces WebAssembly via wasm-bindgen, `--features ffi` produces C headers via cbindgen, `--features uniffi` produces Swift/Kotlin bindings, `--features python` produces PyO3 bindings.

## Alternatives Considered
- **Separate repositories per service**: Eliminates workspace complexity but duplicates shared code (hash library, stats library). Version coordination across repos is painful.
- **Single monolithic crate**: Simplest structure, but compile times scale with total code size. Feature flags on a monolith are fragile — a change to bandit code triggers recompilation of the assignment service.
- **Git submodules for shared crates**: Allows separate repos with shared code, but submodule workflows are notoriously error-prone and poorly supported by IDEs.

## Consequences
- Single `cargo clippy --all-features` CI job catches cross-crate issues.
- sccache shared across all crate compilations reduces build times.
- Algorithm crates can be tested in isolation (`cargo test -p experimentation-stats`) without starting gRPC servers.
- Algorithm crates can be imported into Python notebooks via PyO3 for offline analysis.
