// Parameterized topology test for the database slice of Deploy(). Runs the
// network → database stages with mocks for both providers and asserts the
// private-IP enforcement contract: the relational database MUST be reachable
// only via the private subnet path on either cloud.
//
// AWS RDS expresses this through publiclyAccessible=false (or unset, which
// defaults to false) plus a DB subnet group anchored to the private subnets.
// GCP Cloud SQL expresses it through settings.ipConfiguration.ipv4Enabled=false
// plus a populated privateNetwork pointing at the VPC self-link.
//
// The test asserts on Pulumi resource INPUTS, not outputs, because the
// inputs are the configuration the operator is committing to — outputs are
// just mock echoes. Drift in either provider's private-IP posture trips here
// before it reaches a preview or an apply.
package main

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	awsfacade "github.com/kaizen-experimentation/infra/pkg/aws"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	gcpfacade "github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// dbTopologyMocks records resource registrations for both providers' DB
// stages plus their network prerequisites. Reuses the resource-record shape
// from fullstack_test.go via fsResource.
type dbTopologyMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *dbTopologyMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, fsResource{
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
	// --- AWS network prerequisites ---
	case "aws:ec2/vpc:Vpc":
		outputs["id"] = resource.NewStringProperty("vpc-mock")
	case "aws:ec2/subnet:Subnet":
		outputs["id"] = resource.NewStringProperty(args.Name + "-id")
	case "aws:ec2/securityGroup:SecurityGroup":
		outputs["id"] = resource.NewStringProperty("sg-" + args.Name)

	// --- AWS RDS ---
	case "aws:rds/instance:Instance":
		outputs["endpoint"] = resource.NewStringProperty(
			"kaizen-rds.abc123.us-east-1.rds.amazonaws.com:5432")
		outputs["port"] = resource.NewNumberProperty(5432)
		outputs["identifier"] = resource.NewStringProperty("kaizen-rds")
	case "aws:rds/parameterGroup:ParameterGroup",
		"aws:rds/subnetGroup:SubnetGroup":
		outputs["name"] = resource.NewStringProperty(args.Name)

	// --- GCP network prerequisites ---
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
	case "gcp:compute/globalAddress:GlobalAddress":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:servicenetworking/connection:Connection":
		outputs["peering"] = resource.NewStringProperty("servicenetworking-googleapis-com")

	// --- GCP Cloud SQL ---
	case "gcp:sql/databaseInstance:DatabaseInstance":
		outputs["connectionName"] = resource.NewStringProperty("kaizen-test:us-central1:kaizen-sql")
		outputs["name"] = resource.NewStringProperty(args.Name)
		outputs["firstIpAddress"] = resource.NewStringProperty("10.99.0.3")
		outputs["privateIpAddress"] = resource.NewStringProperty("10.99.0.3")
		outputs["publicIpAddress"] = resource.NewStringProperty("")
	case "gcp:sql/database:Database":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:sql/user:User":
		outputs["name"] = resource.NewStringProperty(args.Name)
	}

	return id, outputs, nil
}

func (m *dbTopologyMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	switch args.Token {
	case "aws:index/getAvailabilityZones:getAvailabilityZones":
		return resource.PropertyMap{
			"id": resource.NewStringProperty("us-east-1"),
			"names": resource.NewArrayProperty([]resource.PropertyValue{
				resource.NewStringProperty("us-east-1a"),
				resource.NewStringProperty("us-east-1b"),
				resource.NewStringProperty("us-east-1c"),
			}),
			"zoneIds": resource.NewArrayProperty([]resource.PropertyValue{
				resource.NewStringProperty("use1-az1"),
				resource.NewStringProperty("use1-az2"),
				resource.NewStringProperty("use1-az3"),
			}),
			"groupNames": resource.NewArrayProperty([]resource.PropertyValue{
				resource.NewStringProperty("us-east-1"),
				resource.NewStringProperty("us-east-1"),
				resource.NewStringProperty("us-east-1"),
			}),
		}, nil
	case "aws:index/getRegion:getRegion":
		return resource.PropertyMap{
			"name":        resource.NewStringProperty("us-east-1"),
			"description": resource.NewStringProperty("US East (N. Virginia)"),
			"endpoint":    resource.NewStringProperty("ec2.us-east-1.amazonaws.com"),
			"id":          resource.NewStringProperty("us-east-1"),
		}, nil
	}
	return resource.PropertyMap{}, nil
}

// TestDatabaseTopologyParameterized exercises network → database for both
// providers and verifies that the chosen private-IP posture is reflected in
// the Pulumi resource inputs. Failure means a default has been changed in a
// way that would let traffic from outside the VPC reach the database.
func TestDatabaseTopologyParameterized(t *testing.T) {
	cases := []struct {
		name        string
		runStages   func(t *testing.T, mocks *dbTopologyMocks) types.DatabaseOutputs
		verifyInput func(t *testing.T, mocks *dbTopologyMocks)
	}{
		{
			name: "aws",
			runStages: func(t *testing.T, mocks *dbTopologyMocks) types.DatabaseOutputs {
				var captured types.DatabaseOutputs
				err := pulumi.RunErr(func(ctx *pulumi.Context) error {
					cfg := kconfig.LoadConfig(ctx)
					netOut, err := awsfacade.NewNetwork(ctx, cfg)
					if err != nil {
						return err
					}
					captured, err = awsfacade.NewDatabase(ctx, cfg, netOut)
					return err
				}, pulumi.WithMocks("kaizen", "dev", mocks),
					configWithProvider("aws"))
				if err != nil {
					t.Fatalf("aws DB stage failed: %v", err)
				}
				return captured
			},
			verifyInput: func(t *testing.T, mocks *dbTopologyMocks) {
				inst := byType(mocks.resources, "aws:rds/instance:Instance")
				if len(inst) != 1 {
					t.Fatalf("aws: expected 1 RDS instance, got %d", len(inst))
				}
				ins := inst[0].Inputs
				// publiclyAccessible MUST NOT be true. Absent is fine — RDS
				// defaults to false. Explicit false also fine.
				if v, ok := ins["publiclyAccessible"]; ok && v.IsBool() && v.BoolValue() {
					t.Errorf("aws: RDS publiclyAccessible=true is not allowed; instance would be reachable from the public internet")
				}
				// dbSubnetGroupName MUST be set, anchoring the instance to the
				// private subnets the subnet group references.
				if v, ok := ins["dbSubnetGroupName"]; !ok || !v.HasValue() {
					t.Errorf("aws: RDS dbSubnetGroupName is unset; instance would be placed in the default VPC")
				}
				// vpcSecurityGroupIds MUST be set.
				if v, ok := ins["vpcSecurityGroupIds"]; !ok || !v.HasValue() {
					t.Errorf("aws: RDS vpcSecurityGroupIds is unset; no SG controls access")
				}
			},
		},
		{
			name: "gcp",
			runStages: func(t *testing.T, mocks *dbTopologyMocks) types.DatabaseOutputs {
				var captured types.DatabaseOutputs
				err := pulumi.RunErr(func(ctx *pulumi.Context) error {
					cfg := kconfig.LoadConfig(ctx)
					netOut, err := gcpfacade.NewNetwork(ctx, cfg)
					if err != nil {
						return err
					}
					captured, err = gcpfacade.NewDatabase(ctx, cfg, netOut)
					return err
				}, pulumi.WithMocks("kaizen", "dev", mocks),
					configWithProvider("gcp"))
				if err != nil {
					t.Fatalf("gcp DB stage failed: %v", err)
				}
				return captured
			},
			verifyInput: func(t *testing.T, mocks *dbTopologyMocks) {
				inst := byType(mocks.resources, "gcp:sql/databaseInstance:DatabaseInstance")
				if len(inst) != 1 {
					t.Fatalf("gcp: expected 1 Cloud SQL instance, got %d", len(inst))
				}
				ins := inst[0].Inputs
				// settings.ipConfiguration MUST set ipv4Enabled=false and a
				// populated privateNetwork.
				settings, ok := ins["settings"]
				if !ok || !settings.IsObject() {
					t.Fatalf("gcp: Cloud SQL instance missing 'settings' input")
				}
				ipCfg, ok := settings.ObjectValue()["ipConfiguration"]
				if !ok || !ipCfg.IsObject() {
					t.Fatalf("gcp: Cloud SQL settings.ipConfiguration is unset")
				}
				ipMap := ipCfg.ObjectValue()
				if v, ok := ipMap["ipv4Enabled"]; !ok {
					t.Errorf("gcp: settings.ipConfiguration.ipv4Enabled is unset; defaults to true and allows public IP")
				} else if !v.IsBool() {
					t.Errorf("gcp: settings.ipConfiguration.ipv4Enabled is not a bool: %v", v)
				} else if v.BoolValue() {
					t.Errorf("gcp: settings.ipConfiguration.ipv4Enabled=true is not allowed; instance would have a public IP")
				}
				priv, ok := ipMap["privateNetwork"]
				if !ok || !priv.HasValue() || (priv.IsString() && priv.StringValue() == "") {
					t.Errorf("gcp: settings.ipConfiguration.privateNetwork is unset; instance cannot reach the VPC privately")
				}
				// PSA reserved range must also have been provisioned during
				// the network stage — Cloud SQL on private IP requires it.
				if len(byType(mocks.resources, "gcp:compute/globalAddress:GlobalAddress")) < 1 {
					t.Errorf("gcp: no GlobalAddress for Private Service Access reservation")
				}
				if len(byType(mocks.resources, "gcp:servicenetworking/connection:Connection")) < 1 {
					t.Errorf("gcp: no servicenetworking.Connection for PSA peering")
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			mocks := &dbTopologyMocks{}
			_ = tc.runStages(t, mocks)
			tc.verifyInput(t, mocks)
		})
	}
}

// byType filters resources by Pulumi type token. Inlined to avoid touching
// network_topology_test.go's countByType helper.
func byType(records []fsResource, typeToken string) []fsResource {
	var out []fsResource
	for _, r := range records {
		if r.TypeToken == typeToken {
			out = append(out, r)
		}
	}
	return out
}
