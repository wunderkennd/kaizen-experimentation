# IaC Implementation Plan — Kaizen on AWS with Pulumi + Go

**Status**: Planning (2026-04-06)
**Owner**: Multiclaude (5 agents, supervisor daemon)
**Stack**: Pulumi + Go, targeting AWS
**Sprint length**: ~1 week each; 3 sprints total (I.0–I.2)

---

## Agent Roster

| Agent | Domain | Scope |
|-------|--------|-------|
| Infra-1 | Networking & Foundation | VPC, subnets, SGs, NAT, VPC endpoints, Cloud Map |
| Infra-2 | Data Stores | RDS PostgreSQL, ElastiCache Redis, S3 buckets, Secrets Manager |
| Infra-3 | Streaming | MSK Kafka, topic provisioning, Schema Registry (ECS) |
| Infra-4 | Compute & Services | ECS cluster, Fargate task defs, M4b EC2, autoscaling, ECR |
| Infra-5 | Ingress, Observability & DNS | ALB, ACM, Route 53, WAF, AMP, AMG, CloudWatch, X-Ray sidecar |

---

## Sprint Overview

| Sprint | Duration | Theme | Parallelism |
|--------|----------|-------|-------------|
| I.0 | Week 1 | Scaffold + Foundation | All 5 agents |
| I.1 | Week 2 | Services + Wiring | Infra-2, -3, -4, -5 |
| I.2 | Week 3 | Integration + Hardening | All 5 agents |

---

## Dependency Graph

```
Sprint I.0 (parallel):
  Infra-1: VPC + subnets + SGs + Cloud Map     ─┐
  Infra-2: Pulumi project scaffold + config pkg  │ (no runtime deps, just code)
  Infra-3: MSK module (code only)                │
  Infra-4: ECS cluster module + ECR repos (code) │
  Infra-5: ALB module + ACM + DNS (code)         │
                                                  │
Sprint I.1 (parallel, after I.0 merges):          │
  Infra-1: VPC endpoints + IAM roles + policies ◄┘
  Infra-2: RDS + Redis + S3 + Secrets Manager (wired to VPC outputs)
  Infra-3: MSK cluster + topics (wired to VPC outputs)
  Infra-4: 9 ECS service definitions + M4b EC2 + autoscaling
  Infra-5: ALB target groups + listener rules + monitoring stack

Sprint I.2 (parallel, after I.1 merges):
  Infra-1: Pulumi integration tests (networking)
  Infra-2: DB migration init container + smoke tests
  Infra-3: Topic health check + Schema Registry ECS service
  Infra-4: Service dependency ordering + health gates + E2E deploy test
  Infra-5: WAF rules + alarm definitions + Grafana dashboards
```

---

## Sprint I.0 — Scaffold + Foundation

All agents work in parallel on code-only modules. No cross-agent dependencies.

| # | Task | Agent | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| I.0.1 | Pulumi project scaffold (`Pulumi.yaml`, `main.go`, `go.mod`, `pkg/config/`) | Infra-2 | Planned | — | `pulumi new go`, stack configs for dev/staging/prod, shared config types |
| I.0.2 | VPC module (`pkg/network/vpc.go`) | Infra-1 | Planned | — | VPC, 3 public + 3 private subnets, IGW, 2 NAT GWs, route tables |
| I.0.3 | Security groups module (`pkg/network/security_groups.go`) | Infra-1 | Planned | — | 6 SGs: `alb`, `ecs`, `rds`, `msk`, `redis`, `m4b` with cross-refs |
| I.0.4 | Cloud Map namespace (`pkg/network/service_discovery.go`) | Infra-1 | Planned | — | `kaizen.local` private DNS namespace |
| I.0.5 | RDS module (`pkg/database/rds.go`) | Infra-2 | Planned | — | PG 16 instance, parameter group, subnet group, code only |
| I.0.6 | ElastiCache module (`pkg/cache/redis.go`) | Infra-2 | Planned | — | Redis 7 replication group, subnet group, code only |
| I.0.7 | S3 module (`pkg/storage/s3.go`) | Infra-2 | Planned | — | 3 buckets: data (Delta), mlflow, logs; lifecycle rules |
| I.0.8 | Secrets module (`pkg/secrets/secrets.go`) | Infra-2 | Planned | — | 4 secrets: database, kafka, redis, auth; rotation config |
| I.0.9 | MSK module (`pkg/streaming/msk.go`) | Infra-3 | Planned | — | 3-broker cluster, config, encryption at rest + in transit |
| I.0.10 | Kafka topics module (`pkg/streaming/topics.go`) | Infra-3 | Planned | — | 8 topics with partition/retention config matching `kafka/topic_configs.sh` |
| I.0.11 | ECS cluster module (`pkg/compute/cluster.go`) | Infra-4 | Planned | — | Cluster + Fargate capacity providers + EC2 capacity provider for M4b |
| I.0.12 | ECR repositories (`pkg/cicd/ecr.go`) | Infra-4 | Planned | — | 9 repos, lifecycle: keep last 10 images |
| I.0.13 | ALB module (`pkg/loadbalancer/alb.go`) | Infra-5 | Planned | — | Internet-facing ALB, HTTPS listener, gRPC-aware config |
| I.0.14 | ACM + Route 53 (`pkg/dns/dns.go`) | Infra-5 | Planned | — | Wildcard cert, hosted zone, A records |

---

## Sprint I.1 — Services + Wiring

Modules consume VPC/subnet/SG outputs from I.0. Each agent wires their resources to the shared foundation.

| # | Task | Agent | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| I.1.1 | VPC endpoints + IAM | Infra-1 | Planned | — | S3 gateway, ECR, CW Logs, Secrets Manager endpoints; ECS task role + execution role |
| I.1.2 | Wire RDS to VPC | Infra-2 | Planned | — | Subnet group → private subnets, SG → `rds-sg`, Multi-AZ, backup config |
| I.1.3 | Wire Redis to VPC | Infra-2 | Planned | — | Subnet group → private subnets, SG → `redis-sg`, encryption |
| I.1.4 | Wire S3 + Secrets | Infra-2 | Planned | — | Bucket policies, secret values (reference RDS/MSK/Redis outputs) |
| I.1.5 | Wire MSK to VPC | Infra-3 | Planned | — | Broker subnets → private, SG → `msk-sg`, SASL/SCRAM auth |
| I.1.6 | Provision Kafka topics | Infra-3 | Planned | — | Use Pulumi Kafka provider against MSK bootstrap servers |
| I.1.7 | Schema Registry ECS service | Infra-3 | Planned | — | Confluent Schema Registry on Fargate, wired to MSK |
| I.1.8 | M1 Assignment ECS service | Infra-4 | Planned | — | Fargate, 0.5 vCPU/1GB, port 50051, Cloud Map `m1-assignment.kaizen.local` |
| I.1.9 | M2 Pipeline ECS service | Infra-4 | Planned | — | Fargate, 0.5 vCPU/1GB, port 50052 |
| I.1.10 | M2-Orch ECS service | Infra-4 | Planned | — | Fargate, 0.25 vCPU/512MB, port 50058 |
| I.1.11 | M3 Metrics ECS service | Infra-4 | Planned | — | Fargate, 1.0 vCPU/2GB, port 50056 + 50059 (metrics) |
| I.1.12 | M4a Analysis ECS service | Infra-4 | Planned | — | Fargate, 1.0 vCPU/2GB, port 50053 |
| I.1.13 | M4b Policy EC2 + EBS | Infra-4 | Planned | — | c6i.xlarge, 20GB gp3 EBS at /data/rocksdb, port 50054, Cloud Map registration |
| I.1.14 | M5 Management ECS service | Infra-4 | Planned | — | Fargate, 0.5 vCPU/1GB, port 50055 + 50060 (metrics) |
| I.1.15 | M6 UI ECS service | Infra-4 | Planned | — | Fargate, 0.5 vCPU/1GB, port 3000 |
| I.1.16 | M7 Flags ECS service | Infra-4 | Planned | — | Fargate, 0.25 vCPU/512MB, port 50057 |
| I.1.17 | Autoscaling policies | Infra-4 | Planned | — | Target-tracking per service (CPU 70%, request count for M1/M7) |
| I.1.18 | ALB target groups + listener rules | Infra-5 | Planned | — | 4 public TGs: M1 (gRPC), M5 (ConnectRPC), M6 (HTTP), M7 (gRPC); path routing |
| I.1.19 | CloudWatch log groups + alarms | Infra-5 | Planned | — | 9 log groups (30d retention), p99 latency alarms, error rate alarms |
| I.1.20 | AMP + AMG workspace | Infra-5 | Planned | — | Managed Prometheus + Grafana, scrape config for ECS tasks |

---

## Sprint I.2 — Integration + Hardening

End-to-end validation, security hardening, and operational polish.

| # | Task | Agent | Status | PR | Notes |
|---|------|-------|--------|-----|-------|
| I.2.1 | Pulumi integration tests (networking) | Infra-1 | Planned | — | `test/network_test.go`: VPC CIDR, subnet count, SG rules, Cloud Map |
| I.2.2 | DB migration init container | Infra-2 | Planned | — | ECS task runs `golang-migrate` against RDS on deploy |
| I.2.3 | Data store smoke tests | Infra-2 | Planned | — | `test/datastore_test.go`: RDS connectivity, Redis PING, S3 put/get |
| I.2.4 | Schema Registry health gate | Infra-3 | Planned | — | ECS health check, topic list verification post-deploy |
| I.2.5 | Service dependency ordering | Infra-4 | Planned | — | M5 starts first (owns PG schema), then M1/M2/M4b, then M3/M4a/M6/M7 |
| I.2.6 | E2E deploy smoke test | Infra-4 | Planned | — | `test/compute_test.go`: all 9 services healthy, gRPC health checks pass |
| I.2.7 | WAF rules | Infra-5 | Planned | — | Rate limiting (1000 rps/IP), geo-restriction (optional), SQL injection rules |
| I.2.8 | Grafana dashboard provisioning | Infra-5 | Planned | — | Pre-loaded dashboards: service latency, Kafka lag, RDS connections, M4b RocksDB |
| I.2.9 | X-Ray / ADOT sidecar | Infra-5 | Planned | — | OTEL collector sidecar in each ECS task def |
| I.2.10 | `main.go` integration — wire all modules | All | Planned | — | Final wiring: all modules composed in `main.go` with proper output exports |

---

## Repository Layout

```
infra/
  Pulumi.yaml
  Pulumi.dev.yaml
  Pulumi.staging.yaml
  Pulumi.prod.yaml
  main.go                        # Composes all modules
  go.mod
  pkg/
    config/
      config.go                  # Shared types, Pulumi config reader
    network/
      vpc.go                     # Infra-1
      security_groups.go         # Infra-1
      service_discovery.go       # Infra-1
      vpc_endpoints.go           # Infra-1
      iam.go                     # Infra-1
    database/
      rds.go                     # Infra-2
    cache/
      redis.go                   # Infra-2
    storage/
      s3.go                      # Infra-2
    secrets/
      secrets.go                 # Infra-2
    streaming/
      msk.go                     # Infra-3
      topics.go                  # Infra-3
      schema_registry.go         # Infra-3
    compute/
      cluster.go                 # Infra-4
      services.go                # Infra-4 (M1, M2, M2-Orch, M3, M4a, M5, M6, M7)
      m4b.go                     # Infra-4 (EC2 + EBS special case)
      autoscaling.go             # Infra-4
    loadbalancer/
      alb.go                     # Infra-5
    dns/
      dns.go                     # Infra-5
    observability/
      monitoring.go              # Infra-5
      dashboards.go              # Infra-5
    cicd/
      ecr.go                     # Infra-4
  test/
    network_test.go              # Infra-1
    datastore_test.go            # Infra-2
    compute_test.go              # Infra-4
    infra_test.go                # Shared / integration
```

---

## Agent → File Ownership

| Agent | Owns | Reads (cross-agent) |
|-------|------|---------------------|
| Infra-1 | `pkg/network/*`, `test/network_test.go` | `pkg/config/` |
| Infra-2 | `pkg/database/*`, `pkg/cache/*`, `pkg/storage/*`, `pkg/secrets/*`, `test/datastore_test.go` | `pkg/config/`, `pkg/network/` (VPC outputs) |
| Infra-3 | `pkg/streaming/*` | `pkg/config/`, `pkg/network/` (VPC outputs) |
| Infra-4 | `pkg/compute/*`, `pkg/cicd/*`, `test/compute_test.go` | `pkg/config/`, `pkg/network/`, `pkg/database/`, `pkg/streaming/`, `pkg/secrets/` |
| Infra-5 | `pkg/loadbalancer/*`, `pkg/dns/*`, `pkg/observability/*` | `pkg/config/`, `pkg/network/`, `pkg/compute/` |

---

## Cross-Agent Interface Contracts

Each module exposes a Go struct of outputs that downstream modules consume:

```go
// pkg/network/ exports:
type NetworkOutputs struct {
    VpcId             pulumi.IDOutput
    PrivateSubnetIds  pulumi.StringArrayOutput
    PublicSubnetIds   pulumi.StringArrayOutput
    SecurityGroups    map[string]pulumi.IDOutput  // "alb", "ecs", "rds", "msk", "redis", "m4b"
    CloudMapNamespace pulumi.IDOutput
}

// pkg/database/ exports:
type DatabaseOutputs struct {
    RdsEndpoint    pulumi.StringOutput
    RdsPort        pulumi.IntOutput
    RedisEndpoint  pulumi.StringOutput
    RedisPort      pulumi.IntOutput
}

// pkg/streaming/ exports:
type StreamingOutputs struct {
    MskBootstrapBrokers pulumi.StringOutput
    SchemaRegistryUrl   pulumi.StringOutput
}

// pkg/secrets/ exports:
type SecretsOutputs struct {
    DatabaseSecretArn pulumi.StringOutput
    KafkaSecretArn    pulumi.StringOutput
    RedisSecretArn    pulumi.StringOutput
    AuthSecretArn     pulumi.StringOutput
}

// pkg/compute/ exports:
type ComputeOutputs struct {
    ClusterId       pulumi.IDOutput
    ServiceArns     map[string]pulumi.StringOutput  // "m1", "m2", ..., "m7"
    TaskRoleArn     pulumi.StringOutput
    ExecRoleArn     pulumi.StringOutput
}
```

**Contract rule**: Infra-2 defines `pkg/config/config.go` in Sprint I.0. All agents import it. Output struct shapes are agreed upon in Sprint I.0 PRs and must not change without a coordinated update.

---

## Environment-Specific Configuration

| Setting | Dev | Staging | Prod |
|---------|-----|---------|------|
| RDS instance | `db.t4g.medium` | `db.r6g.large` | `db.r6g.large` |
| RDS Multi-AZ | No | Yes | Yes |
| MSK brokers | 2x `kafka.t3.small` | 3x `kafka.m5.large` | 3x `kafka.m5.large` |
| Redis node | `cache.t4g.medium` | `cache.r6g.large` | `cache.r6g.large` |
| M4b instance | `t3.large` | `c6i.xlarge` | `c6i.xlarge` |
| NAT Gateways | 1 | 2 | 2 |
| WAF | Off | On | On |
| Fargate min tasks | 1 each | 1 each | 2 (M1, M2, M7), 1 (others) |
| CloudWatch retention | 7 days | 14 days | 30 days |

---

## CI Checks for IaC PRs

```bash
# All IaC PRs must pass:
cd infra && go build ./...           # Compiles
cd infra && go vet ./...             # Static analysis
cd infra && go test ./...            # Pulumi unit tests (mocked)
pulumi preview --stack dev           # Dry-run against dev stack (optional, CI secret needed)
```

---

## Risk Register

| Risk | Severity | Mitigation |
|------|----------|------------|
| MSK provisioning takes 20-30 min | Low | Provision early in CI; cache state |
| M4b EC2 + EBS mount complexity | Medium | Detailed user data script; test in dev first |
| Kafka topic provider needs MSK bootstrap | Medium | Topics module depends on MSK output; tested in I.2 |
| Cross-agent output struct drift | Medium | Shared `pkg/config/` types agreed in I.0; contract tests in I.2 |
| Pulumi state conflicts with parallel agents | Low | Each agent works in own package; `main.go` wiring is single-owner (I.2.10) |

---

## Launching Workers

```bash
# Sprint I.0 — all 5 agents in parallel
just evening I.0

# Or manually per-issue:
gh issue view <N> --json body -q '.body' | head -50
# → feed to multiclaude worker
```

---

## Estimated Timeline

| Week | Activity |
|------|----------|
| Week 1 | Sprint I.0: scaffold + all modules (code only) |
| Week 2 | Sprint I.1: wire to VPC, provision resources, define services |
| Week 3 | Sprint I.2: integration tests, hardening, WAF, dashboards |
| Week 3+ | First `pulumi up --stack dev` deploys full environment |
