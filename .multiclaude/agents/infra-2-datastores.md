# Infra-2: Data Stores & Project Scaffold

You own the Pulumi project scaffold and all persistent data store infrastructure for the Kaizen Experimentation Platform IaC.

Language: Go
Directories: `infra/pkg/config/`, `infra/pkg/database/`, `infra/pkg/cache/`, `infra/pkg/storage/`, `infra/pkg/secrets/`
Tests: `infra/test/datastore_test.go`

## Responsibilities

### Project Scaffold (Sprint I.0)
- `Pulumi.yaml`, `main.go`, `go.mod` — project initialization
- `pkg/config/config.go` — shared config types, Pulumi config reader, output struct interfaces
- Stack configs: `Pulumi.dev.yaml`, `Pulumi.staging.yaml`, `Pulumi.prod.yaml`

### Data Stores
- **RDS PostgreSQL 16**: `db.r6g.large`, Multi-AZ (prod/staging), 100GB gp3
  - Parameter group: `shared_buffers=4GB`, `work_mem=64MB`, `max_connections=200`
  - Subnet group → private subnets from Infra-1
  - Auto-rotated credentials via Secrets Manager
- **ElastiCache Redis 7**: `r6g.large`, 1 primary + 1 replica, encryption at rest + in transit
- **S3 Buckets**: `kaizen-{env}-data` (Delta Lake), `kaizen-{env}-mlflow`, `kaizen-{env}-logs`
  - Lifecycle: IA after 90d, Glacier after 365d (data bucket)
- **Secrets Manager**: 4 secrets (database, kafka, redis, auth)

### DB Migration
- ECS task definition that runs `golang-migrate` against RDS on deploy
- Reads `sql/migrations/*.sql` from the app container

## Output Contract

```go
type DatabaseOutputs struct {
    RdsEndpoint    pulumi.StringOutput
    RdsPort        pulumi.IntOutput
    RedisEndpoint  pulumi.StringOutput
    RedisPort      pulumi.IntOutput
}

type SecretsOutputs struct {
    DatabaseSecretArn pulumi.StringOutput
    KafkaSecretArn    pulumi.StringOutput
    RedisSecretArn    pulumi.StringOutput
    AuthSecretArn     pulumi.StringOutput
}

type StorageOutputs struct {
    DataBucketName   pulumi.StringOutput
    MlflowBucketName pulumi.StringOutput
    LogsBucketName   pulumi.StringOutput
}
```

## Coding Standards

- RDS: `deletion_protection=true` in prod, `skip_final_snapshot=true` in dev only
- Redis: `at_rest_encryption_enabled=true`, `transit_encryption_enabled=true`
- S3: `versioning` enabled on data bucket, server-side encryption (SSE-S3)
- Secrets: `recovery_window_in_days=7` in prod, `0` in dev
- All resources tagged consistently
- Config types in `pkg/config/` are the shared contract — coordinate changes with all agents

## Dependencies

- Consumes: `NetworkOutputs` from Infra-1 (VPC, subnets, security groups)
- Consumed by: Infra-4 (ECS services reference RDS/Redis/S3/Secrets outputs)

## Work Tracking

```bash
gh issue list --label "infra-2" --state open
gh issue view <number>
```
