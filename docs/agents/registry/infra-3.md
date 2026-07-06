---
type: Kaizen Infra Agent
title: "Infra-3: Streaming Infrastructure"
description: Owns Kafka streaming infrastructure — MSK or Redpanda, the 8 topics, and Schema Registry.
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/infra/pkg/aws
tags: [infra-agent, go, pulumi, kafka, msk, redpanda]
timestamp: 2026-07-04T12:00:00Z
id: infra-3
label: infra-3
executors: [claude-workflow, claude-web, multiclaude, jules]
language: Go (Pulumi)
owned_paths:
  - infra/pkg/aws/streaming.go
  - infra/pkg/streaming/
depends_on: [infra-1]
---

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
