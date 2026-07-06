<!-- GENERATED from docs/agents/registry/agent-3.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Agent-3: Metric Computation Engine (M3)

Owns Spark SQL orchestration, metric computation, Delta Lake tables, the MetricQL compiler, and notebook export.

- **Language**: Go
- **Ports**: 50056, 50059
- **Owned paths**: `services/metrics/`, `delta/`
- **Depends on**: agent-2, agent-4, agent-5
- **Work queue**: `gh issue list --label "agent-3" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/agent-3.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-3.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

# Charter

You own Module 3 (Metric Computation Engine) — Spark SQL orchestration, metric
computation, Delta Lake table management, surrogate models, and notebook export
(port 50056; Prometheus on 50059). You own the ADR-026 MetricQL surface: lexer,
recursive-descent parser, AST, semantic analyzer, DFS cycle detector, and Spark SQL
codegen in `services/metrics/internal/metricql/`, plus topo-order scheduling of
`@metric_ref` dependencies (Kahn's algorithm, `metric_computation_status`).

## Standards

- `go test ./services/metrics/...` before every PR.
- SQL templates live in `services/metrics/templates/*.sql.tmpl`; every query logged to
  `query_log` (powers "View SQL" and "Export to Notebook").
- Delta Lake schemas documented as CREATE TABLE DDL under `delta/`.
- New computation pipelines add Prometheus counters/histograms on :50059.
- **No statistical computation in Go** — math belongs to [agent-4](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-4.md)'s
  experimentation-stats crate.

## Contract-test obligations

- M3 ↔ M4a: provider-metric wire format (`experiment_level_metrics` schema).
- M3 ↔ M2: `ModelRetrainingEvent` Kafka roundtrip.
- M3 ↔ M5: MLRATE trigger during STARTING lifecycle.

## Cross-agent dependencies

- [agent-2](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-2.md): `model_retraining_events` topic must exist for contamination pipeline.
- [agent-4](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-4.md): provider metrics consumed for treatment-effect analysis.
- [agent-5](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-5.md): lifecycle hooks (MLRATE at STARTING; metric-definition validation upstream).

## Work tracking

`gh issue list --label "agent-3" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
