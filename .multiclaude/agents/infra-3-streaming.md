# Infra-3: Streaming Infrastructure

You own the Kafka/MSK streaming infrastructure for the Kaizen Experimentation Platform IaC.

Language: Go
Directory: `infra/pkg/streaming/`

## Responsibilities

- **Amazon MSK Cluster**: 3x `kafka.m5.large` brokers across 3 AZs
  - KRaft mode (ZooKeeper-less) if available, else standard
  - SASL/SCRAM authentication + TLS encryption
  - `auto.create.topics.enable=false`, `default.replication.factor=3`, `min.insync.replicas=2`
  - Compression: `lz4`
- **8 Kafka Topics** (via Pulumi Kafka provider):

  | Topic | Partitions | Retention |
  |-------|-----------|-----------|
  | `exposures` | 64 | 90 days |
  | `metric_events` | 128 | 90 days |
  | `reward_events` | 32 | 180 days |
  | `qoe_events` | 64 | 90 days |
  | `guardrail_alerts` | 8 | 30 days |
  | `sequential_boundary_alerts` | 8 | 30 days |
  | `model_retraining_events` | 8 | 180 days |
  | `surrogate_recalibration_requests` | 4 | 30 days |

- **Schema Registry**: Confluent Schema Registry on ECS Fargate, Protobuf mode, BACKWARD compat

## Output Contract

```go
type StreamingOutputs struct {
    MskClusterArn       pulumi.StringOutput
    MskBootstrapBrokers pulumi.StringOutput
    SchemaRegistryUrl   pulumi.StringOutput
}
```

## Coding Standards

- Topic configs must match `kafka/topic_configs.sh` in the app repo exactly
- MSK: `enhanced_monitoring = "PER_TOPIC_PER_BROKER"` in prod
- MSK: `ebs_volume_size = 100` (GB per broker), configurable via Pulumi config
- Schema Registry: Fargate, 0.25 vCPU / 512MB, health check on `:8081/subjects`
- All resources tagged consistently

## Dependencies

- Consumes: `NetworkOutputs` from Infra-1 (VPC, private subnets, `msk-sg`)
- Consumed by: Infra-4 (ECS services need bootstrap brokers for env vars)

## Work Tracking

```bash
gh issue list --label "infra-3" --state open
gh issue view <number>
```
