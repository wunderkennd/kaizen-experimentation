# Infra-4: Compute & Services

You own the ECS compute layer and all 9 service definitions for the Kaizen Experimentation Platform IaC.

Language: Go
Directories: `infra/pkg/compute/`, `infra/pkg/cicd/`
Tests: `infra/test/compute_test.go`

## Responsibilities

### ECS Cluster
- ECS cluster with Fargate + EC2 capacity providers
- Fargate for 8 services, EC2 for M4b (RocksDB + LMAX single-thread)

### ECR Repositories
- 9 repos: `kaizen-{assignment,pipeline,orchestration,metrics,analysis,policy,management,ui,flags}`
- Lifecycle: keep last 10 images

### Service Definitions (9 total)

| Service | Launch | vCPU | RAM | Port(s) | Cloud Map Name | Notes |
|---------|--------|------|-----|---------|----------------|-------|
| M1 Assignment | Fargate | 0.5 | 1GB | 50051 | `m1-assignment` | Min 2, max 20 |
| M2 Pipeline | Fargate | 0.5 | 1GB | 50052 | `m2-pipeline` | Min 2, max 10 |
| M2-Orch | Fargate | 0.25 | 512MB | 50058 | `m2-orchestration` | Min 1, max 3 |
| M3 Metrics | Fargate | 1.0 | 2GB | 50056, 50059 | `m3-metrics` | Min 1, max 5 |
| M4a Analysis | Fargate | 1.0 | 2GB | 50053 | `m4a-analysis` | Min 1, max 5 |
| M4b Policy | **EC2** | **4** | **8GB** | 50054 | `m4b-policy` | **c6i.xlarge, 20GB gp3 EBS, singleton** |
| M5 Management | Fargate | 0.5 | 1GB | 50055, 50060 | `m5-management` | Min 1, max 5 |
| M6 UI | Fargate | 0.5 | 1GB | 3000 | `m6-ui` | Min 1, max 3 |
| M7 Flags | Fargate | 0.25 | 512MB | 50057 | `m7-flags` | Min 2, max 10 |

### M4b Special Case
- EC2 launch type with `c6i.xlarge` instance
- 20GB gp3 EBS volume mounted at `/data/rocksdb`
- AWS Backup: daily EBS snapshots
- Auto-recovery: EC2 status check alarm → replace instance
- User data script mounts EBS, starts ECS agent

### Autoscaling
- Target-tracking policies per service
- M1, M7: `ALBRequestCountPerTarget` (1000 rps/task)
- All others: CPU utilization 70%

### Environment Variables
Each task definition gets env vars referencing Cloud Map DNS names + secrets:
```
DATABASE_URL       → from Secrets Manager (Infra-2)
KAFKA_BROKERS      → MSK bootstrap brokers (Infra-3)
REDIS_URL          → ElastiCache endpoint (Infra-2)
M5_ADDR            → http://m5-management.kaizen.local:50055
M4B_ADDR           → http://m4b-policy.kaizen.local:50054
ANALYSIS_SERVICE_URL → http://m4a-analysis.kaizen.local:50053
OTEL_EXPORTER_OTLP_ENDPOINT → from Infra-5
```

### Service Startup Order
M5 must start first (owns PostgreSQL schema). Then M1, M2, M4b. Then M3, M4a, M6, M7.
Implement via ECS service `dependsOn` or health-check gates.

## Output Contract

```go
type ComputeOutputs struct {
    ClusterId   pulumi.IDOutput
    ServiceArns map[string]pulumi.StringOutput
}
```

## Coding Standards

- One helper function per service to reduce duplication (shared Fargate task def builder)
- M4b EC2 user data script must be idempotent (re-run safe)
- Health checks: gRPC health protocol for Rust services, `/healthz` for Go services
- Log driver: `awslogs` to CloudWatch (log group names from Infra-5)
- All resources tagged consistently

## Dependencies

- Consumes: `NetworkOutputs` (Infra-1), `DatabaseOutputs` + `SecretsOutputs` (Infra-2), `StreamingOutputs` (Infra-3)
- Consumed by: Infra-5 (ALB target groups reference ECS services)

## Work Tracking

```bash
gh issue list --label "infra-4" --state open
gh issue view <number>
```
