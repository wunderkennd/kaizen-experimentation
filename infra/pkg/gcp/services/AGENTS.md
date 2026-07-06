<!-- GENERATED from docs/agents/registry/infra-4.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Infra-4: Compute & Services

Owns the compute layer and all 9 Kaizen service definitions — ECS/Fargate + the M4b EC2 special case on AWS, Cloud Run + GCE on GCP.

- **Language**: Go (Pulumi)
- **Owned paths**: `infra/pkg/aws/compute.go`, `infra/pkg/aws/cicd.go`, `infra/pkg/gcp/compute.go`, `infra/pkg/gcp/services/`, `infra/test/compute_test.go`
- **Depends on**: infra-1, infra-2, infra-3
- **Work queue**: `gh issue list --label "infra-4" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/infra-4.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/infra-4.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

# Charter

You own compute on **both AWS and GCP**: the ECS cluster (Fargate for 8 services, EC2 for
M4b), the 9 ECR repos (`kaizen-{assignment,pipeline,orchestration,metrics,analysis,policy,management,ui,flags}`,
keep-last-10 lifecycle), per-service task definitions with Cloud Map names and
secret-injected env vars, target-tracking autoscaling (request-count for M1/M7, CPU 70%
elsewhere), and startup ordering (M5 first — owns the schema; then M1/M2/M4b; then the
rest). **M4b is special**: stateful singleton on EC2 `c6i.xlarge` (GCP: GCE
`n2-standard-4`, MIG size 1, autohealing) with a 20GB gp3 EBS at `/data/rocksdb`, daily
snapshots, auto-recovery, recovery target < 10s. On GCP, `NewCloudRunService(...)` is the
registry-pattern service template (runbook: `docs/runbooks/gcp-compute-services.md`);
M1/M7 pin `min-instances: 1` to hold p99 < 5ms against cold starts.

## Output contract

Both providers return `types.ComputeOutputs` (`ClusterId`, `ServiceEndpoints`,
`M4bInstanceId`, `M4bEndpoint`).

## Standards

- One shared task-def/service builder — no per-service copy-paste.
- M4b user-data script is idempotent.
- Health checks: gRPC health protocol (Rust services), `/healthz` (Go services).
- Topology tests assert every Cloud Run service carries its Workload Identity binding.

## Work tracking

`gh issue list --label "infra-4" --state open`.
