// Package network unit tests — exercise the GCP network module against
// pulumi.MockResourceMonitor and assert the shape and count of registered
// resources. Mirrors the AWS network module test approach.
package network

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// gcpNetworkMocks captures GCP resource registrations and returns reasonable
// mock outputs (self-links, IDs, names) so downstream apply chains resolve.
type gcpNetworkMocks struct {
	mu        sync.Mutex
	resources []resourceRecord
}

type resourceRecord struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *gcpNetworkMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, resourceRecord{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	id := args.Name + "_id"
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	switch args.TypeToken {
	case "gcp:compute/network:Network":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/networks/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/subnetwork:Subnetwork":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/subnetworks/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/router:Router":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/routers/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/routerNat:RouterNat":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/firewall:Firewall":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/firewalls/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:servicedirectory/namespace:Namespace":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/" + args.Name)
	case "gcp:vpcaccess/connector:Connector":
		outputs["selfLink"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/connectors/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	}

	return id, outputs, nil
}

func (m *gcpNetworkMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *gcpNetworkMocks) byType(typeToken string) []resourceRecord {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []resourceRecord
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			out = append(out, r)
		}
	}
	return out
}

// TestVpcCreatesExpectedResources runs NewVpc and asserts the basic shape:
// one network, two subnets, one router, one NAT.
func TestVpcCreatesExpectedResources(t *testing.T) {
	mocks := &gcpNetworkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewVpc(ctx)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewVpc failed: %v", err)
	}

	if got := len(mocks.byType("gcp:compute/network:Network")); got != 1 {
		t.Errorf("expected 1 Network, got %d", got)
	}
	if got := len(mocks.byType("gcp:compute/subnetwork:Subnetwork")); got != 2 {
		t.Errorf("expected 2 Subnetworks (public+private), got %d", got)
	}
	if got := len(mocks.byType("gcp:compute/router:Router")); got != 1 {
		t.Errorf("expected 1 Router, got %d", got)
	}
	if got := len(mocks.byType("gcp:compute/routerNat:RouterNat")); got != 1 {
		t.Errorf("expected 1 RouterNat, got %d", got)
	}
}

// TestFirewallRulesMatchAwsKeys is the parity gate — every key the AWS
// security_groups.go module exposes ("alb", "ecs", "rds", "msk", "redis",
// "m4b") must also be exposed by the GCP firewall module so downstream
// stages compose without branching.
func TestFirewallRulesMatchAwsKeys(t *testing.T) {
	mocks := &gcpNetworkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		vpc, err := NewVpc(ctx)
		if err != nil {
			return err
		}
		fw, err := NewFirewallRules(ctx, &FirewallArgs{NetworkId: vpc.NetworkId})
		if err != nil {
			return err
		}

		expectedKeys := []string{"alb", "ecs", "rds", "msk", "redis", "m4b"}
		for _, k := range expectedKeys {
			if _, ok := fw.Rules[k]; !ok {
				t.Errorf("firewall key %q missing from result", k)
			}
		}
		if got := len(fw.Rules); got != len(expectedKeys) {
			t.Errorf("expected %d firewall rules, got %d", len(expectedKeys), got)
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("RunErr failed: %v", err)
	}

	if got := len(mocks.byType("gcp:compute/firewall:Firewall")); got != 6 {
		t.Errorf("expected 6 Firewall resources, got %d", got)
	}
}

// TestServiceDirectoryNamespaceIsRegional verifies the Service Directory
// namespace is created with a Location parameter (regional resource), the
// GCP analogue of AWS Cloud Map's VPC-scoped private DNS namespace.
func TestServiceDirectoryNamespaceIsRegional(t *testing.T) {
	mocks := &gcpNetworkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewServiceDirectory(ctx, &ServiceDirectoryArgs{
			Region: pulumi.String("us-central1").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewServiceDirectory failed: %v", err)
	}

	namespaces := mocks.byType("gcp:servicedirectory/namespace:Namespace")
	if len(namespaces) != 1 {
		t.Fatalf("expected 1 Namespace, got %d", len(namespaces))
	}
	if _, ok := namespaces[0].Inputs["location"]; !ok {
		t.Errorf("Namespace input missing required 'location' field")
	}
}

// TestVpcConnectorCidrIsValidSlash28 ensures the Serverless VPC Access
// connector receives a /28 CIDR — required by GCP, the connector subnet
// must be exactly /28.
func TestVpcConnectorCidrIsValidSlash28(t *testing.T) {
	mocks := &gcpNetworkMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewVpcConnector(ctx, &VpcConnectorArgs{
			NetworkName: pulumi.String("kaizen-vpc").ToStringOutput(),
			Region:      pulumi.String("us-central1").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewVpcConnector failed: %v", err)
	}

	connectors := mocks.byType("gcp:vpcaccess/connector:Connector")
	if len(connectors) != 1 {
		t.Fatalf("expected 1 Connector, got %d", len(connectors))
	}
	cidrProp, ok := connectors[0].Inputs["ipCidrRange"]
	if !ok {
		t.Fatal("connector missing ipCidrRange input")
	}
	cidr := cidrProp.StringValue()
	// /28 mask is 8 chars + suffix "/28" — cheap structural check; format
	// validation belongs at the Pulumi/GCP level.
	if got := cidr[len(cidr)-3:]; got != "/28" {
		t.Errorf("expected /28 CIDR, got %q", cidr)
	}
}
