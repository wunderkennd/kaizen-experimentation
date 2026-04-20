# Multi-Cloud Design: AWS + GCP Deployment

**Date:** 2026-04-20
**Status:** Approved
**Author:** Kenneth Sylvain + Claude Code

## Context

A pipeline customer contract requires Kaizen to deploy on GCP. The platform currently runs exclusively on AWS (ECS Fargate, RDS, ElastiCache, MSK, S3, ALB, Route53, CloudWatch). Application code (Rust crates, Go services) is already cloud-agnostic — all coupling is in the `infra/` Pulumi layer.

## Requirements

- **Per-tenant cloud selection** — each customer deploys to their preferred cloud (AWS or GCP). No cross-cloud traffic within a tenant.
- **Full feature parity** — GCP customers get the identical feature set as AWS customers from day one.
- **No application code changes** — services connect via standard protocols (gRPC, Kafka, PostgreSQL, Redis). Infrastructure handles the cloud abstraction.

## Architecture: Provider Modules + Shared Orchestrator

### Directory Structure

```
infra/
  main.go                    # Deploy() dispatches by cloudProvider config
  pkg/
    types/                   # Shared output structs (both providers return these)
      outputs.go             # NetworkOutputs, DatabaseOutputs, ComputeOutputs, etc.
      config.go              # TenantConfig, provider-agnostic fields
    aws/                     # Refactored from current pkg/* modules
      network.go             # VPC, SGs, Cloud Map, VPC Endpoints
      database.go            # RDS PostgreSQL
      cache.go               # ElastiCache Redis
      storage.go             # S3 buckets
      compute.go             # ECS Fargate (8 services) + EC2 (M4b)
      edge.go                # ALB, Route53, ACM, WAF
      observability.go       # CloudWatch, AMP/AMG
      secrets.go             # Secrets Manager
      cicd.go                # ECR repositories
      streaming.go           # MSK (existing tenants only)
    gcp/                     # New, parallel structure
      network.go             # VPC, Firewall rules, Service Directory, VPC Connector
      database.go            # Cloud SQL PostgreSQL
      cache.go               # Memorystore Redis
      storage.go             # Cloud Storage buckets
      compute.go             # Cloud Run (8 services) + GCE (M4b)
      edge.go                # Cloud Load Balancing, Cloud DNS, managed certs, Cloud Armor
      observability.go       # Cloud Logging/Monitoring, Managed Prometheus + Grafana
      secrets.go             # Secret Manager
      cicd.go                # Artifact Registry
    streaming/               # Cloud-agnostic (shared)
      redpanda.go            # Redpanda Cloud provisioning
      topics.go              # Kafka-protocol topic creation
    config/                  # Shared config loading
      config.go              # LoadConfig, DefaultTags, environment helpers
```

### Deploy() Orchestration

`Deploy()` uses a simple `switch cfg.CloudProvider` at each stage. Both provider paths return the same `types.*Outputs` structs, so stages compose without knowing which cloud they're on.

```go
func Deploy(ctx *pulumi.Context) error {
    cfg := config.LoadConfig(ctx)

    var (
        netOut     types.NetworkOutputs
        storageOut types.StorageOutputs
        dbOut      types.DatabaseOutputs
        // ...
        err        error
    )

    // Stage 1: Network
    switch cfg.CloudProvider {
    case "aws":
        netOut, err = aws.NewNetwork(ctx, cfg)
    case "gcp":
        netOut, err = gcp.NewNetwork(ctx, cfg)
    }
    if err != nil { return err }

    // Stage 4: Streaming (cloud-agnostic for new tenants)
    switch cfg.StreamingProvider {
    case "msk":
        streamOut, err = aws.NewMSK(ctx, cfg, netOut)
    case "redpanda":
        streamOut, err = streaming.NewRedpanda(ctx, cfg, netOut)
    }

    // ... same pattern for all 6 stages
    // Exports are generic endpoint strings
    ctx.Export("databaseEndpoint", dbOut.Endpoint)
    return nil
}
```

Key design decisions:
- **Switch, not interface** — no abstraction ceremony. Shared return types enforce the contract at compile time.
- **Streaming dispatches on `streamingProvider`, not `cloudProvider`** — so an AWS tenant can opt into Redpanda later without changing their cloud provider.
- **Stack exports are generic** — endpoint strings, not ARNs. CI/monitoring scripts don't need to know the cloud.

### Shared Output Types

```go
package types

type NetworkOutputs struct {
    VpcId              pulumi.IDOutput
    PublicSubnetIds    pulumi.StringArrayOutput
    PrivateSubnetIds   pulumi.StringArrayOutput
    SecurityGroupIds   map[string]pulumi.IDOutput // keyed: "ecs", "rds", "redis", etc.
    ServiceDiscoveryId pulumi.IDOutput
}

type DatabaseOutputs struct {
    Endpoint   pulumi.StringOutput // host:port
    InstanceId pulumi.StringOutput // cloud-native ID for alarms
}

type CacheOutputs struct {
    Endpoint pulumi.StringOutput // redis://host:port
}

type StorageOutputs struct {
    DataBucketName   pulumi.StringOutput
    DataBucketRef    pulumi.StringOutput // ARN (aws) or gs:// URI (gcp)
    MlflowBucketName pulumi.StringOutput
    MlflowBucketRef  pulumi.StringOutput
    LogsBucketName   pulumi.StringOutput
    LogsBucketRef    pulumi.StringOutput
}

type IAMOutputs struct {
    ExecRoleRef pulumi.StringOutput // ARN (aws) or service account email (gcp)
    TaskRoleRef pulumi.StringOutput
}

type StreamingOutputs struct {
    BootstrapBrokers  pulumi.StringOutput
    SchemaRegistryUrl pulumi.StringOutput
    ClusterArn        pulumi.StringOutput
}

type ComputeOutputs struct {
    ClusterId        pulumi.StringOutput
    ServiceEndpoints map[string]pulumi.StringOutput // service → internal URL
    M4bInstanceId    pulumi.StringOutput
    M4bEndpoint      pulumi.StringOutput
}

type SecretsOutputs struct {
    DatabaseSecretRef pulumi.StringOutput // ARN or projects/*/secrets/* path
    KafkaSecretRef    pulumi.StringOutput
    RedisSecretRef    pulumi.StringOutput
    AuthSecretRef     pulumi.StringOutput
}

type EdgeOutputs struct {
    LoadBalancerDns pulumi.StringOutput
    CertificateRef  pulumi.StringOutput
    HostedZoneId    pulumi.StringOutput
}
```

## Compute Model

### Stateless Services (8 of 9)

| Service | AWS | GCP | Notes |
|---------|-----|-----|-------|
| M1 Assignment | ECS Fargate | Cloud Run (min-instances=1) | p99 < 5ms — must avoid cold starts |
| M2 Pipeline | ECS Fargate | Cloud Run | High throughput Kafka producer |
| M2-Orch | ECS Fargate | Cloud Run | Stateless orchestrator |
| M3 Metrics | ECS Fargate | Cloud Run | Batch Spark job submission |
| M4a Analysis | ECS Fargate | Cloud Run | CPU-intensive batch |
| M5 Management | ECS Fargate | Cloud Run | CRUD, PostgreSQL access |
| M6 UI | ECS Fargate | Cloud Run | Next.js SSR |
| M7 Flags | ECS Fargate | Cloud Run (min-instances=1) | p99 < 5ms — must avoid cold starts |

Cloud Run constraints handled:
- **Cold starts** — M1 and M7 set `min-instances: 1` (analogous to ECS `desiredCount` floor)
- **VPC access** — Serverless VPC Access connector for Memorystore, Cloud SQL, Redpanda
- **Service discovery** — `ComputeOutputs.ServiceEndpoints` map abstracts Cloud Map (AWS) vs Service Directory/Cloud Run URLs (GCP)

### M4b Policy Service (Stateful — Dedicated Instance)

| Concern | AWS | GCP |
|---------|-----|-----|
| Instance | c5.xlarge (4 vCPU, 8 GB) | n2-standard-4 (4 vCPU, 16 GB) |
| Storage | 50GB gp3 EBS volume | 50GB pd-ssd Persistent Disk |
| Orchestration | ASG min=max=desired=1 | MIG size=1 + autohealing |
| Recovery | ASG replaces → EBS reattaches (~3-5s) | MIG recreates → PD reattaches (~2-4s) |
| Discovery | Cloud Map DNS record | Service Directory endpoint |

M4b invariants (both clouds): single instance, RocksDB snapshot on persistent volume, LMAX single-threaded core, crash recovery < 10s, Kafka consumer for reward events. Same container image on both clouds.

### Container Image Strategy

1. **Build once** — CI builds multi-arch images (linux/amd64, linux/arm64) from identical Dockerfiles
2. **Push to both** — CI pushes to ECR (AWS) and Artifact Registry (GCP) in parallel, same tag/digest
3. **Config at runtime** — services read endpoints from env vars injected by container platform
4. **Registry per tenant** — Pulumi stack references the tenant's cloud registry

## Streaming

### Strategy: MSK stays on AWS, Redpanda for GCP (consolidate later)

- **Existing AWS tenants** — remain on MSK. `pkg/aws/streaming.go` retains full MSK support.
- **New GCP tenants** — deploy Redpanda via `pkg/streaming/redpanda.go`. Kafka wire-compatible, no application code changes.
- **New AWS tenants** — can opt into Redpanda via `streamingProvider: redpanda` config. Optional.
- **Future consolidation** — once Redpanda proves out in production, migrate existing MSK tenants. MirrorMaker2 or Redpanda's built-in migration handles the cutover.

### Redpanda Deployment Model

- **Primary:** Redpanda Cloud (managed) — available on both AWS and GCP, zero operational overhead
- **Fallback:** Self-hosted Redpanda on VMs if customer requires data residency in their own cloud account
- **Pulumi provider:** Use the [Redpanda Terraform provider](https://github.com/redpanda-data/terraform-provider-redpanda) via Pulumi's Terraform bridge (`pulumi-redpanda`). Covers cluster provisioning, user/ACL management, and topic creation.

### What Changes from MSK

| Concern | MSK | Redpanda |
|---------|-----|----------|
| Protocol | Kafka | Kafka (wire-compatible) |
| Auth | SASL/SCRAM via Secrets Manager | SASL/SCRAM via Redpanda Cloud |
| Encryption | KMS key per cluster | TLS built-in |
| Topics | Pulumi Kafka provider | Same (works against Redpanda) |
| Schema Registry | Confluent CP container on ECS | Redpanda built-in registry |
| App code changes | — | None |

## Testing Strategy

### Five Test Layers

| Layer | Validates | Runs | Credentials |
|-------|-----------|------|-------------|
| **Unit tests** | Individual module resource creation, properties | Every PR | None |
| **Topology tests** | Full `Deploy()` with mocks, resource counts, exports, secret existence, IAM bindings | Every PR | None |
| **Preview tests** | `pulumi preview` against real cloud APIs | Nightly | AWS + GCP |
| **Streaming integration** | Redpanda wire compatibility with Rust/Go Kafka clients | Every PR | None (Docker) |
| **Smoke load test** | p99 latency SLAs, M4b crash recovery, M2 throughput | Weekly / pre-release | GCP project |

### Gap Mitigations

1. **Cold start + VPC connector latency** — smoke load test deploys to a real GCP project, runs 60-second p99 check against M1/M7 endpoints. Validates the < 5ms SLA on real infrastructure.

2. **IAM/service account binding drift** — topology test asserts that for each Cloud Run service, a Workload Identity binding exists to the correct service account with required IAM roles.

3. **Secret injection failure modes** — topology test verifies every secret referenced in compute module inputs was created by the secrets module in the same test run. Both providers get this check.

4. **Redpanda schema registry compatibility** — streaming integration test replaces `confluentinc/cp-schema-registry` with Redpanda in Docker Compose, runs existing Rust/Go Kafka producer/consumer tests. Validates wire compatibility before any cloud deployment.

### Parameterized Topology Tests

```go
func TestFullStackDeploy(t *testing.T) {
    providers := []string{"aws", "gcp"}
    for _, p := range providers {
        t.Run(p, func(t *testing.T) {
            mocks := providerMocks(p) // returns aws or gcp mocks
            cfg := fullstackConfig(p) // sets cloudProvider
            err := pulumi.RunErr(Deploy,
                pulumi.WithMocks("kaizen", "dev", mocks),
                cfg,
            )
            if err != nil {
                t.Fatalf("Deploy(%s) failed: %v", p, err)
            }
        })
    }
}
```

## AWS → GCP Service Mapping

| AWS Service | GCP Service | Module |
|-------------|-------------|--------|
| VPC + Security Groups | VPC + Firewall Rules | network |
| ECS Fargate | Cloud Run | compute |
| EC2 + EBS (M4b) | GCE + Persistent Disk | compute |
| RDS PostgreSQL | Cloud SQL PostgreSQL | database |
| ElastiCache Redis | Memorystore Redis | cache |
| S3 | Cloud Storage | storage |
| ALB | Cloud Load Balancing | edge |
| Route 53 | Cloud DNS | edge |
| ACM | Google-managed certificates | edge |
| WAF v2 | Cloud Armor | edge |
| CloudWatch | Cloud Logging + Cloud Monitoring | observability |
| AMP / AMG | Managed Prometheus + Grafana Cloud | observability |
| Secrets Manager | Secret Manager | secrets |
| ECR | Artifact Registry | cicd |
| KMS | Cloud KMS | secrets/streaming |
| Cloud Map | Service Directory | network |
| IAM Roles | IAM Service Accounts + Workload Identity | compute/network |

## Phased Delivery

### Phase 0: Foundation (Refactor)

Restructure `infra/` into provider modules. No new cloud resources.

- Move current modules into `pkg/aws/`
- Create `pkg/types/` with shared output structs
- Refactor `Deploy()` to use switch + shared types
- Migrate existing tests to new paths

**Gate:** Zero diff on `pulumi preview` against existing AWS stacks. All existing tests pass.

### Phase 1: GCP Core Modules

Build GCP provider modules following the same stage order as AWS.

- `gcp/network.go` — VPC, firewall, Service Directory, VPC connector
- `gcp/storage.go` — Cloud Storage buckets
- `gcp/database.go` — Cloud SQL PostgreSQL
- `gcp/cache.go` — Memorystore Redis
- `gcp/secrets.go` — Secret Manager
- `gcp/compute.go` — Cloud Run (8 services) + GCE (M4b)
- `gcp/cicd.go` — Artifact Registry

**Parallelizable:** network + storage + cicd can be built concurrently. database + cache + secrets can be built concurrently.

**Gate:** `pulumi preview --stack gcp-dev` succeeds. GCP topology test passes.

### Phase 2: Streaming (Parallel with Phase 1)

Redpanda integration + schema registry migration.

- `pkg/streaming/redpanda.go` — Redpanda Cloud provisioning
- Replace Confluent Schema Registry in Docker Compose with Redpanda
- Run all Rust/Go Kafka client tests against Redpanda
- Validate schema registry protocol compatibility

**Gate:** All existing `cargo test` and `go test` pass against Redpanda in Docker Compose. No application code changes.

### Phase 3: Edge + Observability

GCP edge layer + monitoring.

- `gcp/edge.go` — Cloud Load Balancing, Cloud DNS, managed certs, Cloud Armor
- `gcp/observability.go` — Cloud Logging/Monitoring, Managed Prometheus + Grafana
- CI pipeline: push images to Artifact Registry alongside ECR
- Tenant provisioning: `cloudProvider` config in Pulumi stack

**Gate:** Full GCP stack deploys end-to-end. All 9 services healthy.

### Phase 4: Validation

SLA validation, load testing, hardening.

- Smoke load test: M1/M7 p99 < 5ms on Cloud Run
- M4b chaos test: kill GCE instance, verify recovery < 10s
- M2 throughput test: 100K events/sec through Redpanda on GCP
- Security review: IAM bindings, network isolation, secret access
- Parity audit: compare AWS/GCP stack exports, verify feature equivalence

**Gate:** All SLAs met. Security review passed. Parity audit clean.

### Critical Path

```
Phase 0 (sequential) → Phase 1 (GCP compute) → Phase 3 (edge) → Phase 4 (validation)
                        Phase 2 (Redpanda) runs in parallel with Phase 1, merges before Phase 4
```

## Decisions Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Deployment model | Per-tenant cloud selection | Customer requirement — each tenant lives on one cloud |
| Container orchestration | Cloud-native per cloud (ECS / Cloud Run) | Lower ops overhead than K8s for 1-3 person team |
| M4b | Dedicated instance (EC2 / GCE) | Stateful workload, persistent volume required |
| Streaming | Redpanda for GCP, MSK stays on AWS | Kafka wire-compatible, eliminates MSK-on-GCP problem |
| IaC | Pulumi (stay), parallel provider modules | Preserve investment, add GCP alongside AWS |
| Abstraction level | Shared types, no interfaces yet | Avoid premature abstraction; extract interfaces after both providers exist |
| MSK migration | Decide later | Ship GCP with Redpanda first, consolidate after operational data |

## Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Cloud Run cold start breaks p99 SLA | Medium | High | min-instances=1, smoke load test in Phase 4 |
| VPC connector latency pushes M1/M7 over budget | Medium | High | Direct VPC egress (beta), or move to GKE for those 2 services |
| Redpanda schema registry incompatibility | Low | Medium | Streaming integration test catches before deploy |
| Phase 0 refactor introduces regression | Low | High | Zero-diff gate on pulumi preview, existing test suite |
| GCP IAM model surprises (Workload Identity) | Medium | Medium | IAM contract tests, security review in Phase 4 |
| Parity drift over time | High | Medium | CI parity check — both providers compile against same types |
