<!-- GENERATED from docs/agents/registry/ (the full bundle) by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Kaizen — Agent Context (vendor-neutral anchor)

This file exists for tools that discover context via the [agents.md](https://agents.md) convention (Jules, Devin, Codex, Cursor, Copilot, …). It is a **generated view** — two sources outrank it:

1. [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md) — the repo-wide context anchor (architecture, rules, commands, work tracking). **Read it first.**
2. [`docs/agents/registry/`](https://github.com/wunderkennd/kaizen-experimentation/tree/main/docs/agents/registry) — canonical per-agent identity + charters (OKF v0.1 bundle).

Directory-scoped `AGENTS.md` views are generated into each owned directory (nearest-file-wins). Ownership map (directories and files):

| Path | Agent | Charter |
| --- | --- | --- |
| `crates/experimentation-analysis/` | agent-4 | Agent-4: Statistical Analysis & Bandit Policy (M4a + M4b) |
| `crates/experimentation-assignment/` | agent-1 | Agent-1: Assignment Service (M1) |
| `crates/experimentation-bandit/` | agent-4 | Agent-4: Statistical Analysis & Bandit Policy (M4a + M4b) |
| `crates/experimentation-flags/` | agent-7 | Agent-7: Feature Flag Service (M7) |
| `crates/experimentation-ingest/` | agent-2 | Agent-2: Event Pipeline (M2) |
| `crates/experimentation-interleaving/` | agent-1 | Agent-1: Assignment Service (M1) |
| `crates/experimentation-management/` | agent-5 | Agent-5: Experiment Management Service (M5) |
| `crates/experimentation-pipeline/` | agent-2 | Agent-2: Event Pipeline (M2) |
| `crates/experimentation-policy/` | agent-4 | Agent-4: Statistical Analysis & Bandit Policy (M4a + M4b) |
| `crates/experimentation-stats/` | agent-4 | Agent-4: Statistical Analysis & Bandit Policy (M4a + M4b) |
| `delta/` | agent-3 | Agent-3: Metric Computation Engine (M3) |
| `infra/dashboards/` | infra-5 | Infra-5: Ingress, Observability & DNS |
| `infra/pkg/aws/cache.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/aws/cicd.go` | infra-4 | Infra-4: Compute & Services |
| `infra/pkg/aws/compute.go` | infra-4 | Infra-4: Compute & Services |
| `infra/pkg/aws/database.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/aws/edge.go` | infra-5 | Infra-5: Ingress, Observability & DNS |
| `infra/pkg/aws/network.go` | infra-1 | Infra-1: Networking & Foundation |
| `infra/pkg/aws/observability.go` | infra-5 | Infra-5: Ingress, Observability & DNS |
| `infra/pkg/aws/secrets.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/aws/storage.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/aws/streaming.go` | infra-3 | Infra-3: Streaming Infrastructure |
| `infra/pkg/config/` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/gcp/cache.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/gcp/compute.go` | infra-4 | Infra-4: Compute & Services |
| `infra/pkg/gcp/database.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/gcp/edge.go` | infra-5 | Infra-5: Ingress, Observability & DNS |
| `infra/pkg/gcp/network.go` | infra-1 | Infra-1: Networking & Foundation |
| `infra/pkg/gcp/observability.go` | infra-5 | Infra-5: Ingress, Observability & DNS |
| `infra/pkg/gcp/secrets.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/gcp/services/` | infra-4 | Infra-4: Compute & Services |
| `infra/pkg/gcp/storage.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/pkg/streaming/` | infra-3 | Infra-3: Streaming Infrastructure |
| `infra/test/compute_test.go` | infra-4 | Infra-4: Compute & Services |
| `infra/test/datastore_test.go` | infra-2 | Infra-2: Data Stores & Project Scaffold |
| `infra/test/network_test.go` | infra-1 | Infra-1: Networking & Foundation |
| `sdks/` | agent-1 | Agent-1: Assignment Service (M1) |
| `services/management/` | agent-5 | Agent-5: Experiment Management Service (M5) |
| `services/metrics/` | agent-3 | Agent-3: Metric Computation Engine (M3) |
| `services/orchestration/` | agent-2 | Agent-2: Event Pipeline (M2) |
| `sql/migrations/` | agent-5 | Agent-5: Experiment Management Service (M5) |
| `ui/` | agent-6 | Agent-6: Decision Support UI (M6) |

Dispatch, claims, and readiness: `scripts/orchestration/README.md`. Executor lanes are pluggable (`dispatch.d/`); this file names no vendor.
