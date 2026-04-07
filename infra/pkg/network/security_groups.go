package network

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ec2"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// SecurityGroupsArgs holds the inputs for creating the platform security groups.
type SecurityGroupsArgs struct {
	VpcId pulumi.IDOutput
}

// SecurityGroupsResult holds the created security group IDs keyed by role.
// Keys: "alb", "ecs", "rds", "msk", "redis", "m4b"
type SecurityGroupsResult struct {
	Groups map[string]pulumi.IDOutput
}

// NewSecurityGroups creates 6 least-privilege security groups with cross-references.
//
// Traffic flow:
//
//	Internet ─443→ [alb-sg] ─all TCP→ [ecs-sg / m4b-sg] ─specific ports→ [rds-sg, msk-sg, redis-sg]
//	                                   ↕ self + cross (gRPC)
func NewSecurityGroups(ctx *pulumi.Context, prefix string, args *SecurityGroupsArgs) (*SecurityGroupsResult, error) {
	vpcId := args.VpcId.ToStringOutput()

	// ── Phase 1: Create all security groups (no inline rules) ──────────────
	// This avoids circular dependency issues when groups reference each other.

	albSg, err := ec2.NewSecurityGroup(ctx, fmt.Sprintf("%s-alb-sg", prefix), &ec2.SecurityGroupArgs{
		VpcId:              vpcId,
		Description:        pulumi.String("ALB — public HTTPS ingress"),
		RevokeRulesOnDelete: pulumi.Bool(true),
		Tags:               pulumi.StringMap{"Name": pulumi.Sprintf("%s-alb-sg", prefix)},
	})
	if err != nil {
		return nil, fmt.Errorf("alb-sg: %w", err)
	}

	ecsSg, err := ec2.NewSecurityGroup(ctx, fmt.Sprintf("%s-ecs-sg", prefix), &ec2.SecurityGroupArgs{
		VpcId:              vpcId,
		Description:        pulumi.String("ECS Fargate services — inter-service gRPC"),
		RevokeRulesOnDelete: pulumi.Bool(true),
		Tags:               pulumi.StringMap{"Name": pulumi.Sprintf("%s-ecs-sg", prefix)},
	})
	if err != nil {
		return nil, fmt.Errorf("ecs-sg: %w", err)
	}

	rdsSg, err := ec2.NewSecurityGroup(ctx, fmt.Sprintf("%s-rds-sg", prefix), &ec2.SecurityGroupArgs{
		VpcId:              vpcId,
		Description:        pulumi.String("RDS PostgreSQL — port 5432 from ECS/M4b only"),
		RevokeRulesOnDelete: pulumi.Bool(true),
		Tags:               pulumi.StringMap{"Name": pulumi.Sprintf("%s-rds-sg", prefix)},
	})
	if err != nil {
		return nil, fmt.Errorf("rds-sg: %w", err)
	}

	mskSg, err := ec2.NewSecurityGroup(ctx, fmt.Sprintf("%s-msk-sg", prefix), &ec2.SecurityGroupArgs{
		VpcId:              vpcId,
		Description:        pulumi.String("MSK Kafka brokers — ports 9092/9094 from ECS/M4b only"),
		RevokeRulesOnDelete: pulumi.Bool(true),
		Tags:               pulumi.StringMap{"Name": pulumi.Sprintf("%s-msk-sg", prefix)},
	})
	if err != nil {
		return nil, fmt.Errorf("msk-sg: %w", err)
	}

	redisSg, err := ec2.NewSecurityGroup(ctx, fmt.Sprintf("%s-redis-sg", prefix), &ec2.SecurityGroupArgs{
		VpcId:              vpcId,
		Description:        pulumi.String("ElastiCache Redis — port 6379 from ECS/M4b only"),
		RevokeRulesOnDelete: pulumi.Bool(true),
		Tags:               pulumi.StringMap{"Name": pulumi.Sprintf("%s-redis-sg", prefix)},
	})
	if err != nil {
		return nil, fmt.Errorf("redis-sg: %w", err)
	}

	m4bSg, err := ec2.NewSecurityGroup(ctx, fmt.Sprintf("%s-m4b-sg", prefix), &ec2.SecurityGroupArgs{
		VpcId:              vpcId,
		Description:        pulumi.String("M4b Policy service (EC2) — same rules as ECS"),
		RevokeRulesOnDelete: pulumi.Bool(true),
		Tags:               pulumi.StringMap{"Name": pulumi.Sprintf("%s-m4b-sg", prefix)},
	})
	if err != nil {
		return nil, fmt.Errorf("m4b-sg: %w", err)
	}

	// ── Phase 2: ALB rules ─────────────────────────────────────────────────

	// ALB ingress: HTTPS from the internet.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-alb-in-https", prefix), &ec2.SecurityGroupRuleArgs{
		Type:            pulumi.String("ingress"),
		SecurityGroupId: albSg.ID().ToStringOutput(),
		Protocol:        pulumi.String("tcp"),
		FromPort:        pulumi.Int(443),
		ToPort:          pulumi.Int(443),
		CidrBlocks:      pulumi.StringArray{pulumi.String("0.0.0.0/0")},
		Description:     pulumi.String("HTTPS from internet"),
	}); err != nil {
		return nil, fmt.Errorf("alb-in-https: %w", err)
	}

	// ALB egress: forward to ECS services (various ports).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-alb-out-ecs", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          albSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    ecsSg.ID().ToStringOutput(),
		Description:              pulumi.String("Forward to ECS Fargate services"),
	}); err != nil {
		return nil, fmt.Errorf("alb-out-ecs: %w", err)
	}

	// ALB egress: forward to M4b (port 50054).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-alb-out-m4b", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          albSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    m4bSg.ID().ToStringOutput(),
		Description:              pulumi.String("Forward to M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("alb-out-m4b: %w", err)
	}

	// ── Phase 3: ECS rules ─────────────────────────────────────────────────

	// ECS ingress: from ALB.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-in-alb", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          ecsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    albSg.ID().ToStringOutput(),
		Description:              pulumi.String("Inbound from ALB"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-in-alb: %w", err)
	}

	// ECS ingress: self (inter-service gRPC).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-in-self", prefix), &ec2.SecurityGroupRuleArgs{
		Type:            pulumi.String("ingress"),
		SecurityGroupId: ecsSg.ID().ToStringOutput(),
		Protocol:        pulumi.String("tcp"),
		FromPort:        pulumi.Int(0),
		ToPort:          pulumi.Int(65535),
		Self:            pulumi.Bool(true),
		Description:     pulumi.String("Inter-service gRPC (self)"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-in-self: %w", err)
	}

	// ECS ingress: from M4b (cross-compute gRPC).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-in-m4b", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          ecsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    m4bSg.ID().ToStringOutput(),
		Description:              pulumi.String("Inbound from M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-in-m4b: %w", err)
	}

	// ECS egress: to RDS (PostgreSQL).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-out-rds", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          ecsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(5432),
		ToPort:                   pulumi.Int(5432),
		SourceSecurityGroupId:    rdsSg.ID().ToStringOutput(),
		Description:              pulumi.String("PostgreSQL to RDS"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-out-rds: %w", err)
	}

	// ECS egress: to MSK (plaintext Kafka).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-out-msk-plain", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          ecsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9092),
		ToPort:                   pulumi.Int(9092),
		SourceSecurityGroupId:    mskSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka plaintext to MSK"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-out-msk-plain: %w", err)
	}

	// ECS egress: to MSK (TLS Kafka).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-out-msk-tls", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          ecsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9094),
		ToPort:                   pulumi.Int(9094),
		SourceSecurityGroupId:    mskSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka TLS to MSK"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-out-msk-tls: %w", err)
	}

	// ECS egress: to Redis (ElastiCache).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-out-redis", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          ecsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(6379),
		ToPort:                   pulumi.Int(6379),
		SourceSecurityGroupId:    redisSg.ID().ToStringOutput(),
		Description:              pulumi.String("Redis to ElastiCache"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-out-redis: %w", err)
	}

	// ECS egress: self (inter-service gRPC return path).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-out-self", prefix), &ec2.SecurityGroupRuleArgs{
		Type:            pulumi.String("egress"),
		SecurityGroupId: ecsSg.ID().ToStringOutput(),
		Protocol:        pulumi.String("tcp"),
		FromPort:        pulumi.Int(0),
		ToPort:          pulumi.Int(65535),
		Self:            pulumi.Bool(true),
		Description:     pulumi.String("Inter-service gRPC (self)"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-out-self: %w", err)
	}

	// ECS egress: to M4b (cross-compute gRPC).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-ecs-out-m4b", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          ecsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    m4bSg.ID().ToStringOutput(),
		Description:              pulumi.String("gRPC to M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-out-m4b: %w", err)
	}

	// ── Phase 4: Data store rules (ingress only — stateful return is implicit) ─

	// RDS ingress: PostgreSQL from ECS.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-rds-in-ecs", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          rdsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(5432),
		ToPort:                   pulumi.Int(5432),
		SourceSecurityGroupId:    ecsSg.ID().ToStringOutput(),
		Description:              pulumi.String("PostgreSQL from ECS services"),
	}); err != nil {
		return nil, fmt.Errorf("rds-in-ecs: %w", err)
	}

	// RDS ingress: PostgreSQL from M4b.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-rds-in-m4b", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          rdsSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(5432),
		ToPort:                   pulumi.Int(5432),
		SourceSecurityGroupId:    m4bSg.ID().ToStringOutput(),
		Description:              pulumi.String("PostgreSQL from M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("rds-in-m4b: %w", err)
	}

	// MSK ingress: Kafka plaintext from ECS.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-msk-in-ecs-plain", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          mskSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9092),
		ToPort:                   pulumi.Int(9092),
		SourceSecurityGroupId:    ecsSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka plaintext from ECS services"),
	}); err != nil {
		return nil, fmt.Errorf("msk-in-ecs-plain: %w", err)
	}

	// MSK ingress: Kafka TLS from ECS.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-msk-in-ecs-tls", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          mskSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9094),
		ToPort:                   pulumi.Int(9094),
		SourceSecurityGroupId:    ecsSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka TLS from ECS services"),
	}); err != nil {
		return nil, fmt.Errorf("msk-in-ecs-tls: %w", err)
	}

	// MSK ingress: Kafka plaintext from M4b.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-msk-in-m4b-plain", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          mskSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9092),
		ToPort:                   pulumi.Int(9092),
		SourceSecurityGroupId:    m4bSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka plaintext from M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("msk-in-m4b-plain: %w", err)
	}

	// MSK ingress: Kafka TLS from M4b.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-msk-in-m4b-tls", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          mskSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9094),
		ToPort:                   pulumi.Int(9094),
		SourceSecurityGroupId:    m4bSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka TLS from M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("msk-in-m4b-tls: %w", err)
	}

	// Redis ingress: from ECS.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-redis-in-ecs", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          redisSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(6379),
		ToPort:                   pulumi.Int(6379),
		SourceSecurityGroupId:    ecsSg.ID().ToStringOutput(),
		Description:              pulumi.String("Redis from ECS services"),
	}); err != nil {
		return nil, fmt.Errorf("redis-in-ecs: %w", err)
	}

	// Redis ingress: from M4b.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-redis-in-m4b", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          redisSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(6379),
		ToPort:                   pulumi.Int(6379),
		SourceSecurityGroupId:    m4bSg.ID().ToStringOutput(),
		Description:              pulumi.String("Redis from M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("redis-in-m4b: %w", err)
	}

	// ── Phase 5: M4b rules (mirrors ECS rules) ────────────────────────────

	// M4b ingress: from ALB.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-in-alb", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          m4bSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    albSg.ID().ToStringOutput(),
		Description:              pulumi.String("Inbound from ALB"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-in-alb: %w", err)
	}

	// M4b ingress: self.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-in-self", prefix), &ec2.SecurityGroupRuleArgs{
		Type:            pulumi.String("ingress"),
		SecurityGroupId: m4bSg.ID().ToStringOutput(),
		Protocol:        pulumi.String("tcp"),
		FromPort:        pulumi.Int(0),
		ToPort:          pulumi.Int(65535),
		Self:            pulumi.Bool(true),
		Description:     pulumi.String("Self-reference (M4b instances)"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-in-self: %w", err)
	}

	// M4b ingress: from ECS (cross-compute gRPC).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-in-ecs", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("ingress"),
		SecurityGroupId:          m4bSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    ecsSg.ID().ToStringOutput(),
		Description:              pulumi.String("Inbound from ECS Fargate services"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-in-ecs: %w", err)
	}

	// M4b egress: to RDS.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-out-rds", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          m4bSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(5432),
		ToPort:                   pulumi.Int(5432),
		SourceSecurityGroupId:    rdsSg.ID().ToStringOutput(),
		Description:              pulumi.String("PostgreSQL to RDS"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-out-rds: %w", err)
	}

	// M4b egress: to MSK (plaintext).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-out-msk-plain", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          m4bSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9092),
		ToPort:                   pulumi.Int(9092),
		SourceSecurityGroupId:    mskSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka plaintext to MSK"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-out-msk-plain: %w", err)
	}

	// M4b egress: to MSK (TLS).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-out-msk-tls", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          m4bSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(9094),
		ToPort:                   pulumi.Int(9094),
		SourceSecurityGroupId:    mskSg.ID().ToStringOutput(),
		Description:              pulumi.String("Kafka TLS to MSK"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-out-msk-tls: %w", err)
	}

	// M4b egress: to Redis.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-out-redis", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          m4bSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(6379),
		ToPort:                   pulumi.Int(6379),
		SourceSecurityGroupId:    redisSg.ID().ToStringOutput(),
		Description:              pulumi.String("Redis to ElastiCache"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-out-redis: %w", err)
	}

	// M4b egress: self.
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-out-self", prefix), &ec2.SecurityGroupRuleArgs{
		Type:            pulumi.String("egress"),
		SecurityGroupId: m4bSg.ID().ToStringOutput(),
		Protocol:        pulumi.String("tcp"),
		FromPort:        pulumi.Int(0),
		ToPort:          pulumi.Int(65535),
		Self:            pulumi.Bool(true),
		Description:     pulumi.String("Self-reference (M4b instances)"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-out-self: %w", err)
	}

	// M4b egress: to ECS (cross-compute gRPC).
	if _, err = ec2.NewSecurityGroupRule(ctx, fmt.Sprintf("%s-m4b-out-ecs", prefix), &ec2.SecurityGroupRuleArgs{
		Type:                     pulumi.String("egress"),
		SecurityGroupId:          m4bSg.ID().ToStringOutput(),
		Protocol:                 pulumi.String("tcp"),
		FromPort:                 pulumi.Int(0),
		ToPort:                   pulumi.Int(65535),
		SourceSecurityGroupId:    ecsSg.ID().ToStringOutput(),
		Description:              pulumi.String("gRPC to ECS Fargate services"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-out-ecs: %w", err)
	}

	return &SecurityGroupsResult{
		Groups: map[string]pulumi.IDOutput{
			"alb":   albSg.ID(),
			"ecs":   ecsSg.ID(),
			"rds":   rdsSg.ID(),
			"msk":   mskSg.ID(),
			"redis": redisSg.ID(),
			"m4b":   m4bSg.ID(),
		},
	}, nil
}
