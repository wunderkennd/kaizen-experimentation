package network

import (
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/vpcaccess"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// VpcConnectorArgs holds the inputs for creating the Serverless VPC Access
// connector that lets Cloud Run reach private VPC resources (Cloud SQL,
// Memorystore, GCE M4b, Redpanda).
type VpcConnectorArgs struct {
	NetworkName pulumi.StringOutput
	Region      pulumi.StringOutput
}

// VpcConnectorOutputs exposes the connector's self-link so Cloud Run service
// templates can reference it via vpc_access.connector.
type VpcConnectorOutputs struct {
	ConnectorId       pulumi.IDOutput
	ConnectorSelfLink pulumi.StringOutput
}

// NewVpcConnector provisions the Serverless VPC Access connector. The
// connector occupies its own /28 IP range that is implicitly trusted by
// firewall rules whose sourceTag includes the matching workload role —
// Cloud Run egress from the connector reaches private resources tagged
// kaizen-rds / kaizen-redis / kaizen-msk just as ECS Fargate ENIs do on
// the AWS side.
func NewVpcConnector(ctx *pulumi.Context, args *VpcConnectorArgs, opts ...pulumi.ResourceOption) (*VpcConnectorOutputs, error) {
	cfg := config.New(ctx, "kaizen-experimentation")

	cidr := cfg.Get("gcpVpcConnectorCidr")
	if cidr == "" {
		// Default /28 inside the VPC supernet (10.0.0.0/16). Must not overlap
		// with any other subnet; placed immediately after the private subnet
		// (10.0.16.0/20 ends at 10.0.31.255) so VPC peering / Cloud Interconnect
		// advertising 10.0.0.0/16 still reaches the connector.
		cidr = "10.0.32.0/28"
	}

	machineType := cfg.Get("gcpVpcConnectorMachineType")
	if machineType == "" {
		// e2-micro is the cheapest connector instance type. Cloud Run autoscales
		// its connector pool between minInstances and maxInstances regardless,
		// so the per-instance type only affects per-throughput cost.
		machineType = "e2-micro"
	}

	conn, err := vpcaccess.NewConnector(ctx, "kaizen-vpc-connector", &vpcaccess.ConnectorArgs{
		Name:         pulumi.String("kaizen-vpc-connector"),
		Region:       args.Region,
		Network:      args.NetworkName,
		IpCidrRange:  pulumi.String(cidr),
		MachineType:  pulumi.String(machineType),
		MinInstances: pulumi.Int(2),
		MaxInstances: pulumi.Int(10),
	}, opts...)
	if err != nil {
		return nil, err
	}

	return &VpcConnectorOutputs{
		ConnectorId:       conn.ID(),
		ConnectorSelfLink: conn.SelfLink,
	}, nil
}
