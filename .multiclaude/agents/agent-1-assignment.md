# Agent-1: Assignment Service

You own Module 1 (Assignment Service) — all variant allocation, interleaving list construction, bandit arm delegation, and client SDKs.

Language: Rust
Crate: `crates/experimentation-assignment/`
Service port: 50051 (gRPC + HTTP JSON via tonic-web)
Dependencies: experimentation-hash, experimentation-proto, experimentation-interleaving, experimentation-core

## Phase 5 ADR Responsibilities

### Primary Owner
- **ADR-016 (Slate Bandits)**: Implement `GetSlateAssignment` RPC. New proto: `GetSlateAssignmentRequest/Response` in `proto/experimentation/assignment/v1/`. Forward candidate items to M4b's slate bandit, return ordered slate with per-slot probabilities. SDKs gain `getSlate()` method.
- **ADR-022 (Switchback)**: Implement time-based assignment for `EXPERIMENT_TYPE_SWITCHBACK`. Assignment determined by `(current_time, block_duration, cluster_attribute)` instead of `hash(user_id)`. Washout period exclusion returns `None`. Block index recorded in exposure events.
- **ADR-013 (Meta-Experiments)**: Route META experiment assignments — hash user to variant via standard bucketing, then delegate to M4b with variant-specific `MetaVariantConfig`.

### Supporting Role
- **ADR-012 (LP Constraints)**: Ensure `SelectArm` response from M4b includes LP-adjusted probabilities. Log adjusted `assignment_probability` in exposure events.

## Coding Standards
- Run `cargo test -p experimentation-assignment` before creating PR.
- Hash parity: any change to bucketing logic must pass all 10K test vectors.
- p99 latency target: < 5ms for GetAssignment, < 15ms for GetSlateAssignment.
- All new RPCs must have tonic-web JSON mode enabled for SDK compatibility.
- Write status to `docs/coordination/status/agent-1-status.md`.

## Dependencies on Other Agents
- Agent-4 (M4b): `SelectArm` and `GetSlateAssignment` responses — coordinate on proto contract.
- Agent-5 (M5): `StreamConfigUpdates` must include new experiment type configs (SWITCHBACK, META).
- Agent-Proto: Proto schema for new RPCs and experiment types must land first.

## Contract Tests to Write
- M1 ↔ M4b: Slate assignment roundtrip (ADR-016)
- M1 ↔ M5: Switchback config compatibility (ADR-022)
- M1 ↔ M4b: LP constraint probabilities logged correctly (ADR-012)
