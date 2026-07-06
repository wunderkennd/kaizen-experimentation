<!-- GENERATED from docs/agents/registry/infra-3.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Infra-3: Streaming Infrastructure

Owns Kafka streaming infrastructure — MSK or Redpanda, the 8 topics, and Schema Registry.

- **Language**: Go (Pulumi)
- **Owned paths**: `infra/pkg/aws/streaming.go`, `infra/pkg/streaming/`
- **Depends on**: infra-1
- **Work queue**: `gh issue list --label "infra-3" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/infra-3.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/infra-3.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

# Charter

You own streaming infrastructure. AWS tenants: MSK (3× `kafka.m5.large` across 3 AZs,
SASL/SCRAM + TLS, `auto.create.topics.enable=false`, RF 3, `min.insync.replicas=2`, lz4)
plus Confluent Schema Registry on Fargate (Protobuf, BACKWARD compat). Streaming
dispatches on `cfg.StreamingProvider`, **not** `cfg.CloudProvider` — new tenants on any
cloud can use Redpanda (`infra/pkg/streaming/redpanda.go`, Redpanda Cloud bridge, built-in
registry). The eight topics (exposures 64p/90d, metric_events 128p/90d, reward_events
32p/180d, qoe_events 64p/90d, guardrail_alerts 8p/30d, sequential_boundary_alerts 8p/30d,
model_retraining_events 8p/180d, surrogate_recalibration_requests 4p/30d) must match
`kafka/topic_configs.sh` exactly.

## Output contract

Both paths return `types.StreamingOutputs` (`BootstrapBrokers`, `SchemaRegistryUrl`,
`ClusterArn`).

## Standards

- MSK prod: `enhanced_monitoring = "PER_TOPIC_PER_BROKER"`; 100GB EBS per broker (configurable).
- Schema Registry on Fargate: 0.25 vCPU / 512MB, health check `:8081/subjects`.
- Registry wire compatibility for Redpanda proven via Docker Compose before adoption (#478).

## Work tracking

`gh issue list --label "infra-3" --state open`.
