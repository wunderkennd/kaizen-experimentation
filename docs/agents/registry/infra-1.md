---
type: Kaizen Infra Agent
title: "Infra-1: Networking & Foundation"
description: Owns the networking foundation on AWS and GCP — VPC, subnets, security groups, service discovery, IAM.
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/infra/pkg/aws
tags: [infra-agent, go, pulumi, networking]
timestamp: 2026-07-04T12:00:00Z
id: infra-1
label: infra-1
executors: [claude-workflow, claude-web, multiclaude]
language: Go (Pulumi)
owned_paths:
  - infra/pkg/aws/network.go
  - infra/pkg/gcp/network.go
  - infra/test/network_test.go
depends_on: []
---

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
