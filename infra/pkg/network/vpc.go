// Package network provisions the Kaizen VPC foundation: VPC, subnets,
// internet gateway, NAT gateways, and route tables.
package network

import (
	"fmt"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ec2"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// VpcOutputs exposes the VPC resources that downstream modules consume.
type VpcOutputs struct {
	VpcId                pulumi.IDOutput
	PublicSubnetIds      pulumi.StringArrayOutput
	PrivateSubnetIds     pulumi.StringArrayOutput
	PrivateRouteTableIds pulumi.StringArrayOutput
}

// NewVpc creates the core networking foundation: VPC, subnets across 3 AZs,
// internet gateway, NAT gateways, and route tables.
func NewVpc(ctx *pulumi.Context) (*VpcOutputs, error) {
	tags := kconfig.CommonTags(ctx)
	cfg := config.New(ctx, "kaizen")

	// Configurable CIDR — default 10.0.0.0/16
	vpcCidr := cfg.Get("vpcCidr")
	if vpcCidr == "" {
		vpcCidr = "10.0.0.0/16"
	}

	// NAT gateway count — default 2 (use 1 for dev to save cost)
	natCount := cfg.GetInt("natGatewayCount")
	if natCount == 0 {
		natCount = 2
	}

	// Discover AZs in the current region.
	azs, err := aws.GetAvailabilityZones(ctx, &aws.GetAvailabilityZonesArgs{
		State: pulumi.StringRef("available"),
	})
	if err != nil {
		return nil, fmt.Errorf("get AZs: %w", err)
	}
	if len(azs.Names) < 3 {
		return nil, fmt.Errorf("need at least 3 AZs, got %d", len(azs.Names))
	}
	azNames := azs.Names[:3]

	// ── VPC ──────────────────────────────────────────────────────────────
	vpc, err := ec2.NewVpc(ctx, "kaizen-vpc", &ec2.VpcArgs{
		CidrBlock:          pulumi.String(vpcCidr),
		EnableDnsSupport:   pulumi.Bool(true),
		EnableDnsHostnames: pulumi.Bool(true),
		Tags: kconfig.MergeTags(tags, pulumi.StringMap{
			"Name": pulumi.String("kaizen-vpc"),
		}),
	})
	if err != nil {
		return nil, fmt.Errorf("create VPC: %w", err)
	}

	// ── Public subnets (/20 = 4,096 IPs each) ──────────────────────────
	// 10.0.0.0/20, 10.0.16.0/20, 10.0.32.0/20
	publicSubnets := make([]*ec2.Subnet, 3)
	publicSubnetIds := pulumi.StringArray{}
	for i, az := range azNames {
		cidr := fmt.Sprintf("10.0.%d.0/20", i*16)
		name := fmt.Sprintf("kaizen-public-%d", i)
		subnet, err := ec2.NewSubnet(ctx, name, &ec2.SubnetArgs{
			VpcId:               vpc.ID(),
			CidrBlock:           pulumi.String(cidr),
			AvailabilityZone:    pulumi.String(az),
			MapPublicIpOnLaunch: pulumi.Bool(true),
			Tags: kconfig.MergeTags(tags, pulumi.StringMap{
				"Name":                   pulumi.String(name),
				"kubernetes.io/role/elb": pulumi.String("1"),
			}),
		})
		if err != nil {
			return nil, fmt.Errorf("create public subnet %d: %w", i, err)
		}
		publicSubnets[i] = subnet
		publicSubnetIds = append(publicSubnetIds, subnet.ID().ToStringOutput())
	}

	// ── Private subnets (/19 = 8,192 IPs each) ─────────────────────────
	// 10.0.64.0/19, 10.0.96.0/19, 10.0.128.0/19
	privateSubnets := make([]*ec2.Subnet, 3)
	privateSubnetIds := pulumi.StringArray{}
	for i, az := range azNames {
		cidr := fmt.Sprintf("10.0.%d.0/19", 64+i*32)
		name := fmt.Sprintf("kaizen-private-%d", i)
		subnet, err := ec2.NewSubnet(ctx, name, &ec2.SubnetArgs{
			VpcId:            vpc.ID(),
			CidrBlock:        pulumi.String(cidr),
			AvailabilityZone: pulumi.String(az),
			Tags: kconfig.MergeTags(tags, pulumi.StringMap{
				"Name":                            pulumi.String(name),
				"kubernetes.io/role/internal-elb": pulumi.String("1"),
			}),
		})
		if err != nil {
			return nil, fmt.Errorf("create private subnet %d: %w", i, err)
		}
		privateSubnets[i] = subnet
		privateSubnetIds = append(privateSubnetIds, subnet.ID().ToStringOutput())
	}

	// ── Internet Gateway ────────────────────────────────────────────────
	igw, err := ec2.NewInternetGateway(ctx, "kaizen-igw", &ec2.InternetGatewayArgs{
		VpcId: vpc.ID(),
		Tags: kconfig.MergeTags(tags, pulumi.StringMap{
			"Name": pulumi.String("kaizen-igw"),
		}),
	})
	if err != nil {
		return nil, fmt.Errorf("create IGW: %w", err)
	}

	// ── Public route table → IGW ────────────────────────────────────────
	publicRT, err := ec2.NewRouteTable(ctx, "kaizen-public-rt", &ec2.RouteTableArgs{
		VpcId: vpc.ID(),
		Tags: kconfig.MergeTags(tags, pulumi.StringMap{
			"Name": pulumi.String("kaizen-public-rt"),
		}),
	})
	if err != nil {
		return nil, fmt.Errorf("create public route table: %w", err)
	}

	_, err = ec2.NewRoute(ctx, "kaizen-public-default", &ec2.RouteArgs{
		RouteTableId:         publicRT.ID(),
		DestinationCidrBlock: pulumi.String("0.0.0.0/0"),
		GatewayId:            igw.ID(),
	})
	if err != nil {
		return nil, fmt.Errorf("create public default route: %w", err)
	}

	for i, subnet := range publicSubnets {
		_, err = ec2.NewRouteTableAssociation(ctx, fmt.Sprintf("kaizen-public-rta-%d", i), &ec2.RouteTableAssociationArgs{
			SubnetId:     subnet.ID(),
			RouteTableId: publicRT.ID(),
		})
		if err != nil {
			return nil, fmt.Errorf("associate public subnet %d: %w", i, err)
		}
	}

	// ── NAT Gateways (in public subnets) ────────────────────────────────
	natGateways := make([]*ec2.NatGateway, natCount)
	for i := 0; i < natCount; i++ {
		eip, err := ec2.NewEip(ctx, fmt.Sprintf("kaizen-nat-eip-%d", i), &ec2.EipArgs{
			Domain: pulumi.String("vpc"),
			Tags: kconfig.MergeTags(tags, pulumi.StringMap{
				"Name": pulumi.String(fmt.Sprintf("kaizen-nat-eip-%d", i)),
			}),
		})
		if err != nil {
			return nil, fmt.Errorf("create NAT EIP %d: %w", i, err)
		}

		nat, err := ec2.NewNatGateway(ctx, fmt.Sprintf("kaizen-nat-%d", i), &ec2.NatGatewayArgs{
			AllocationId: eip.ID(),
			SubnetId:     publicSubnets[i].ID(),
			Tags: kconfig.MergeTags(tags, pulumi.StringMap{
				"Name": pulumi.String(fmt.Sprintf("kaizen-nat-%d", i)),
			}),
		})
		if err != nil {
			return nil, fmt.Errorf("create NAT gateway %d: %w", i, err)
		}
		natGateways[i] = nat
	}

	// ── Private route tables → NAT (round-robin across NAT GWs) ────────
	privateRTIds := pulumi.StringArray{}
	for i, subnet := range privateSubnets {
		natIdx := i % natCount
		rtName := fmt.Sprintf("kaizen-private-rt-%d", i)

		privateRT, err := ec2.NewRouteTable(ctx, rtName, &ec2.RouteTableArgs{
			VpcId: vpc.ID(),
			Tags: kconfig.MergeTags(tags, pulumi.StringMap{
				"Name": pulumi.String(rtName),
			}),
		})
		if err != nil {
			return nil, fmt.Errorf("create private route table %d: %w", i, err)
		}

		privateRTIds = append(privateRTIds, privateRT.ID().ToStringOutput())

		_, err = ec2.NewRoute(ctx, fmt.Sprintf("kaizen-private-default-%d", i), &ec2.RouteArgs{
			RouteTableId:         privateRT.ID(),
			DestinationCidrBlock: pulumi.String("0.0.0.0/0"),
			NatGatewayId:         natGateways[natIdx].ID(),
		})
		if err != nil {
			return nil, fmt.Errorf("create private default route %d: %w", i, err)
		}

		_, err = ec2.NewRouteTableAssociation(ctx, fmt.Sprintf("kaizen-private-rta-%d", i), &ec2.RouteTableAssociationArgs{
			SubnetId:     subnet.ID(),
			RouteTableId: privateRT.ID(),
		})
		if err != nil {
			return nil, fmt.Errorf("associate private subnet %d: %w", i, err)
		}
	}

	// ── Pulumi stack exports ────────────────────────────────────────────
	pubIds := publicSubnetIds.ToStringArrayOutput()
	privIds := privateSubnetIds.ToStringArrayOutput()
	ctx.Export("vpcId", vpc.ID())
	ctx.Export("publicSubnetIds", pubIds)
	ctx.Export("privateSubnetIds", privIds)

	privRTIds := privateRTIds.ToStringArrayOutput()
	ctx.Export("privateRouteTableIds", privRTIds)

	return &VpcOutputs{
		VpcId:                vpc.ID(),
		PublicSubnetIds:      pubIds,
		PrivateSubnetIds:     privIds,
		PrivateRouteTableIds: privRTIds,
	}, nil
}
