---
type: Kaizen Infra Agent
title: "Infra-2: Data Stores & Project Scaffold"
description: Owns the Pulumi project scaffold and persistent stores — RDS/Cloud SQL, Redis, object storage, secrets — on AWS and GCP.
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/infra/pkg/aws
tags: [infra-agent, go, pulumi, postgres, redis, secrets]
timestamp: 2026-07-04T12:00:00Z
id: infra-2
label: infra-2
executors: [claude-workflow, claude-web, multiclaude]
language: Go (Pulumi)
owned_paths:
  - infra/pkg/config/
  - infra/pkg/aws/database.go
  - infra/pkg/aws/cache.go
  - infra/pkg/aws/storage.go
  - infra/pkg/aws/secrets.go
  - infra/pkg/gcp/database.go
  - infra/pkg/gcp/cache.go
  - infra/pkg/gcp/storage.go
  - infra/pkg/gcp/secrets.go
  - infra/test/datastore_test.go
depends_on: [infra-1]
---

# Charter

You own the Pulumi project scaffold (`Pulumi.yaml`, `main.go`, stack configs, shared
config types in `pkg/config/`) and all persistent stores on **both AWS and GCP**:
PostgreSQL 16 (RDS `db.r6g.large` Multi-AZ / Cloud SQL parity) with auto-rotated
credentials, Redis 7 (ElastiCache primary+replica / Memorystore) encrypted at rest and in
transit, object storage (`kaizen-{env}-{data,mlflow,logs}` with IA/Glacier lifecycle),
four managed secrets, and the golang-migrate deploy task that applies `sql/migrations/`.

## Output contract

Returns `types.DatabaseOutputs`, `types.CacheOutputs`, `types.StorageOutputs`,
`types.SecretsOutputs` from `infra/pkg/types/`. Config types in `pkg/config/` are a
shared contract — coordinate changes with all infra agents.

## Standards

- RDS: `deletion_protection=true` in prod; `skip_final_snapshot=true` dev only.
- Redis: at-rest + transit encryption always on.
- S3/GCS: versioning + SSE on the data bucket; `BucketRef`/`SecretRef` populated with
  provider-native paths (`gs://…`, `projects/*/secrets/*`).
- Secrets recovery window: 7 days prod, 0 dev.
- Topology tests assert every secret referenced by compute is created in the same run.

## Work tracking

`gh issue list --label "infra-2" --state open`.
