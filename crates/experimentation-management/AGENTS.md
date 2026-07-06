<!-- GENERATED from docs/agents/registry/agent-5.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Agent-5: Experiment Management Service (M5)

Owns experiment CRUD, lifecycle state machine, RBAC, guardrails, bucket reuse, portfolio, and the adaptive-N scheduler.

- **Language**: Go (production) + Rust port in progress (ADR-025)
- **Ports**: 50055, 50060
- **Owned paths**: `services/management/`, `crates/experimentation-management/`, `sql/migrations/`
- **Depends on**: agent-1, agent-3, agent-4, agent-6
- **Work queue**: `gh issue list --label "agent-5" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/agent-5.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-5.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

# Charter

You own Module 5 (Experiment Management Service, port 50055 ConnectRPC) — CRUD, the
lifecycle state machine, RBAC, guardrails, bucket reuse, portfolio endpoints, the
OnlineFdrController singleton, and adaptive-sample-size scheduling. Two implementations:
Go (`services/management/`, production) and the ADR-025 Rust port
(`crates/experimentation-management/` — Phase 2/4 landed; **Phase 1 RBAC interceptor and
Phase 3 statistical integration pending, #590** — the Rust build currently ships with no
auth module; do not shift traffic to it).

## Standards

- Go: `go test ./services/management/...`; Rust: `cargo test -p experimentation-management`.
- State transitions use `UPDATE ... WHERE state = $expected` with `RowsAffected() == 1`
  (TOCTOU safety).
- RBAC: every new RPC is wired into the auth interceptor with explicit role levels.
- All lifecycle transitions, config changes, and classifications hit the audit trail.
- PostgreSQL migrations in `sql/migrations/` with sequential numbering.

## Contract-test obligations

- M5 ↔ M4a: conditional power; e-value submission. M5 ↔ M6: portfolio data,
  meta-experiment config, adaptive-N zone. M5 ↔ M1: SWITCHBACK/META in
  StreamConfigUpdates. M5 ↔ M3: MLRATE STARTING trigger.

## Cross-agent dependencies

- [agent-4](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-4.md): all statistical computation is delegated — never reimplement.
- [agent-3](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-3.md), [agent-1](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-1.md), [agent-6](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-6.md): see obligations.

## Work tracking

`gh issue list --label "agent-5" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
