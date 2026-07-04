---
type: Kaizen Module Agent
title: "Agent-7: Feature Flag Service (M7)"
description: Owns feature flags, percentage rollout with monotonic guarantee, and the experiment-conclusion reconciler.
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/crates/experimentation-flags
tags: [module-agent, rust, flags]
timestamp: 2026-07-04T12:00:00Z
id: agent-7
label: agent-7
language: Rust (ADR-024 port from Go shipped)
ports: [50057]
owned_paths:
  - crates/experimentation-flags/
depends_on: [agent-1, agent-5, agent-6]
---

# Charter

You own Module 7 (Feature Flag Service, port 50057) — flag CRUD, percentage rollout with
the monotonic guarantee (direct `experimentation_hash::murmur3_x86_32()`, no FFI),
multi-variant traffic fractions, PromoteToExperiment, audit trail, stale-flag detection,
and the Kafka reconciler that resolves flags when experiments conclude. ADR-024 shipped:
the service is Rust (tonic + tonic-web JSON, sqlx with compile-time-checked queries);
`experimentation-ffi` was deleted — **do not reintroduce it**.

## Standards

- `cargo test -p experimentation-flags` before every PR.
- Wire-format parity: JSON responses stay byte-identical to the retired Go service's shapes.
- Performance: p99 < 5ms at 20K rps (k6); all 13 chaos tests must pass.
- sqlx compile-time checking requires `DATABASE_URL` for `cargo check`.

## Contract-test obligations

- M7 ↔ M6: flag RPC wire-format parity (M6 is the primary consumer).
- M7 ↔ M5: PromoteToExperiment roundtrip.

## Cross-agent dependencies

- [agent-5](/agent-5.md): PromoteToExperiment target; StreamConfigUpdates consumer pattern.
- [agent-1](/agent-1.md): Go SDK pure-Go MurmurHash3 is primary post-FFI — hash parity applies.

## Work tracking

`gh issue list --label "agent-7" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
