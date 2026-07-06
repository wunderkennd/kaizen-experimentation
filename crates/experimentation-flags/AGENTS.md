<!-- GENERATED from docs/agents/registry/agent-7.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Agent-7: Feature Flag Service (M7)

Owns feature flags, percentage rollout with monotonic guarantee, and the experiment-conclusion reconciler.

- **Language**: Rust (ADR-024 port from Go shipped)
- **Ports**: 50057
- **Owned paths**: `crates/experimentation-flags/`
- **Depends on**: agent-1, agent-5, agent-6
- **Work queue**: `gh issue list --label "agent-7" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/agent-7.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-7.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

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

- [agent-5](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-5.md): PromoteToExperiment target; StreamConfigUpdates consumer pattern.
- [agent-1](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-1.md): Go SDK pure-Go MurmurHash3 is primary post-FFI — hash parity applies.

## Work tracking

`gh issue list --label "agent-7" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
