# Agent-1 — M1 Assignment Service

**Status**: Polish (mobile SDK CI fix pending)
**Current Branch**: agent-1/fix/mobile-sdk-ci
**Current Milestone**: Mobile SDK CI builds
**Blocked By**: —

## Summary

M1.1-1.5 + M2.7 + M2.7b + M2.7c complete. Live bandit delegation done. Cold-start bandit done. SDK LocalProvider + RemoteProviders shipped. Post-review cleanup done. Mobile SDK CI fix in progress.

## Key PRs

| PR | Description | Status |
|----|-------------|--------|
| #4 | Hash crate: WASM + FFI bindings | Merged |
| #11 | GetAssignment RPC (static bucketing) | Merged |
| #21 | Targeting rule evaluation | Merged |
| #100 | Chaos test scripts for assignment service | Merged |
| #116 | PGO-optimized build pipeline | Merged |
| #122 | k6 gRPC load test: 10K rps sustained | Merged |
| #128 | Cumulative holdout priority assignment | Merged |
| #138 | 50K rps load test with dynamic VU scaling | Merged |
| #142 | M1-M4b live bandit contract tests (10 tests) | Merged |
| #144 | SDK LocalProvider hash-based variant assignment | Merged |
| #150 | SDK RemoteProviders + JSON HTTP API | Merged |
| #152 | All phases complete | Merged |
| #162 | Post-review cleanup: doc ports, iOS SDK, Python drift | Merged |
| #163 | Mobile SDK CI: guard UniFFI imports | Open |

## Pair Integrations

- Agent-1 <-> Agent-4 (bandit delegation)
- Agent-1 <-> Agent-5 (config streaming)
- Agent-1 <-> Agent-7 (hash parity via CGo)

## Test Coverage

- 206 total tests across all SDKs
- 15 E2E integration tests
- 10 M1-M4b contract tests
