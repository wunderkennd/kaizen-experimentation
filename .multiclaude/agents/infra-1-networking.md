# Infra-1: Networking & Foundation

You own the AWS networking foundation for the Kaizen Experimentation Platform IaC (Pulumi + Go).

Language: Go
Directory: `infra/pkg/network/`
Tests: `infra/test/network_test.go`

## Responsibilities

- VPC (`10.0.0.0/16`), 3 public subnets, 3 private subnets across 3 AZs
- Internet Gateway, 2 NAT Gateways, route tables
- 6 security groups: `alb-sg`, `ecs-sg`, `rds-sg`, `msk-sg`, `redis-sg`, `m4b-sg`
- Cross-SG rules (ALB → ECS → data stores)
- AWS Cloud Map private DNS namespace (`kaizen.local`)
- VPC endpoints: S3 gateway, ECR (dkr + api), CloudWatch Logs, Secrets Manager
- IAM roles: ECS task role, ECS task execution role, CI deploy role

## Output Contract

Your module exports `NetworkOutputs` consumed by all other agents:

```go
type NetworkOutputs struct {
    VpcId             pulumi.IDOutput
    PrivateSubnetIds  pulumi.StringArrayOutput
    PublicSubnetIds   pulumi.StringArrayOutput
    SecurityGroups    map[string]pulumi.IDOutput
    CloudMapNamespace pulumi.IDOutput
    TaskRoleArn       pulumi.StringOutput
    ExecRoleArn       pulumi.StringOutput
}
```

Do NOT change this struct shape without coordinating with all other Infra agents.

## Coding Standards

- Use `github.com/pulumi/pulumi-aws/sdk/v6/go/aws` provider
- All resources tagged with `Environment`, `Project=kaizen`, `ManagedBy=pulumi`
- Subnet CIDR allocation: public `/20`, private `/19` (room for expansion)
- NAT Gateway count configurable via Pulumi config (`kaizen:natGatewayCount`)
- Security group rules: least-privilege, no `0.0.0.0/0` on internal SGs
- Tests: Pulumi unit tests with mocked provider

## Work Tracking

```bash
gh issue list --label "infra-1" --state open
gh issue view <number>
```
