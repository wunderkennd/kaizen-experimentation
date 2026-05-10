// Package network provisions VPC endpoints for private AWS service access,
// eliminating NAT gateway data processing charges for high-volume services.
package network

import (
	"fmt"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ec2"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// VpcEndpointArgs holds the inputs for creating VPC endpoints.
type VpcEndpointArgs struct {
	VpcId                pulumi.IDOutput
	PrivateSubnetIds     pulumi.StringArrayOutput
	PrivateRouteTableIds pulumi.StringArrayOutput
	EcsSecurityGroupId   pulumi.IDOutput
	M4bSecurityGroupId   pulumi.IDOutput
}

// VpcEndpointOutputs holds the VPC endpoint resources for downstream modules.
type VpcEndpointOutputs struct {
	EndpointSecurityGroupId pulumi.IDOutput
	S3EndpointId            pulumi.IDOutput
}

// NewVpcEndpoints creates VPC endpoints for private AWS service access.
//
// Endpoints created:
//   - S3 (Gateway): routes S3 traffic through route tables, not NAT
//   - ECR DKR + API (Interface): private Docker image pulls
//   - CloudWatch Logs (Interface): private log delivery
//   - Secrets Manager (Interface): private secret retrieval
//
// Also adds HTTPS egress rules to ECS and M4b security groups so compute
// resources can reach the interface endpoint ENIs on port 443.
func NewVpcEndpoints(ctx *pulumi.Context, args *VpcEndpointArgs) (*VpcEndpointOutputs, error) {
	tags := kconfig.CommonTags(ctx)

	region, err := aws.GetRegion(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("get region: %w", err)
	}

	// ── Endpoint Security Group ─────────────────────────────────────────
	// Interface endpoints receive HTTPS traffic from ECS and M4b compute.
	endpointSg, err := ec2.NewSecurityGroup(ctx, "kaizen-vpce-sg", &ec2.SecurityGroupArgs{
		VpcId:               args.VpcId.ToStringOutput(),
		Description:         pulumi.String("VPC interface endpoints — HTTPS from compute"),
		RevokeRulesOnDelete: pulumi.Bool(true),
		Tags: kconfig.MergeTags(tags, pulumi.StringMap{
			"Name": pulumi.String("kaizen-vpce-sg"),
		}),
	})
	if err != nil {
		return nil, fmt.Errorf("vpce-sg: %w", err)
	}

	// Endpoint SG ingress: HTTPS from ECS Fargate services.
	if _, err = ec2.NewSecurityGroupRule(ctx, "kaizen-vpce-in-ecs", &ec2.SecurityGroupRuleArgs{
		Type:                  pulumi.String("ingress"),
		SecurityGroupId:       endpointSg.ID().ToStringOutput(),
		Protocol:              pulumi.String("tcp"),
		FromPort:              pulumi.Int(443),
		ToPort:                pulumi.Int(443),
		SourceSecurityGroupId: args.EcsSecurityGroupId.ToStringOutput(),
		Description:           pulumi.String("HTTPS from ECS Fargate services"),
	}); err != nil {
		return nil, fmt.Errorf("vpce-in-ecs: %w", err)
	}

	// Endpoint SG ingress: HTTPS from M4b Policy service.
	if _, err = ec2.NewSecurityGroupRule(ctx, "kaizen-vpce-in-m4b", &ec2.SecurityGroupRuleArgs{
		Type:                  pulumi.String("ingress"),
		SecurityGroupId:       endpointSg.ID().ToStringOutput(),
		Protocol:              pulumi.String("tcp"),
		FromPort:              pulumi.Int(443),
		ToPort:                pulumi.Int(443),
		SourceSecurityGroupId: args.M4bSecurityGroupId.ToStringOutput(),
		Description:           pulumi.String("HTTPS from M4b Policy service"),
	}); err != nil {
		return nil, fmt.Errorf("vpce-in-m4b: %w", err)
	}

	// ECS egress: HTTPS to VPC endpoints.
	if _, err = ec2.NewSecurityGroupRule(ctx, "kaizen-ecs-out-vpce", &ec2.SecurityGroupRuleArgs{
		Type:                  pulumi.String("egress"),
		SecurityGroupId:       args.EcsSecurityGroupId.ToStringOutput(),
		Protocol:              pulumi.String("tcp"),
		FromPort:              pulumi.Int(443),
		ToPort:                pulumi.Int(443),
		SourceSecurityGroupId: endpointSg.ID().ToStringOutput(),
		Description:           pulumi.String("HTTPS to VPC endpoints"),
	}); err != nil {
		return nil, fmt.Errorf("ecs-out-vpce: %w", err)
	}

	// M4b egress: HTTPS to VPC endpoints.
	if _, err = ec2.NewSecurityGroupRule(ctx, "kaizen-m4b-out-vpce", &ec2.SecurityGroupRuleArgs{
		Type:                  pulumi.String("egress"),
		SecurityGroupId:       args.M4bSecurityGroupId.ToStringOutput(),
		Protocol:              pulumi.String("tcp"),
		FromPort:              pulumi.Int(443),
		ToPort:                pulumi.Int(443),
		SourceSecurityGroupId: endpointSg.ID().ToStringOutput(),
		Description:           pulumi.String("HTTPS to VPC endpoints"),
	}); err != nil {
		return nil, fmt.Errorf("m4b-out-vpce: %w", err)
	}

	// ── S3 Gateway Endpoint ─────────────────────────────────────────────
	// Gateway endpoints route traffic via prefix lists in route tables,
	// so they don't need security groups or subnet placement.
	s3Endpoint, err := ec2.NewVpcEndpoint(ctx, "kaizen-s3-endpoint", &ec2.VpcEndpointArgs{
		VpcId:           args.VpcId.ToStringOutput(),
		ServiceName:     pulumi.String(fmt.Sprintf("com.amazonaws.%s.s3", region.Name)),
		VpcEndpointType: pulumi.String("Gateway"),
		RouteTableIds:   args.PrivateRouteTableIds,
		Tags: kconfig.MergeTags(tags, pulumi.StringMap{
			"Name": pulumi.String("kaizen-s3-endpoint"),
		}),
	})
	if err != nil {
		return nil, fmt.Errorf("s3 endpoint: %w", err)
	}

	// ── Interface Endpoints ─────────────────────────────────────────────
	// All interface endpoints share the same SG, subnets, and private DNS.
	sgIds := pulumi.StringArray{endpointSg.ID().ToStringOutput()}

	interfaceEndpoints := []struct {
		name    string
		service string
	}{
		{"kaizen-ecr-dkr-endpoint", fmt.Sprintf("com.amazonaws.%s.ecr.dkr", region.Name)},
		{"kaizen-ecr-api-endpoint", fmt.Sprintf("com.amazonaws.%s.ecr.api", region.Name)},
		{"kaizen-logs-endpoint", fmt.Sprintf("com.amazonaws.%s.logs", region.Name)},
		{"kaizen-secretsmanager-endpoint", fmt.Sprintf("com.amazonaws.%s.secretsmanager", region.Name)},
	}

	for _, ep := range interfaceEndpoints {
		if _, err = ec2.NewVpcEndpoint(ctx, ep.name, &ec2.VpcEndpointArgs{
			VpcId:             args.VpcId.ToStringOutput(),
			ServiceName:       pulumi.String(ep.service),
			VpcEndpointType:   pulumi.String("Interface"),
			SubnetIds:         args.PrivateSubnetIds,
			SecurityGroupIds:  sgIds,
			PrivateDnsEnabled: pulumi.Bool(true),
			Tags: kconfig.MergeTags(tags, pulumi.StringMap{
				"Name": pulumi.String(ep.name),
			}),
		}); err != nil {
			return nil, fmt.Errorf("%s: %w", ep.name, err)
		}
	}

	return &VpcEndpointOutputs{
		EndpointSecurityGroupId: endpointSg.ID(),
		S3EndpointId:            s3Endpoint.ID(),
	}, nil
}
