package network

import (
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/servicedirectory"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ServiceDirectoryArgs holds the inputs for creating the Service Directory
// namespace.
type ServiceDirectoryArgs struct {
	// Region is the GCP region the namespace lives in. Service Directory
	// namespaces are regional, unlike AWS Cloud Map which is VPC-scoped.
	Region pulumi.StringOutput
}

// ServiceDirectoryOutputs holds the namespace identifier returned to
// callers. Mirrors the shape of ServiceDiscoveryOutputs in the AWS module
// so the top-level facade can map both into types.NetworkOutputs uniformly.
type ServiceDirectoryOutputs struct {
	// NamespaceId is the Pulumi-managed ID of the namespace, used by Cloud
	// Run services and the AWS-side downstream consumer that types this
	// field as pulumi.IDOutput.
	NamespaceId pulumi.IDOutput
	// NamespaceName is the fully-qualified Service Directory resource name
	// (projects/<P>/locations/<R>/namespaces/<N>) used by service
	// registrations and IAM bindings.
	NamespaceName pulumi.StringOutput
}

// NewServiceDirectory creates the Service Directory namespace that backs
// internal service discovery for Kaizen on GCP. This is the GCP equivalent
// of the AWS Cloud Map private DNS namespace; Cloud Run services register
// here and resolve each other via Service Directory's HTTP API or via the
// auto-published private DNS zone.
func NewServiceDirectory(ctx *pulumi.Context, args *ServiceDirectoryArgs, opts ...pulumi.ResourceOption) (*ServiceDirectoryOutputs, error) {
	ns, err := servicedirectory.NewNamespace(ctx, "kaizen-local", &servicedirectory.NamespaceArgs{
		NamespaceId: pulumi.String("kaizen-local"),
		Location:    args.Region,
	}, opts...)
	if err != nil {
		return nil, err
	}

	return &ServiceDirectoryOutputs{
		NamespaceId:   ns.ID(),
		NamespaceName: ns.Name,
	}, nil
}
