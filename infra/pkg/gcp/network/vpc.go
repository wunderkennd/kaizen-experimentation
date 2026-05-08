// Package network provisions the Kaizen GCP networking foundation: a custom
// VPC, public and private regional subnetworks, a Cloud Router, and a Cloud
// NAT gateway for private-subnet egress. GCP subnets are regional and span
// every zone in the region, so a single private subnet plays the role of the
// three AZ-pinned private subnets on AWS.
package network

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/compute"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// VpcOutputs exposes the GCP VPC resources that downstream modules consume.
// Field shapes mirror the AWS VpcOutputs so callers can compose without
// branching, but the underlying values use GCP self-links and regional IDs.
type VpcOutputs struct {
	// NetworkId is the GCP network self-link (returned by .ID() on
	// compute.Network — Pulumi returns the resource's self-link as the ID).
	NetworkId pulumi.IDOutput

	// NetworkName is the bare network name, used by firewall rules and the
	// Serverless VPC Access connector that need a name reference instead of a
	// self-link.
	NetworkName pulumi.StringOutput

	// PublicSubnetIds is a single-element array holding the public regional
	// subnet's self-link. Returned as an array so the shared NetworkOutputs
	// shape (PublicSubnetIds pulumi.StringArrayOutput) is satisfied without a
	// special case.
	PublicSubnetIds pulumi.StringArrayOutput

	// PrivateSubnetIds is a single-element array holding the private regional
	// subnet's self-link.
	PrivateSubnetIds pulumi.StringArrayOutput

	// PrivateSubnetName is the bare name of the private subnet. The
	// Serverless VPC Access connector references its host subnet by name.
	PrivateSubnetName pulumi.StringOutput

	// Region is the configured GCP region. Downstream modules (Cloud SQL,
	// Memorystore, VPC connector) need this and the network module is the
	// canonical place it is read once.
	Region pulumi.StringOutput
}

// NewVpc creates the core GCP networking foundation: a custom-mode VPC,
// public and private regional subnetworks, a Cloud Router, and a Cloud NAT
// gateway. Mirrors infra/pkg/aws/network/vpc.go in shape but uses GCP's
// regional subnet model.
func NewVpc(ctx *pulumi.Context) (*VpcOutputs, error) {
	cfg := config.New(ctx, "kaizen")
	gcpCfg := config.New(ctx, "gcp")

	region := gcpCfg.Get("region")
	if region == "" {
		region = "us-central1"
	}

	publicCidr := cfg.Get("gcpPublicSubnetCidr")
	if publicCidr == "" {
		publicCidr = "10.0.0.0/20"
	}
	privateCidr := cfg.Get("gcpPrivateSubnetCidr")
	if privateCidr == "" {
		privateCidr = "10.0.16.0/20"
	}

	// ── VPC (custom-mode network — no auto-created subnets) ────────────
	network, err := compute.NewNetwork(ctx, "kaizen-vpc", &compute.NetworkArgs{
		Name:                  pulumi.String("kaizen-vpc"),
		AutoCreateSubnetworks: pulumi.Bool(false),
		RoutingMode:           pulumi.String("REGIONAL"),
		Description:           pulumi.String("Kaizen experimentation VPC (custom mode)"),
	})
	if err != nil {
		return nil, fmt.Errorf("create VPC: %w", err)
	}

	// ── Public regional subnet ─────────────────────────────────────────
	// Holds external load balancer NEGs and any internet-facing GCE.
	publicSubnet, err := compute.NewSubnetwork(ctx, "kaizen-public", &compute.SubnetworkArgs{
		Name:                  pulumi.String("kaizen-public"),
		Network:               network.ID(),
		IpCidrRange:           pulumi.String(publicCidr),
		Region:                pulumi.String(region),
		PrivateIpGoogleAccess: pulumi.Bool(true),
		Description:           pulumi.String("Public regional subnet for ingress and NAT"),
	})
	if err != nil {
		return nil, fmt.Errorf("create public subnet: %w", err)
	}

	// ── Private regional subnet ────────────────────────────────────────
	// Holds Cloud SQL (via private services access), Memorystore, GCE M4b,
	// and Cloud Run egress via the Serverless VPC Access connector.
	privateSubnet, err := compute.NewSubnetwork(ctx, "kaizen-private", &compute.SubnetworkArgs{
		Name:                  pulumi.String("kaizen-private"),
		Network:               network.ID(),
		IpCidrRange:           pulumi.String(privateCidr),
		Region:                pulumi.String(region),
		PrivateIpGoogleAccess: pulumi.Bool(true),
		Description:           pulumi.String("Private regional subnet for stateful workloads"),
	})
	if err != nil {
		return nil, fmt.Errorf("create private subnet: %w", err)
	}

	// ── Cloud Router + Cloud NAT (egress for private subnet) ───────────
	// GCP equivalent of the AWS NAT gateway; one Cloud NAT services every
	// zone in the region so a single resource replaces the AWS NAT-per-AZ
	// fleet.
	router, err := compute.NewRouter(ctx, "kaizen-router", &compute.RouterArgs{
		Name:        pulumi.String("kaizen-router"),
		Network:     network.ID(),
		Region:      pulumi.String(region),
		Description: pulumi.String("Cloud Router for NAT egress and dynamic routing"),
	})
	if err != nil {
		return nil, fmt.Errorf("create Cloud Router: %w", err)
	}

	if _, err = compute.NewRouterNat(ctx, "kaizen-nat", &compute.RouterNatArgs{
		Name:                          pulumi.String("kaizen-nat"),
		Router:                        router.Name,
		Region:                        pulumi.String(region),
		NatIpAllocateOption:           pulumi.String("AUTO_ONLY"),
		SourceSubnetworkIpRangesToNat: pulumi.String("LIST_OF_SUBNETWORKS"),
		Subnetworks: compute.RouterNatSubnetworkArray{
			&compute.RouterNatSubnetworkArgs{
				Name: privateSubnet.ID(),
				SourceIpRangesToNats: pulumi.StringArray{
					pulumi.String("ALL_IP_RANGES"),
				},
			},
		},
	}); err != nil {
		return nil, fmt.Errorf("create Cloud NAT: %w", err)
	}

	// ── Pulumi stack exports ───────────────────────────────────────────
	pubIds := pulumi.StringArray{publicSubnet.ID().ToStringOutput()}.ToStringArrayOutput()
	privIds := pulumi.StringArray{privateSubnet.ID().ToStringOutput()}.ToStringArrayOutput()
	ctx.Export("gcpNetworkId", network.ID())
	ctx.Export("gcpPublicSubnetIds", pubIds)
	ctx.Export("gcpPrivateSubnetIds", privIds)
	ctx.Export("gcpRegion", pulumi.String(region))

	return &VpcOutputs{
		NetworkId:         network.ID(),
		NetworkName:       network.Name,
		PublicSubnetIds:   pubIds,
		PrivateSubnetIds:  privIds,
		PrivateSubnetName: privateSubnet.Name,
		Region:            pulumi.String(region).ToStringOutput(),
	}, nil
}
