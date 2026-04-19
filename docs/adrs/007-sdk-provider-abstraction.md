# ADR-007: SDK Provider Abstraction with Fallback Chain

**Status**: Accepted
**Date**: 2026-03-03

---

## Context
GrowthBook users learned to wrap the SDK in a provider abstraction so the underlying platform can be swapped without changing call sites. Our SDKs must work across web, iOS, Android, and server environments with varying network reliability. Mobile clients in particular cannot depend on the Assignment Service being reachable.

## Decision
All client SDKs implement an `ExperimentProvider` interface with three concrete implementations:

- **RemoteProvider**: gRPC/HTTP call to M1 Assignment Service. Primary path.
- **LocalProvider**: Uses WASM-compiled hash library (web) or UniFFI hash library (mobile) with a cached config snapshot. Produces deterministic assignments offline. Config snapshot refreshed periodically when network is available.
- **MockProvider**: Returns deterministic, configurable assignments for unit testing product code.

A `ResilientProvider` wraps these in a fallback chain: Remote → Local cache → Static defaults. Product code calls `getAssignment()` on the ResilientProvider and is unaware of which backend served the result.

## Alternatives Considered
- **Remote-only SDK**: Simplest, but mobile apps in poor network conditions would show default experiences instead of experiment variants. Unacceptable for global SVOD.
- **Local-only SDK (download all configs)**: Eliminates network dependency but the config payload grows linearly with active experiments. 100+ experiments × targeting rules × variant configs = megabytes of config data on every app launch.
- **Platform-specific SDKs without abstraction**: Each platform gets a custom implementation. Works initially, but migration to a different experimentation platform (or splitting the platform) requires rewriting all call sites in all apps.

## Consequences
- Hash library must produce identical results across Rust native, WASM, UniFFI, CGo, and Python. 10,000 test vectors verified in CI.
- Config snapshot format must be versioned and backward-compatible.
- LocalProvider assignments may be stale if config has changed since last refresh — acceptable tradeoff vs. no assignment at all.
