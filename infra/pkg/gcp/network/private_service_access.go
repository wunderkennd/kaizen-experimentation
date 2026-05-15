// Private Service Access (PSA) — the VPC peering that lets Cloud SQL,
// Memorystore, and other Google-managed services be reachable via private
// IPs inside the Kaizen VPC. PSA is a one-time, VPC-scoped concern: a single
// peering serves every downstream consumer (#484 Cloud SQL, #485 Memorystore,
// future managed services), so it lives in the network module rather than
// being co-located with any one data store.
//
// Provisioning is two resources:
//   1. A compute.GlobalAddress with purpose=VPC_PEERING — reserves an RFC1918
//      range that Google's tenant projects will assign IPs from.
//   2. A servicenetworking.Connection — establishes the peering with the
//      servicenetworking.googleapis.com service.
//
// Operational note: servicenetworking.Connection is known to leave residue on
// `pulumi destroy` because GCP's API does not always fully release the peering
// state. Stack teardown procedures should run
//   gcloud services vpc-peerings delete --service=servicenetworking.googleapis.com --network=<vpc>
// after destroy if the connection lingers. This is a GCP-API quirk, not a
// Pulumi bug.
package network

import (
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/compute"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/servicenetworking"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// PrivateServiceAccessArgs carries the network references needed to attach
// the peering. NetworkId is the VPC self-link (from VpcOutputs.NetworkId).
type PrivateServiceAccessArgs struct {
	NetworkId pulumi.IDOutput
}

// PrivateServiceAccessOutputs exposes the reserved range name so downstream
// modules (Cloud SQL) can pass it as `privateNetwork` to their service
// instances. The Connection resource itself is not exported — consumers only
// need to depend on it implicitly via the resource graph.
type PrivateServiceAccessOutputs struct {
	// ReservedRangeName is the bare name of the reserved IP range
	// (e.g. "kaizen-psa-range"). Cloud SQL and Memorystore reference the
	// range by name when binding to private IPs.
	ReservedRangeName pulumi.StringOutput

	// ConnectionPeering is the peering identifier
	// (servicenetworking.googleapis.com). Exported for test introspection.
	ConnectionPeering pulumi.StringOutput
}

// NewPrivateServiceAccess provisions the PSA peering. The reserved range is
// /20 by default (1024 IPs) which leaves headroom for many managed-service
// instances; override via `kaizen-experimentation:gcpPsaPrefixLength`.
//
// Network sequencing: this must be created before any Cloud SQL or Memorystore
// instance binds to the VPC for private IP. The peering is a one-shot per
// VPC — multiple invocations would error.
func NewPrivateServiceAccess(ctx *pulumi.Context, args *PrivateServiceAccessArgs, opts ...pulumi.ResourceOption) (*PrivateServiceAccessOutputs, error) {
	cfg := config.New(ctx, "kaizen-experimentation")

	prefixLength := 20
	if v, err := cfg.TryInt("gcpPsaPrefixLength"); err == nil {
		prefixLength = v
	}

	// Reserved range. Address omitted → Google auto-allocates from the
	// privately-owned RFC1918 space; pin via gcpPsaAddress only if you need
	// a deterministic CIDR (e.g. for VPN advertisement).
	rangeArgs := &compute.GlobalAddressArgs{
		Name:         pulumi.String("kaizen-psa-range"),
		Purpose:      pulumi.String("VPC_PEERING"),
		AddressType:  pulumi.String("INTERNAL"),
		PrefixLength: pulumi.Int(prefixLength),
		Network:      args.NetworkId.ToStringOutput(),
		Description:  pulumi.String("Kaizen — reserved range for Private Service Access (Cloud SQL, Memorystore)"),
	}
	if v := cfg.Get("gcpPsaAddress"); v != "" {
		rangeArgs.Address = pulumi.String(v)
	}

	psaRange, err := compute.NewGlobalAddress(ctx, "kaizen-psa-range", rangeArgs, opts...)
	if err != nil {
		return nil, err
	}

	// servicenetworking.Connection — the peering itself.
	conn, err := servicenetworking.NewConnection(ctx, "kaizen-psa-connection", &servicenetworking.ConnectionArgs{
		Network: args.NetworkId.ToStringOutput(),
		Service: pulumi.String("servicenetworking.googleapis.com"),
		ReservedPeeringRanges: pulumi.StringArray{
			psaRange.Name,
		},
	}, opts...)
	if err != nil {
		return nil, err
	}

	return &PrivateServiceAccessOutputs{
		ReservedRangeName: psaRange.Name,
		ConnectionPeering: conn.Peering,
	}, nil
}
