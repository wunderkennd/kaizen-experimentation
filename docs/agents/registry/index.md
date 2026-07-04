---
okf_version: "0.1"
---

# Kaizen Agent Registry

Canonical agent-identity registry for the Kaizen Experimentation Platform, packaged as an
[Open Knowledge Format v0.1](https://github.com/GoogleCloudPlatform/knowledge-catalog/tree/main/okf)
bundle. Each concept below is one agent: YAML frontmatter carries the machine-readable
identity (id, label, language, ports, owned paths, dependencies); the body carries the
charter (responsibilities, standards, contract-test obligations).

This bundle is the **single source of truth** for agent identity (harness proposal
[§7 R3/R6](../../coordination/harness-modernization-proposal.md)). The copies in
`.multiclaude/agents/`, `docs/coordination/prompts/`, and `docs/onboarding/` become
generated views under [#682](https://github.com/wunderkennd/kaizen-experimentation/issues/682);
until the generator lands, edit HERE first and mirror by hand.

Conformance: `just check-registry` (three OKF rules + reserved-file structure).

## Module agents (product)

- [agent-1](/agent-1.md) — M1 Assignment Service: variant allocation, interleaving, bandit delegation, SDKs
- [agent-2](/agent-2.md) — M2 Event Pipeline: validation, dedup, Kafka (Rust) + orchestration (Go)
- [agent-3](/agent-3.md) — M3 Metric Computation: Spark SQL, Delta Lake, MetricQL compiler
- [agent-4](/agent-4.md) — M4a Statistical Analysis + M4b Bandit Policy: all statistical computation
- [agent-5](/agent-5.md) — M5 Experiment Management: CRUD, lifecycle, RBAC, guardrails, portfolio
- [agent-6](/agent-6.md) — M6 Decision Support UI: dashboards and visualization, TypeScript only
- [agent-7](/agent-7.md) — M7 Feature Flag Service: flags, percentage rollout, reconciler (Rust, ADR-024)

## Infra agents (Pulumi IaC)

- [infra-1](/infra-1.md) — Networking & foundation: VPC, subnets, security groups, DNS namespace, IAM
- [infra-2](/infra-2.md) — Data stores & scaffold: RDS/Cloud SQL, Redis, S3/GCS, secrets, Pulumi project
- [infra-3](/infra-3.md) — Streaming: MSK/Redpanda, topics, Schema Registry
- [infra-4](/infra-4.md) — Compute & services: ECS/Cloud Run, ECR, the 9 service definitions, M4b special case
- [infra-5](/infra-5.md) — Ingress & observability: ALB/CLB, DNS, TLS, WAF, Prometheus/Grafana, alarms

## Related (outside this bundle)

- [CLAUDE.md](../../../CLAUDE.md) — repo-wide context anchor (architecture table is a projection of this registry)
- [Harness modernization proposal](../../coordination/harness-modernization-proposal.md) — why this bundle exists
- [Agent onboarding](../../onboarding/) — deep-dive per-agent guides (view layer)
- [Module runbooks](../../runbooks/) — operational procedures
