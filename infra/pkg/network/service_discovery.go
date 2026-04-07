package network

import (
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/servicediscovery"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ServiceDiscoveryArgs holds the inputs required for the Cloud Map namespace.
type ServiceDiscoveryArgs struct {
	VpcId pulumi.IDOutput
}

// ServiceDiscoveryOutputs holds the outputs from the Cloud Map namespace.
type ServiceDiscoveryOutputs struct {
	NamespaceId  pulumi.IDOutput
	NamespaceArn pulumi.StringOutput
}

// NewServiceDiscovery creates a Cloud Map private DNS namespace for internal
// service discovery. ECS services register into this namespace so they can
// resolve each other via DNS (e.g. m1-assignment.kaizen.local).
func NewServiceDiscovery(ctx *pulumi.Context, args *ServiceDiscoveryArgs, opts ...pulumi.ResourceOption) (*ServiceDiscoveryOutputs, error) {
	ns, err := servicediscovery.NewPrivateDnsNamespace(ctx, "kaizen-local", &servicediscovery.PrivateDnsNamespaceArgs{
		Name:        pulumi.String("kaizen.local"),
		Description: pulumi.String("Private DNS namespace for Kaizen service discovery"),
		Vpc:         args.VpcId.ToStringOutput(),
	}, opts...)
	if err != nil {
		return nil, err
	}

	return &ServiceDiscoveryOutputs{
		NamespaceId:  ns.ID(),
		NamespaceArn: ns.Arn,
	}, nil
}
