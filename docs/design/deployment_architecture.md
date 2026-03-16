# Deployment Architecture Guide

## Kaizen Experimentation Platform

**Version**: 1.0 | **Last Updated**: March 2026

---

## Table of Contents

1. [Platform Overview](#1-platform-overview)
2. [Service Inventory](#2-service-inventory)
3. [Infrastructure Dependencies](#3-infrastructure-dependencies)
4. [Deployment Topologies](#4-deployment-topologies)
5. [Fly.io Deployment](#5-flyio-deployment)
6. [AWS Deployment](#6-aws-deployment)
7. [GCP Deployment](#7-gcp-deployment)
8. [Hybrid Deployment](#8-hybrid-deployment)
9. [Networking & Service Communication](#9-networking--service-communication)
10. [Data Layer](#10-data-layer)
11. [Observability](#11-observability)
12. [Scaling Strategy](#12-scaling-strategy)
13. [Security](#13-security)
14. [CI/CD Pipeline](#14-cicd-pipeline)
15. [Disaster Recovery](#15-disaster-recovery)
16. [Cost Estimation](#16-cost-estimation)
17. [Migration Playbook](#17-migration-playbook)
18. [Decision Matrix](#18-decision-matrix)

---

## 1. Platform Overview

Kaizen Experimentation is a multi-service platform for running A/B tests, multi-armed bandits, interleaving experiments, and feature flags. The platform comprises **9 services** written in **3 languages** (Rust, Go, TypeScript), backed by **5 infrastructure components** (PostgreSQL, Kafka, Redis, Delta Lake/MLflow, RocksDB).

### Architecture Diagram

```
                                    +-----------------+
                                    |  Client SDKs    |
                                    | Web/iOS/Android |
                                    | Go/Python       |
                                    +--------+--------+
                                             |
                              gRPC/ConnectRPC|
                                             v
+---------------------------+  +---------------------------+  +---------------------------+
| M1: Assignment Service    |  | M2: Event Pipeline        |  | M7: Feature Flag Service  |
| (Rust) :50051             |  | (Rust) :50052             |  | (Go)  :50057              |
| Variant allocation        |  | Validation, dedup         |  | Progressive delivery      |
| Interleaving              |  | Kafka publish             |  | CGo hash bridge           |
| Bandit arm delegation     |  | Crash-only                |  |                           |
+----------+----------------+  +----------+----------------+  +---------------------------+
           |                              |
           | gRPC                         | Kafka
           v                              v
+---------------------------+  +---------------------------+
| M4b: Bandit Policy        |  | Kafka Cluster             |
| (Rust) :50054             |  | 6 topics, 4-128 partitions|
| Thompson, LinUCB, Neural  |  | exposures, metric_events  |
| LMAX single-thread core   |  | reward_events, qoe_events |
| RocksDB snapshots         |  | guardrail_alerts,         |
|                           |  | surrogate_recalib_reqs    |
+---------------------------+  +----------+----------------+
                                          |
                                          v
+---------------------------+  +---------------------------+  +---------------------------+
| M3: Metric Engine         |  | M4a: Analysis Engine      |  | M5: Management Service    |
| (Go)  :50056              |  | (Rust) :50053             |  | (Go)  :50055              |
| Spark SQL orchestration   |  | Frequentist, mSPRT, GST   |  | CRUD, lifecycle           |
| Surrogates, QoE           |  | CUPED, novelty            |  | Auto-pause guardrails     |
| Lifecycle segmentation    |  | Interference detection    |  | Bucket reuse management   |
+---------------------------+  +---------------------------+  +---------------------------+
                                                              |
                                                              | ConnectRPC
                                                              v
                                                   +---------------------------+
                                                   | M2-Orch: Orchestration    |
                                                   | (Go)  :50058              |
                                                   | SQL query logging         |
                                                   +---------------------------+
                                                              |
                                                              v
                                                   +---------------------------+
                                                   | M6: Decision Support UI   |
                                                   | (TypeScript/Next.js) :3000|
                                                   | Dashboards, View SQL      |
                                                   | Export to Notebook         |
                                                   +---------------------------+
```

---

## 2. Service Inventory

### Service Port Map

| Module | Service | Language | Port | Protocol | Stateful? | Latency SLA |
|--------|---------|----------|------|----------|-----------|-------------|
| M1 | Assignment | Rust | 50051 | gRPC | No | p99 < 5ms |
| M2 | Event Pipeline | Rust | 50052 | gRPC | No | p99 < 10ms |
| M2-Orch | Orchestration | Go | 50058 | ConnectRPC | No | N/A (batch) |
| M3 | Metric Engine | Go | 50056 | gRPC | No | N/A (batch) |
| M4a | Analysis Engine | Rust | 50053 | gRPC | No | N/A (batch) |
| M4b | Bandit Policy | Rust | 50054 | gRPC | Yes (RocksDB) | p99 < 15ms |
| M5 | Management | Go | 50055 | ConnectRPC | No (uses PG) | p99 < 50ms |
| M6 | UI | TypeScript | 3000 | HTTP | No | N/A (frontend) |
| M7 | Feature Flags | Go | 50057 | gRPC | No | p99 < 10ms |

### Prometheus Metrics Port

| Service | Metrics Endpoint |
|---------|-----------------|
| M3 (Metrics) | :50059 |
| All others | Same as service port (`/metrics` path) |

### Docker Images

All services use multi-stage Docker builds:

- **Rust services**: `rust:1.80-slim` builder -> `debian:bookworm-slim` runtime (~50-80MB)
- **Go services**: `golang:1.22-bookworm` builder -> `gcr.io/distroless/static-debian12` runtime (~15-25MB)
- **UI**: `node:20-alpine` builder -> `node:20-alpine` runtime (~120MB)

Build all images:
```bash
just docker-build
```

Build a single service:
```bash
just docker-build-svc assignment
```

---

## 3. Infrastructure Dependencies

### Required Infrastructure

| Component | Purpose | Local Dev Image | Production Requirement |
|-----------|---------|-----------------|----------------------|
| **PostgreSQL 16** | Config, results, audit trail, query log | `postgres:16-alpine` | Managed (RDS/Cloud SQL/Fly Postgres) |
| **Kafka** (Confluent 7.7) | Event streaming (6 topics) | `confluentinc/cp-kafka:7.7.0` | Managed (MSK/Confluent Cloud) or self-hosted |
| **ZooKeeper** | Kafka coordination | `confluentinc/cp-zookeeper:7.7.0` | Bundled with Kafka (KRaft mode in production) |
| **Schema Registry** | Avro/Protobuf schema enforcement | `confluentinc/cp-schema-registry:7.7.0` | Confluent Cloud or self-hosted |
| **Redis 7** | Feature store, caching | `redis:7-alpine` | Managed (ElastiCache/Memorystore/Upstash) |
| **RocksDB** | Bandit policy state (M4b local) | Embedded in M4b binary | Persistent volume on M4b container |
| **Delta Lake + MLflow** | Metric storage, surrogate models | External dependency | S3/GCS + managed MLflow |

### Kafka Topics

| Topic | Partitions | Purpose | Producers | Consumers |
|-------|-----------|---------|-----------|-----------|
| `exposures` | 64 | Assignment events | M2 | M3, M4a |
| `metric_events` | 128 | User behavior metrics | M2 | M3 |
| `reward_events` | 32 | Bandit reward signals | M2 | M4b, M3 |
| `qoe_events` | 64 | Playback quality events | M2 | M3 |
| `guardrail_alerts` | 8 | Auto-pause triggers | M3 | M5 |
| `surrogate_recalibration_requests` | 4 | Surrogate model recalibration | M5 | M3 |

### PostgreSQL Schema

The database uses 4 schema domains:

1. **Config** (M5 owns): `layers`, `experiments`, `variants`, `targeting_rules`, `metric_definitions`, `guardrail_configs`, `layer_allocations`, `surrogate_models`
2. **Results** (M4a writes, M6 reads): `analysis_results`, `metric_results`, `surrogate_projections`, `novelty_analysis_results`, `interference_analysis_results`
3. **Query Log** (M3 writes, M6 reads): `query_log`
4. **Audit** (M5 writes): `audit_trail`, `policy_snapshots`

---

## 4. Deployment Topologies

### Topology Options

```
┌─────────────────────────────────────────────────────────────────────┐
│                        DEPLOYMENT TOPOLOGIES                        │
├───────────────┬──────────────────┬──────────────────────────────────┤
│   Fly.io      │   AWS / GCP      │   Hybrid                        │
│   (Simple)    │   (Enterprise)   │   (Optimal)                     │
├───────────────┼──────────────────┼──────────────────────────────────┤
│ All 9 services│ All 9 services   │ M1, M7 on Fly.io edge           │
│ on Fly.io VMs │ on ECS/Cloud Run │ M4b, M3, M5 on AWS/GCP          │
│               │                  │ Managed infra on AWS/GCP         │
│ Fly Postgres  │ RDS / Cloud SQL  │ RDS / Cloud SQL                  │
│ Upstash Redis │ ElastiCache      │ ElastiCache / Memorystore        │
│ Upstash Kafka │ MSK / Pub-Sub    │ MSK / Confluent Cloud            │
│               │ or Confluent     │                                  │
├───────────────┼──────────────────┼──────────────────────────────────┤
│ Best for:     │ Best for:        │ Best for:                        │
│ Dev/staging   │ Production at    │ Global low-latency + managed     │
│ Small-scale   │ scale, strict    │ stateful backends                │
│ prototyping   │ compliance       │                                  │
├───────────────┼──────────────────┼──────────────────────────────────┤
│ Monthly cost  │ Monthly cost     │ Monthly cost                     │
│ ~$50-200      │ ~$500-2,000+     │ ~$300-1,200                      │
└───────────────┴──────────────────┴──────────────────────────────────┘
```

---

## 5. Fly.io Deployment

### Overview

Fly.io provides the simplest path from Docker images to running services. Each service gets a `fly.toml` config file and runs as a Fly Machine (microVM).

### Architecture

```
                    ┌──────────────────────────────┐
                    │         Fly.io Edge           │
                    │    (Anycast, auto-TLS)        │
                    └──────────┬───────────────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
     ┌────────▼─────┐  ┌──────▼───────┐  ┌─────▼────────┐
     │ M1: Assignment│  │ M7: Flags    │  │ M6: UI       │
     │ ord/iad/cdg   │  │ ord/iad      │  │ ord          │
     │ 2x shared-2x  │  │ 2x shared-1x │  │ 1x shared-1x │
     └───────┬───────┘  └──────────────┘  └──────────────┘
             │ .internal
     ┌───────▼───────────────────────────────────────────┐
     │              Internal Network (.internal)          │
     ├───────────┬────────────┬────────────┬─────────────┤
     │ M4b:Policy│ M5:Mgmt    │ M2:Pipeline│ M2-Orch     │
     │ 1x perf-2x│ 1x shared-1x│ 2x shared-1x│ 1x shared-1x│
     │ +volume   │            │            │             │
     └───────────┴─────┬──────┴────────────┴─────────────┘
                       │
          ┌────────────┼────────────┐
          │            │            │
   ┌──────▼───┐  ┌─────▼────┐  ┌───▼─────┐
   │Fly Postgres│ │Upstash   │ │Upstash  │
   │ (ha)      │ │Redis     │ │Kafka    │
   └───────────┘ └──────────┘ └─────────┘
```

### Service Configuration (`fly.toml` examples)

**M1: Assignment Service (latency-critical, multi-region)**

```toml
# fly.toml — experimentation-assignment
app = "kaizen-assignment"
primary_region = "ord"            # Chicago

[build]
  dockerfile = "crates/experimentation-assignment/Dockerfile"

[env]
  RUST_LOG = "info"
  MANAGEMENT_ADDR = "kaizen-management.internal:50055"
  POLICY_ADDR = "kaizen-policy.internal:50054"

[[services]]
  internal_port = 50051
  protocol = "tcp"

  [[services.ports]]
    port = 443
    handlers = ["tls"]

  [services.concurrency]
    type = "connections"
    hard_limit = 500
    soft_limit = 400

[[vm]]
  size = "shared-cpu-2x"
  memory = "1gb"
  count = 2

# Deploy to multiple regions for edge latency
[regions]
  default = ["ord", "iad", "cdg"]
```

**M4b: Bandit Policy Service (stateful, needs persistent volume)**

```toml
# fly.toml — experimentation-policy
app = "kaizen-policy"
primary_region = "ord"

[build]
  dockerfile = "crates/experimentation-policy/Dockerfile"

[env]
  RUST_LOG = "info"
  POLICY_ROCKSDB_PATH = "/data/policy.db"
  KAFKA_BROKERS = "your-upstash-kafka-endpoint:9092"

[mounts]
  source = "policy_data"
  destination = "/data"
  initial_size = "5gb"

[[services]]
  internal_port = 50054
  protocol = "tcp"
  auto_stop_machines = false    # Always running — policy state in memory

[[vm]]
  size = "performance-2x"       # CPU-intensive: Thompson MC, LinUCB
  memory = "4gb"
  count = 1                     # Single instance (LMAX single-thread design)
```

**M5: Management Service**

```toml
# fly.toml — experimentation-management
app = "kaizen-management"
primary_region = "ord"

[build]
  dockerfile = "services/management/Dockerfile"

[env]
  DATABASE_URL = "postgres://..."   # Use fly secrets for credentials
  KAFKA_BROKERS = "your-upstash-kafka-endpoint:9092"

[[services]]
  internal_port = 50055
  protocol = "tcp"

  [[services.ports]]
    port = 443
    handlers = ["tls"]

[[vm]]
  size = "shared-cpu-1x"
  memory = "512mb"
  count = 1
```

**M6: UI**

```toml
# fly.toml — experimentation-ui
app = "kaizen-ui"
primary_region = "ord"

[build]
  dockerfile = "ui/Dockerfile"

[env]
  MANAGEMENT_API_URL = "https://kaizen-management.fly.dev"

[[services]]
  internal_port = 3000
  protocol = "tcp"

  [[services.ports]]
    port = 80
    handlers = ["http"]
  [[services.ports]]
    port = 443
    handlers = ["tls", "http"]

[[vm]]
  size = "shared-cpu-1x"
  memory = "256mb"
  count = 1
```

### Fly.io Infrastructure Setup

```bash
# 1. Create Fly apps for each service
for svc in assignment pipeline analysis policy management metrics flags orchestration ui; do
  fly apps create kaizen-$svc
done

# 2. Create Fly Postgres cluster (HA)
fly postgres create --name kaizen-db --region ord --vm-size shared-cpu-2x --volume-size 10

# 3. Attach Postgres to management service
fly postgres attach kaizen-db --app kaizen-management

# 4. Create persistent volume for M4b RocksDB
fly volumes create policy_data --app kaizen-policy --region ord --size 5

# 5. Set secrets
fly secrets set DATABASE_URL="postgres://..." --app kaizen-management
fly secrets set KAFKA_BROKERS="..." --app kaizen-pipeline
fly secrets set REDIS_URL="..." --app kaizen-flags

# 6. Deploy all services
for svc in assignment pipeline analysis policy management metrics flags orchestration ui; do
  fly deploy --app kaizen-$svc
done
```

### Fly.io Tradeoffs

| Aspect | Assessment |
|--------|-----------|
| **Setup complexity** | Low — Docker-native, `fly.toml` per service |
| **Edge latency** | Excellent — anycast routing, multi-region VMs |
| **PostgreSQL** | Weak — Fly Postgres is "not managed" (no automated failover, no PITR, no SLA). TOCTOU transaction patterns (`SELECT ... FOR UPDATE`) need reliable DB. Consider external managed Postgres (Neon, Supabase, Crunchy Bridge) |
| **Kafka** | Not native — requires Upstash Kafka or external. Upstash Kafka has 1MB max message size and limited partition counts |
| **Redis** | Via Upstash — good for caching, limited for pub/sub workloads |
| **RocksDB (M4b)** | Single-attach volumes only. No cross-region replication. Volume loss = policy state loss (recovered from Kafka replay, but takes time) |
| **Autoscaling** | Manual machine count changes only. No metric-based autoscaling. Thompson MC (1000 draws x K arms) is CPU-intensive |
| **Networking** | `.internal` DNS for inter-service. No service mesh, no mTLS between services, no circuit breaking |
| **Observability** | Fly Metrics (basic). Need external Prometheus/Grafana for production-grade monitoring |
| **Cost** | Very low floor (~$50/month for minimal setup). Linear scaling |
| **Compliance** | SOC 2 Type II. No HIPAA BAA available |

---

## 6. AWS Deployment

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              AWS VPC                                    │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │                        ECS Cluster                                 │ │
│  │                                                                    │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │ │
│  │  │ M1:Assignment │  │ M7:Flags     │  │ M6:UI        │            │ │
│  │  │ Fargate      │  │ Fargate      │  │ Fargate      │            │ │
│  │  │ 2 tasks      │  │ 2 tasks      │  │ 1 task       │            │ │
│  │  │ 0.5 vCPU/1GB │  │ 0.25 vCPU    │  │ 0.25 vCPU    │            │ │
│  │  └──────┬───────┘  └──────────────┘  └──────────────┘            │ │
│  │         │                                                          │ │
│  │  ┌──────▼───────┐  ┌──────────────┐  ┌──────────────┐            │ │
│  │  │ M4b:Policy   │  │ M5:Management│  │ M2:Pipeline  │            │ │
│  │  │ EC2 (EBS vol)│  │ Fargate      │  │ Fargate      │            │ │
│  │  │ c6i.xlarge   │  │ 0.5 vCPU/1GB │  │ 2 tasks      │            │ │
│  │  │ RocksDB on   │  │              │  │ 0.25 vCPU    │            │ │
│  │  │ gp3 EBS      │  │              │  │              │            │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘            │ │
│  │                                                                    │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐            │ │
│  │  │ M3:Metrics   │  │ M4a:Analysis │  │ M2-Orch      │            │ │
│  │  │ Fargate      │  │ Fargate      │  │ Fargate      │            │ │
│  │  │ 0.5 vCPU/1GB │  │ 1 vCPU/2GB   │  │ 0.25 vCPU    │            │ │
│  │  └──────────────┘  └──────────────┘  └──────────────┘            │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│                                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                  │
│  │ RDS Postgres  │  │ Amazon MSK   │  │ ElastiCache  │                  │
│  │ db.r6g.large  │  │ 3 brokers    │  │ Redis r6g    │                  │
│  │ Multi-AZ      │  │ kafka.m5.lg  │  │ cluster mode │                  │
│  │ auto-backup   │  │              │  │              │                  │
│  └──────────────┘  └──────────────┘  └──────────────┘                  │
│                                                                         │
│  ┌──────────────┐  ┌──────────────┐                                    │
│  │ S3 (Delta)   │  │ CloudWatch   │                                    │
│  │ + MLflow     │  │ + Grafana    │                                    │
│  └──────────────┘  └──────────────┘                                    │
└─────────────────────────────────────────────────────────────────────────┘
         │
    ┌────▼─────────┐
    │ ALB + WAF     │
    │ (TLS term,    │
    │  gRPC support)│
    └──────────────┘
```

### ECS Task Definitions

**M1: Assignment Service**

```json
{
  "family": "kaizen-assignment",
  "networkMode": "awsvpc",
  "requiresCompatibilities": ["FARGATE"],
  "cpu": "512",
  "memory": "1024",
  "containerDefinitions": [{
    "name": "assignment",
    "image": "ECR_REPO/experimentation-assignment:latest",
    "portMappings": [{"containerPort": 50051, "protocol": "tcp"}],
    "environment": [
      {"name": "RUST_LOG", "value": "info"},
      {"name": "MANAGEMENT_ADDR", "value": "management.kaizen.local:50055"},
      {"name": "POLICY_ADDR", "value": "policy.kaizen.local:50054"}
    ],
    "healthCheck": {
      "command": ["CMD-SHELL", "grpc_health_probe -addr=:50051 || exit 1"],
      "interval": 10,
      "timeout": 5,
      "retries": 3
    },
    "logConfiguration": {
      "logDriver": "awslogs",
      "options": {
        "awslogs-group": "/ecs/kaizen-assignment",
        "awslogs-region": "us-east-1",
        "awslogs-stream-prefix": "ecs"
      }
    }
  }]
}
```

**M4b: Bandit Policy — EC2 (not Fargate) for persistent EBS**

M4b is the only service that requires persistent local storage (RocksDB). Use EC2-backed ECS tasks with an EBS gp3 volume, or run on a dedicated EC2 instance.

```bash
# EBS volume for RocksDB (gp3 for consistent IOPS)
aws ec2 create-volume --volume-type gp3 --size 20 --iops 3000 --throughput 125 \
  --availability-zone us-east-1a --tag-specifications 'ResourceType=volume,Tags=[{Key=Name,Value=kaizen-policy-rocksdb}]'
```

### AWS Service Map

| Component | AWS Service | Config |
|-----------|-------------|--------|
| Compute (stateless) | ECS Fargate | Per-service task definitions |
| Compute (M4b) | ECS on EC2 or dedicated EC2 | c6i.xlarge + gp3 EBS |
| Database | RDS PostgreSQL 16 | db.r6g.large, Multi-AZ, automated backups |
| Streaming | Amazon MSK | 3 brokers (kafka.m5.large), 6 topics |
| Cache | ElastiCache Redis | r6g.large, cluster mode |
| Object Storage | S3 | Delta Lake tables, MLflow artifacts |
| Load Balancer | ALB | gRPC support (HTTP/2), TLS termination |
| DNS | Cloud Map | Service discovery (`*.kaizen.local`) |
| Secrets | Secrets Manager | DB credentials, API keys |
| Monitoring | CloudWatch + Prometheus (AMP) | Logs, metrics, alarms |
| Container Registry | ECR | Docker images |
| WAF | AWS WAF | Rate limiting on public endpoints |

### AWS Autoscaling

```yaml
# ECS Service Autoscaling for M1 (Assignment)
Type: AWS::ApplicationAutoScaling::ScalableTarget
Properties:
  MaxCapacity: 20
  MinCapacity: 2
  ResourceId: service/kaizen-cluster/kaizen-assignment
  ScalableDimension: ecs:service:DesiredCount
  ServiceNamespace: ecs

# Scale on request count
Type: AWS::ApplicationAutoScaling::ScalingPolicy
Properties:
  PolicyType: TargetTrackingScaling
  TargetTrackingScalingPolicyConfiguration:
    PredefinedMetricSpecification:
      PredefinedMetricType: ALBRequestCountPerTarget
    TargetValue: 1000     # requests per target per minute
    ScaleInCooldown: 60
    ScaleOutCooldown: 30
```

### AWS Tradeoffs

| Aspect | Assessment |
|--------|-----------|
| **Setup complexity** | High — VPC, subnets, security groups, IAM roles, ALB, service discovery |
| **Managed data stores** | Excellent — RDS (automatic failover, PITR, read replicas), MSK, ElastiCache |
| **Autoscaling** | Native — ECS service autoscaling, custom metrics via CloudWatch |
| **Networking** | Full control — VPC, private subnets, security groups, NACLs, VPN |
| **Observability** | CloudWatch + AMP (managed Prometheus) + AMG (managed Grafana) |
| **M4b (stateful)** | EC2 + EBS — persistent, snapshotable, but not auto-healing |
| **gRPC support** | ALB supports gRPC natively (HTTP/2). NLB for raw TCP |
| **Cost floor** | ~$500-800/month minimum (RDS, MSK, NAT Gateway, ALB) |
| **Compliance** | SOC 2, HIPAA, PCI DSS, FedRAMP |
| **Multi-region** | Possible but complex (Global Accelerator, cross-region replication) |

---

## 7. GCP Deployment

### Architecture

GCP offers Cloud Run for stateless services (better cold-start than Fargate) and GKE for stateful workloads.

### Service Mapping

| Component | GCP Service | Notes |
|-----------|-------------|-------|
| Compute (stateless) | Cloud Run | Auto-scales to zero, gRPC native |
| Compute (M4b) | GKE with PersistentVolumeClaim | StatefulSet for RocksDB |
| Database | Cloud SQL PostgreSQL 16 | HA, automatic backups, read replicas |
| Streaming | Confluent Cloud on GCP or Pub/Sub | MSK equivalent not available on GCP |
| Cache | Memorystore Redis | Managed Redis |
| Object Storage | GCS | Delta Lake tables |
| Load Balancer | Cloud Load Balancing | Native gRPC, global anycast |
| DNS | Cloud DNS | Service discovery |
| Secrets | Secret Manager | Integrated with Cloud Run |
| Monitoring | Cloud Monitoring + managed Prometheus | Native integration |

### Cloud Run Deployment

```bash
# Deploy M1 Assignment to Cloud Run
gcloud run deploy kaizen-assignment \
  --image gcr.io/PROJECT/experimentation-assignment:latest \
  --port 50051 \
  --use-http2 \                    # Required for gRPC
  --cpu 1 \
  --memory 1Gi \
  --min-instances 2 \              # Avoid cold starts on hot path
  --max-instances 20 \
  --concurrency 200 \
  --set-env-vars RUST_LOG=info \
  --set-env-vars MANAGEMENT_ADDR=kaizen-management-HASH.run.app:443 \
  --vpc-connector kaizen-vpc-connector \
  --region us-central1

# Deploy M6 UI to Cloud Run
gcloud run deploy kaizen-ui \
  --image gcr.io/PROJECT/experimentation-ui:latest \
  --port 3000 \
  --cpu 0.5 \
  --memory 256Mi \
  --min-instances 0 \
  --max-instances 5 \
  --allow-unauthenticated
```

### GKE StatefulSet for M4b

```yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: kaizen-policy
spec:
  serviceName: kaizen-policy
  replicas: 1                      # Single instance (LMAX design)
  template:
    spec:
      containers:
      - name: policy
        image: gcr.io/PROJECT/experimentation-policy:latest
        ports:
        - containerPort: 50054
        env:
        - name: POLICY_ROCKSDB_PATH
          value: /data/policy.db
        volumeMounts:
        - name: policy-data
          mountPath: /data
        resources:
          requests:
            cpu: "2"
            memory: "4Gi"
          limits:
            cpu: "4"
            memory: "8Gi"
  volumeClaimTemplates:
  - metadata:
      name: policy-data
    spec:
      accessModes: ["ReadWriteOnce"]
      storageClassName: premium-rwo
      resources:
        requests:
          storage: 20Gi
```

### GCP Tradeoffs

| Aspect | Assessment |
|--------|-----------|
| **Cloud Run** | Excellent for stateless gRPC services. Native HTTP/2, auto-scales to zero |
| **Cold starts** | Cloud Run cold starts ~1-3s for Rust binaries. Set `min-instances=2` for M1/M7 |
| **GKE (M4b)** | Full Kubernetes — StatefulSet with PVC for RocksDB. More operational overhead than EC2+EBS |
| **Cloud SQL** | Comparable to RDS. HA, automatic backups, maintenance windows |
| **Kafka** | No native equivalent. Use Confluent Cloud on GCP or Pub/Sub (different semantics) |
| **Networking** | VPC, Cloud Armor (WAF), Cloud NAT. Serverless VPC Connectors for Cloud Run |
| **Cost** | Cloud Run is pay-per-request (cheaper at low traffic). GKE has cluster overhead |
| **Monitoring** | Cloud Monitoring + managed Prometheus + managed Grafana |
| **Global LB** | Anycast global load balancing — excellent for multi-region |

---

## 8. Hybrid Deployment

### Recommended: Edge + Managed Backend

The hybrid topology puts latency-sensitive, stateless services at the edge (Fly.io) and stateful/batch services on a managed cloud (AWS/GCP).

```
┌─────────────────────────────────────────────────────────────┐
│                    Fly.io Edge (Global)                      │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ M1:Assignment │  │ M7:Flags     │  │ M6:UI        │      │
│  │ ord/iad/cdg   │  │ ord/iad      │  │ ord          │      │
│  │ /lhr/nrt      │  │ /lhr/nrt     │  │              │      │
│  └──────┬───────┘  └──────────────┘  └──────────────┘      │
│         │                                                    │
│         │ Fly-to-AWS private link (WireGuard)               │
└─────────┼────────────────────────────────────────────────────┘
          │
┌─────────▼────────────────────────────────────────────────────┐
│                    AWS us-east-1 (Backend)                    │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ M4b:Policy   │  │ M5:Management│  │ M2:Pipeline  │      │
│  │ EC2+EBS      │  │ Fargate      │  │ Fargate      │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ M3:Metrics   │  │ M4a:Analysis │  │ M2-Orch      │      │
│  │ Fargate      │  │ Fargate      │  │ Fargate      │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ RDS Postgres  │  │ Amazon MSK   │  │ ElastiCache  │      │
│  │ Multi-AZ      │  │              │  │ Redis        │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└──────────────────────────────────────────────────────────────┘
```

### Why Hybrid Works

1. **M1 (Assignment)** is stateless and latency-critical (p99 < 5ms). Fly.io edge regions put it within 10-20ms of any user globally. The deterministic hash-based assignment means every edge instance returns identical results.

2. **M7 (Feature Flags)** is also stateless and latency-sensitive. Benefits from the same edge deployment.

3. **M4b (Bandit Policy)** is stateful (RocksDB) and CPU-intensive (Thompson MC: 1000 draws). Needs a reliable persistent volume and won't benefit from multi-region (single-thread LMAX design means one instance).

4. **M5 (Management)** needs reliable PostgreSQL transactions (`SELECT ... FOR UPDATE` for TOCTOU safety). AWS RDS provides this with automatic failover.

5. **M3/M4a** are batch services. Latency doesn't matter; reliability and access to data stores does.

### Cross-Cloud Networking

Fly.io supports WireGuard-based private networking to external clouds:

```bash
# Create a WireGuard peer from Fly to your AWS VPC
fly wireguard create kaizen-org aws-vpc

# The resulting WireGuard config can be deployed as a
# site-to-site VPN on an EC2 instance or Transit Gateway
```

Alternatively, expose AWS services via PrivateLink or public endpoints with mTLS.

---

## 9. Networking & Service Communication

### Inter-Service Communication Matrix

```
                M1    M2    M2-O  M3    M4a   M4b   M5    M6    M7
M1  (Assign)    -     -     -     -     -     gRPC  gRPC  -     -
M2  (Pipeline)  -     -     -     -     -     -     -     -     -
M2-O(Orch)      -     -     -     -     -     -     CnRPC -     -
M3  (Metrics)   -     -     -     -     -     -     -     -     -
M4a (Analysis)  -     -     -     -     -     -     -     -     -
M4b (Policy)    -     -     -     -     -     -     -     -     -
M5  (Mgmt)      -     -     -     -     CnRPC CnRPC -     -     -
M6  (UI)        -     -     -     -     -     -     CnRPC -     -
M7  (Flags)     CGo   -     -     -     -     -     -     -     -

Async (Kafka):
M2 -> [exposures, metric_events, reward_events, qoe_events] -> M3, M4b
M3 -> [guardrail_alerts] -> M5
```

### Service Discovery

| Platform | Mechanism | Example |
|----------|-----------|---------|
| Fly.io | `.internal` DNS | `kaizen-policy.internal:50054` |
| AWS ECS | Cloud Map / Service Connect | `policy.kaizen.local:50054` |
| GCP Cloud Run | Service URL | `kaizen-policy-HASH.run.app:443` |
| GCP GKE | Kubernetes DNS | `kaizen-policy.default.svc.cluster.local:50054` |

### TLS Configuration

| Path | TLS Requirement |
|------|----------------|
| Client -> Public services (M1, M5, M6, M7) | TLS terminated at load balancer (Fly/ALB/Cloud LB) |
| Internal service-to-service | Plaintext on private network (Fly `.internal`, AWS VPC, GKE cluster) |
| Service -> Managed databases | TLS enforced (RDS, Cloud SQL require TLS by default) |
| Service -> Kafka | TLS + SASL (SCRAM or mTLS depending on provider) |

---

## 10. Data Layer

### PostgreSQL Configuration

**Production sizing (for ~1000 experiments, ~10M events/day)**

| Setting | Value | Rationale |
|---------|-------|-----------|
| Instance | db.r6g.large (2 vCPU, 16GB) | Analysis queries can be memory-intensive |
| Storage | gp3, 100GB, 3000 IOPS | Audit trail + results grow over time |
| Multi-AZ | Yes | Required for TOCTOU transaction safety |
| Backup | Daily automated, 7-day retention | Point-in-time recovery |
| Extensions | `uuid-ossp`, `pgcrypto` | Required by schema |
| `max_connections` | 200 | 9 services x ~20 connections each |
| `shared_buffers` | 4GB (25% of RAM) | Standard tuning |
| `work_mem` | 64MB | Analysis queries with sorts |

### Kafka Configuration

**Production sizing**

| Setting | Value | Rationale |
|---------|-------|-----------|
| Brokers | 3 (kafka.m5.large on MSK) | Replication factor 3 for durability |
| `exposures` | 64 partitions, 90-day retention | High throughput (~50K/sec), keyed by experiment_id |
| `metric_events` | 128 partitions, 90-day retention | Highest volume topic (~100K/sec), keyed by user_id |
| `reward_events` | 32 partitions, 180-day retention | M4b consumer group (~5K/sec), longer retention for bandit replay |
| `qoe_events` | 64 partitions, 90-day retention | QoE metrics (~20K/sec), keyed by session_id |
| `guardrail_alerts` | 8 partitions, 30-day retention | Low volume (~10/hour), audit trail |
| `surrogate_recalibration_requests` | 4 partitions, 30-day retention | Rare (~1/day), keyed by model_id |
| Compression | lz4 | Best throughput/compression ratio |
| `acks` | all | No message loss |

### Redis Configuration

| Setting | Value | Rationale |
|---------|-------|-----------|
| Instance | r6g.large (2 vCPU, 13GB) | Feature store for lifecycle segments |
| Cluster mode | Enabled | Shard across multiple nodes for throughput |
| Persistence | RDB snapshots every 5 min | Feature store can be rebuilt from source |
| Eviction | `allkeys-lru` | Cache semantics, rebuilt on miss |

### RocksDB (M4b Only)

| Setting | Value | Rationale |
|---------|-------|-----------|
| Storage | gp3 EBS, 20GB, 3000 IOPS | Policy state is compact (~1KB per experiment) |
| Write buffer | 64MB | Batch updates from reward events |
| Block cache | 512MB | Hot policies in memory |
| Compaction | Level compaction | Stable read performance |
| Snapshot frequency | Every policy update | Crash-only design: no separate shutdown path |

---

## 11. Observability

### Metrics Stack

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ Services     │────>│ Prometheus   │────>│ Grafana      │
│ /metrics     │     │ (scrape 15s) │     │ Dashboards   │
└──────────────┘     └──────┬───────┘     └──────────────┘
                            │
                     ┌──────▼───────┐
                     │ AlertManager │
                     │ PagerDuty    │
                     └──────────────┘
```

### Key Metrics per Service

| Service | Critical Metrics |
|---------|-----------------|
| M1 (Assignment) | `assignment_latency_p99`, `assignments_per_second`, `cache_hit_rate`, `grpc_error_rate` |
| M2 (Pipeline) | `events_ingested_per_second`, `dedup_rate`, `kafka_produce_latency_p99`, `validation_errors` |
| M3 (Metrics) | `spark_job_duration`, `metrics_computed`, `guardrail_breaches` |
| M4a (Analysis) | `analysis_job_duration`, `experiments_analyzed`, `srm_detections` |
| M4b (Policy) | `select_arm_latency_p99`, `rewards_processed_per_second`, `rocksdb_write_latency`, `mc_simulation_duration` |
| M5 (Management) | `api_latency_p99`, `state_transitions`, `auto_pauses` |
| M7 (Flags) | `flag_eval_latency_p99`, `evaluations_per_second` |

### Alerting Rules

```yaml
# monitoring/prometheus/alerts.yml
groups:
  - name: kaizen-critical
    rules:
      - alert: AssignmentLatencyHigh
        expr: histogram_quantile(0.99, rate(assignment_latency_seconds_bucket[5m])) > 0.005
        for: 2m
        labels: { severity: critical }
        annotations:
          summary: "M1 Assignment p99 latency > 5ms"

      - alert: PolicyServiceDown
        expr: up{job="policy-service"} == 0
        for: 30s
        labels: { severity: critical }
        annotations:
          summary: "M4b Bandit Policy service is down"

      - alert: KafkaConsumerLag
        expr: kafka_consumer_group_lag > 10000
        for: 5m
        labels: { severity: warning }
        annotations:
          summary: "Kafka consumer lag exceeding 10K messages"

      - alert: PostgresConnectionPoolExhausted
        expr: pg_stat_activity_count / pg_settings_max_connections > 0.8
        for: 2m
        labels: { severity: warning }
```

### Distributed Tracing

The monitoring stack includes Jaeger for distributed tracing:

```bash
# Start monitoring stack locally
just monitoring

# Access:
# Grafana:    http://localhost:3000 (admin/admin)
# Prometheus: http://localhost:9090
# Jaeger:     http://localhost:16686
```

All services emit OpenTelemetry traces via OTLP (gRPC port 4317, HTTP port 4318).

---

## 12. Scaling Strategy

### Service Scaling Characteristics

| Service | Scaling Dimension | Strategy | Min | Max |
|---------|-------------------|----------|-----|-----|
| M1 (Assignment) | Requests/sec | Horizontal (stateless) | 2 | 20 |
| M2 (Pipeline) | Events/sec | Horizontal (stateless) | 2 | 10 |
| M3 (Metrics) | Experiments count | Vertical (Spark workers) | 1 | 5 |
| M4a (Analysis) | Experiments count | Horizontal (job queue) | 1 | 5 |
| M4b (Policy) | Cannot scale horizontally | Vertical only (LMAX single-thread) | 1 | 1 |
| M5 (Management) | API requests | Horizontal (stateless) | 1 | 5 |
| M6 (UI) | Page views | Horizontal (stateless) | 1 | 3 |
| M7 (Flags) | Evaluations/sec | Horizontal (stateless) | 2 | 10 |

### M4b Scaling Limitation

M4b uses an LMAX-inspired single-threaded policy core. It **cannot** be horizontally scaled. Scaling strategies:

1. **Vertical scaling**: Increase CPU clock speed. Thompson MC (1000 draws x K arms) is CPU-bound
2. **Reduce MC_SIMULATIONS**: Trade statistical precision for latency (e.g., 100 draws for p99 < 5ms)
3. **Shard by experiment**: Run multiple M4b instances, each owning a subset of experiments (requires routing logic in M1)
4. **GPU offload**: Neural bandit (`--features gpu`) can use `tch-rs` for GPU-accelerated policy updates

### Traffic Estimates

| Scale | M1 RPS | M2 Events/s | M4b Rewards/s | M5 API/s | PG Connections |
|-------|--------|-------------|---------------|----------|----------------|
| Small (10 experiments) | 100 | 500 | 50 | 10 | 30 |
| Medium (100 experiments) | 5,000 | 10,000 | 500 | 50 | 80 |
| Large (1000 experiments) | 50,000 | 100,000 | 5,000 | 200 | 150 |

---

## 13. Security

### Network Security

| Layer | Control |
|-------|---------|
| External access | Only M1, M5 (API), M6 (UI), M7 exposed publicly |
| Internal services | M2, M3, M4a, M4b, M2-Orch on private network only |
| Database | Private subnet, security group restricted to service IPs |
| Kafka | SASL/SCRAM authentication, TLS encryption |
| Redis | AUTH token, TLS, private subnet |

### Authentication & Authorization

| Endpoint | Auth Method |
|----------|-------------|
| M1 (Assignment SDK calls) | API key in metadata header |
| M5 (Management API) | OAuth2 / JWT bearer token |
| M6 (UI) | Session cookie (M5 manages auth) |
| M7 (Feature Flags SDK) | API key in metadata header |
| Inter-service | mTLS or private network trust |

### Secrets Management

| Platform | Secrets Solution | Usage |
|----------|-----------------|-------|
| Fly.io | `fly secrets` | Encrypted, injected as env vars |
| AWS | Secrets Manager | Rotated, referenced in task definitions |
| GCP | Secret Manager | Versioned, mounted as volumes or env vars |

### Database Credentials Rotation

```bash
# AWS RDS automatic rotation via Secrets Manager
aws secretsmanager rotate-secret --secret-id kaizen/db-credentials \
  --rotation-lambda-arn arn:aws:lambda:...:function:rotate-postgres
```

---

## 14. CI/CD Pipeline

### Build Pipeline

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌──────────────┐
│ Push to PR   │───>│ Schema Lint  │───>│ Build & Test │───>│ Docker Build  │
│              │    │ (buf)       │    │ Rust/Go/TS   │    │ (9 images)    │
└─────────────┘    └─────────────┘    └──────┬───────┘    └──────┬────────┘
                                              │                    │
                                       ┌──────▼───────┐    ┌──────▼────────┐
                                       │ Hash Parity   │    │ Push to ECR/  │
                                       │ (10K vectors) │    │ GCR/Fly Reg   │
                                       └──────────────┘    └──────┬────────┘
                                                                   │
                                                            ┌──────▼────────┐
                                                            │ Deploy Staging │
                                                            │ (auto on main) │
                                                            └──────┬────────┘
                                                                   │
                                                            ┌──────▼────────┐
                                                            │ Deploy Prod    │
                                                            │ (manual gate)  │
                                                            │ Canary 10%     │
                                                            └───────────────┘
```

### Deployment Commands

```bash
# Fly.io deployment (per service)
fly deploy --app kaizen-assignment --image registry.fly.io/kaizen-assignment:SHA

# AWS ECS deployment
aws ecs update-service --cluster kaizen --service assignment \
  --task-definition kaizen-assignment:REVISION --force-new-deployment

# GCP Cloud Run deployment
gcloud run deploy kaizen-assignment \
  --image gcr.io/PROJECT/experimentation-assignment:SHA \
  --region us-central1
```

### Rollback

All platforms support instant rollback to the previous image:

```bash
# Fly.io
fly releases --app kaizen-assignment   # List releases
fly deploy --app kaizen-assignment --image registry.fly.io/kaizen-assignment:PREV_SHA

# AWS ECS
aws ecs update-service --cluster kaizen --service assignment \
  --task-definition kaizen-assignment:PREV_REVISION

# GCP Cloud Run
gcloud run services update-traffic kaizen-assignment --to-revisions PREV_REV=100
```

---

## 15. Disaster Recovery

### Recovery Time Objectives

| Component | RPO (data loss) | RTO (downtime) | Strategy |
|-----------|-----------------|----------------|----------|
| M1 (Assignment) | 0 (stateless) | < 10s | Restart, re-fetch config |
| M2 (Pipeline) | 0 (at-least-once) | < 10s | Restart, reconnect Kafka |
| M4b (Policy) | Last RocksDB snapshot | < 60s | Load snapshot + Kafka replay |
| PostgreSQL | < 1 min (WAL) | < 5 min (failover) | Multi-AZ, automated failover |
| Kafka | 0 (replication=3) | < 30s (leader election) | Multi-broker, ISR |
| Redis | < 5 min (RDB) | < 30s | Replica promotion |

### Backup Strategy

```bash
# PostgreSQL (automated by managed service)
# Manual snapshot
aws rds create-db-snapshot --db-instance-identifier kaizen-db --db-snapshot-identifier manual-$(date +%Y%m%d)

# RocksDB (M4b) — backup to S3
aws s3 sync /data/policy.db s3://kaizen-backups/policy/$(date +%Y%m%d)/

# Kafka — topic backup via MirrorMaker 2 (for DR region)
```

### Crash-Only Recovery

Per the platform's crash-only design (ADR-002), all services share startup and crash recovery code paths:

| Service | Recovery Action | Data Source |
|---------|----------------|-------------|
| M1 | Re-fetch experiment config | M5 Management API |
| M2 | Reconnect Kafka producer | Kafka cluster |
| M4a | Re-run analysis job | Delta Lake (idempotent) |
| M4b | Load RocksDB snapshot + replay Kafka | RocksDB + Kafka offsets |
| M5 | Reconnect PostgreSQL | PostgreSQL |

---

## 16. Cost Estimation

### Monthly Cost Comparison (Medium Scale: 100 experiments, 5K RPS)

| Component | Fly.io | AWS | GCP |
|-----------|--------|-----|-----|
| **Compute (9 services)** | $180 (shared VMs) | $450 (Fargate + 1 EC2) | $350 (Cloud Run + GKE node) |
| **PostgreSQL** | $50 (Fly Postgres) | $300 (RDS db.r6g.large Multi-AZ) | $250 (Cloud SQL) |
| **Kafka** | $100 (Upstash) | $500 (MSK 3x kafka.m5.large) | $400 (Confluent Cloud) |
| **Redis** | $30 (Upstash) | $150 (ElastiCache r6g.large) | $130 (Memorystore) |
| **Storage (S3/GCS)** | N/A (external) | $25 | $20 |
| **Load Balancer** | $0 (included) | $50 (ALB) | $30 (Cloud LB) |
| **NAT Gateway** | $0 | $100 | $50 |
| **Monitoring** | $0 (external) | $50 (CloudWatch) | $30 (Cloud Monitoring) |
| **Total** | **~$360/month** | **~$1,625/month** | **~$1,260/month** |
| **Hybrid** | | | **~$800/month** (M1/M7 on Fly + AWS backend) |

### Cost Scaling Notes

- **Fly.io** scales linearly with compute. No hidden costs (no NAT, no LB charges)
- **AWS** has high fixed costs (NAT Gateway: $0.045/GB, ALB: $0.0225/hr) but better managed services
- **GCP** Cloud Run is pay-per-request at low scale, cheaper than Fargate
- **Hybrid** avoids Fly's managed DB weakness while keeping edge compute cheap

---

## 17. Migration Playbook

### Phase 1: Local Development (Current State)

```bash
# Start everything locally
just dev              # Infra (Postgres, Kafka, Redis)
just monitoring       # Prometheus + Grafana + Jaeger
cargo run --package experimentation-assignment   # M1
# ... start other services
```

### Phase 2: Staging on Fly.io (1-2 days)

```bash
# 1. Build Docker images
just docker-build

# 2. Create Fly apps and deploy
# (see Section 5 for detailed commands)

# 3. Validate
curl -s https://kaizen-assignment.fly.dev/health
grpcurl kaizen-assignment.fly.dev:443 grpc.health.v1.Health/Check
```

### Phase 3: Production on AWS/GCP (1-2 weeks)

1. **Week 1**: Infrastructure provisioning (Terraform/Pulumi)
   - VPC, subnets, security groups
   - RDS PostgreSQL, MSK Kafka, ElastiCache Redis
   - ECR/GCR registry, push Docker images
   - ECS task definitions / Cloud Run services

2. **Week 2**: Service deployment and validation
   - Deploy services with blue-green strategy
   - Run integration tests against staging
   - Load test with `just loadtest-assignment` (target: p99 < 5ms at 10K RPS)
   - Canary production deployment (10% -> 50% -> 100%)

### Phase 4: Hybrid (Optional, 1 week)

1. Deploy M1 and M7 to Fly.io edge regions
2. Configure WireGuard tunnel to AWS VPC
3. Update M1 config to point to AWS-hosted M4b and M5
4. Validate cross-cloud latency (target: < 50ms Fly -> AWS)

---

## 18. Decision Matrix

### Choosing Your Deployment Topology

| Factor | Weight | Fly.io | AWS | GCP | Hybrid |
|--------|--------|--------|-----|-----|--------|
| **Setup speed** | High | 5 | 2 | 3 | 3 |
| **Operational simplicity** | High | 5 | 2 | 3 | 3 |
| **Edge latency (M1 SLA)** | Critical | 5 | 3 | 4 | 5 |
| **Managed Postgres reliability** | Critical | 2 | 5 | 5 | 5 |
| **Kafka reliability** | High | 3 | 5 | 4 | 5 |
| **Autoscaling** | Medium | 2 | 5 | 5 | 4 |
| **RocksDB persistence (M4b)** | High | 3 | 5 | 5 | 5 |
| **Observability** | Medium | 2 | 5 | 4 | 4 |
| **Cost (medium scale)** | Medium | 5 | 2 | 3 | 4 |
| **Compliance (HIPAA/SOC2)** | Varies | 3 | 5 | 5 | 4 |
| **Multi-region** | Low-Med | 5 | 3 | 4 | 5 |
| **Weighted Score** | | **3.6** | **3.7** | **3.9** | **4.3** |

### Recommendation Summary

| Use Case | Recommended Topology |
|----------|---------------------|
| **Proof of concept / demo** | Fly.io (all services) |
| **Early production (< 1K RPS)** | Fly.io + external managed Postgres (Neon/Supabase) |
| **Production (1K-10K RPS)** | Hybrid (Fly.io edge + AWS/GCP backend) |
| **Enterprise (> 10K RPS, compliance)** | AWS or GCP (full managed) |
| **Global SVOD platform** | Hybrid (Fly.io edge in 5+ regions + AWS backend) |

---

## Appendix A: Environment Variables

| Variable | Service(s) | Description | Example |
|----------|-----------|-------------|---------|
| `DATABASE_URL` | M5 | PostgreSQL connection string | `postgres://user:pass@host:5432/experimentation` |
| `KAFKA_BROKERS` | M2, M3, M4b, M5 | Kafka bootstrap servers | `broker1:9092,broker2:9092` |
| `REDIS_URL` | M3, M7 | Redis connection string | `redis://host:6379` |
| `MANAGEMENT_ADDR` | M1 | M5 gRPC address (config source) | `management.internal:50055` |
| `POLICY_ADDR` | M1 | M4b gRPC address (bandit delegation) | `policy.internal:50054` |
| `POLICY_ROCKSDB_PATH` | M4b | Path to RocksDB data directory | `/data/policy.db` |
| `RUST_LOG` | All Rust | Log level filter | `info` or `debug` |
| `PORT` | All | Override default service port | `50051` |

## Appendix B: Health Check Endpoints

All services expose gRPC health checks:

```bash
# gRPC health check (requires grpcurl)
grpcurl -plaintext localhost:50051 grpc.health.v1.Health/Check

# HTTP health check (M6 UI)
curl http://localhost:3000/api/health
```

## Appendix C: Load Testing

```bash
# Assignment service: p99 < 5ms at 10K RPS
just loadtest-assignment

# Assignment service: p99 < 5ms at 50K RPS (Phase 4 target)
just loadtest-assignment-50k

# Policy service: p99 < 15ms at 10K RPS
just loadtest-policy

# Feature flag service: p99 < 10ms at 20K RPS
just loadtest-flags

# Full platform load test (k6)
just loadtest

# Spike test
just loadtest-spike

# Soak test (30 minutes)
just loadtest-soak
```
