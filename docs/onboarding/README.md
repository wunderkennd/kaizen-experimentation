# Agent Onboarding Quickstart Guides

Experimentation Platform — 7 Agents, 7 Modules, 3 Languages

## Start Here

1. Read the [Coordination Guide](agent-0-coordination.md) for the dependency map, integration schedule, and communication protocol
2. Read your agent-specific guide (below)
3. Clone the repo, run `docker-compose up -d`, and start your first PR

## Agent Summary

| Agent | Module | Language | First PR | Primary SLA |
|-------|--------|----------|----------|-------------|
| [Agent-1](agent-1-assignment.md) | M1 Assignment | Rust | Hash library + 10K test vectors | p99 < 5ms GetAssignment |
| [Agent-2](agent-2-pipeline.md) | M2 Pipeline | Rust + Go | Event validation + Kafka publisher | p99 < 10ms ingest, zero data loss |
| [Agent-3](agent-3-metrics.md) | M3 Metrics | Go + Spark | Standard metric computation job | Daily metrics < 2h |
| [Agent-4](agent-4-analysis-bandit.md) | M4a Analysis + M4b Bandit | Rust | Welch's t-test + SRM validated against R | Analysis < 60s for 1M users |
| [Agent-5](agent-5-management.md) | M5 Management | Go | Experiment CRUD + state machine | API p99 < 100ms |
| [Agent-6](agent-6-ui.md) | M6 UI | TypeScript | Experiment list + detail shell | Dashboard < 1s |
| [Agent-7](agent-7-flags.md) | M7 Flags | Go + CGo | Boolean flag CRUD + CGo hash bridge | p99 < 10ms EvaluateFlag |

## Critical Paths

Two dependency chains converge at M6 (UI):

**Data path:** M2 (Pipeline) → M3 (Metrics) → M4a (Analysis) → M6 (UI)

**Config path:** M5 (Management) → M1 (Assignment) → SDKs

Agent-2 and Agent-5 are the highest-leverage Week 1 deliverables. If either is late, multiple downstream agents are blocked with no realistic workaround beyond synthetic data.

## Dependency Map

See [agent_dependency_map.mermaid](agent_dependency_map.mermaid) for the visual dependency graph.
