# Architecture Decision Records

This directory contains the architectural decisions that shaped the experimentation platform. Each ADR documents a significant technical choice, the alternatives considered, and the consequences. These are settled decisions — agents should not relitigate them without strong new evidence.

## Decision Index

| ADR | Decision | Status | Impact |
|-----|----------|--------|--------|
| [001](001-language-selection.md) | Rust for hot paths, Go for orchestration, TypeScript for UI only | Accepted | All modules |
| [002](002-lmax-bandit-core.md) | LMAX-inspired single-threaded core for bandit policy | Accepted | M4b |
| [003](003-rocksdb-policy-state.md) | RocksDB for bandit policy crash-only state | Accepted | M4b |
| [004](004-gst-alongside-msprt.md) | Group Sequential Tests alongside mSPRT | Accepted | M4a, M5, M6 |
| [005](005-component-state-machine.md) | Transitional states (STARTING, CONCLUDING) in experiment lifecycle | Accepted | M1, M4a, M5, M6 |
| [006](006-cargo-workspace.md) | Cargo workspace with 13 crates across 4 layers | Accepted | All Rust |
| [007](007-sdk-provider-abstraction.md) | SDK provider abstraction with fallback chain | Accepted | SDKs, M1 |
| [008](008-auto-pause-guardrails.md) | Auto-pause as default guardrail behavior | Accepted | M3, M5 |
| [009](009-bucket-reuse.md) | Automated bucket reuse with 24h cooldown | Accepted | M1, M5 |
| [010](010-connectrpc.md) | ConnectRPC for Go, tonic for Rust, shared proto contracts | Accepted | All modules |

## Related

- [CI/CD Pipeline Design](cicd-pipeline-design.md) — build pipeline architecture for the multi-language workspace
