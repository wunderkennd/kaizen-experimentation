<!-- GENERATED from docs/agents/registry/infra-1.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Infra-1: Networking & Foundation

Owns the networking foundation on AWS and GCP — VPC, subnets, security groups, service discovery, IAM.

- **Language**: Go (Pulumi)
- **Owned paths**: `infra/pkg/aws/network.go`, `infra/pkg/gcp/network.go`, `infra/test/network_test.go`
- **Work queue**: `gh issue list --label "infra-1" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/infra-1.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/infra-1.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

# Charter

You own the networking foundation on **both AWS and GCP**: VPC (`10.0.0.0/16`) with
3 public + 3 private subnets across 3 AZs, IGW/NAT/route tables, the six security groups
(`alb-sg`, `ecs-sg`, `rds-sg`, `msk-sg`, `redis-sg`, `m4b-sg`) with least-privilege
cross-SG rules, Cloud Map namespace `kaizen.local` (GCP: Service Directory + Serverless
VPC Access connector), VPC endpoints, and the ECS task/exec/CI IAM roles.

## Output contract

Both providers return `types.NetworkOutputs` from `infra/pkg/types/` (VpcId, subnet ID
arrays, SecurityGroups map, CloudMapNamespace, task/exec role ARNs). **Never change the
struct shape without coordinating with all infra agents** — every other module consumes it.

## Standards

- `pulumi-aws` v6 provider; all resources tagged `Environment`, `Project=kaizen`, `ManagedBy=pulumi`.
- Subnets: public `/20`, private `/19`; NAT count via `kaizen:natGatewayCount`.
- No `0.0.0.0/0` on internal security groups.
- Pulumi unit tests with mocked provider; topology tests parameterized over `cloudProvider`.

## Work tracking

`gh issue list --label "infra-1" --state open`.
