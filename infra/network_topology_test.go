// Parameterized topology tests for the network slice of Deploy(). Run the
// network stage with mocks for both cloudProvider: aws and cloudProvider:
// gcp, and assert that the resulting types.NetworkOutputs has matching
// field shapes across providers. Subsequent issues (#480-#486) extend
// coverage to the remaining stages.
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

// gcpTopologyMocks captures GCP resource registrations for the topology
// test. Keeps GCP-specific output shapes — self-links and resource names —
// alongside the AWS mocks already defined in fullstack_test.go.
type gcpTopologyMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *gcpTopologyMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
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

func (m *gcpTopologyMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

// TestNetworkTopologyParameterized runs the network stage for both
// providers and asserts the NetworkOutputs shape contract. AWS already has
// a comprehensive fullstack test; this case re-runs only the network slice
// to keep the AWS↔GCP comparison apples-to-apples.
func TestNetworkTopologyParameterized(t *testing.T) {
	cases := []struct {
		name               string
		runner             func(t *testing.T) types.NetworkOutputs
		expectAwsResources bool
		expectGcpResources bool
	}{
		{
			name: "aws",
			runner: func(t *testing.T) types.NetworkOutputs {
				mocks := &fullstackMocks{}
				var captured types.NetworkOutputs
				err := pulumi.RunErr(func(ctx *pulumi.Context) error {
					cfg := kconfig.LoadConfig(ctx)
					out, err := awsfacade.NewNetwork(ctx, cfg)
					if err != nil {
						return err
					}
					captured = out
					return nil
				}, pulumi.WithMocks("kaizen", "dev", mocks),
					configWithProvider("aws"))
				if err != nil {
					t.Fatalf("aws NewNetwork failed: %v", err)
				}
				if got := mocks.count("aws:ec2/vpc:Vpc"); got != 1 {
					t.Errorf("aws: expected 1 VPC, got %d", got)
				}
				if got := mocks.count("aws:ec2/securityGroup:SecurityGroup"); got < 6 {
					t.Errorf("aws: expected at least 6 SecurityGroups, got %d", got)
				}
				return captured
			},
			expectAwsResources: true,
		},
		{
			name: "gcp",
			runner: func(t *testing.T) types.NetworkOutputs {
				mocks := &gcpTopologyMocks{}
				var captured types.NetworkOutputs
				err := pulumi.RunErr(func(ctx *pulumi.Context) error {
					cfg := kconfig.LoadConfig(ctx)
					out, err := gcpfacade.NewNetwork(ctx, cfg)
					if err != nil {
						return err
					}
					captured = out
					return nil
				}, pulumi.WithMocks("kaizen", "dev", mocks),
					configWithProvider("gcp"))
				if err != nil {
					t.Fatalf("gcp NewNetwork failed: %v", err)
				}
				if got := countByType(mocks.resources, "gcp:compute/network:Network"); got != 1 {
					t.Errorf("gcp: expected 1 Network, got %d", got)
				}
				if got := countByType(mocks.resources, "gcp:compute/firewall:Firewall"); got != 6 {
					t.Errorf("gcp: expected 6 Firewall rules, got %d", got)
				}
				if got := countByType(mocks.resources, "gcp:servicedirectory/namespace:Namespace"); got != 1 {
					t.Errorf("gcp: expected 1 Service Directory Namespace, got %d", got)
				}
				if got := countByType(mocks.resources, "gcp:vpcaccess/connector:Connector"); got != 1 {
					t.Errorf("gcp: expected 1 VPC Connector, got %d", got)
				}
				return captured
			},
			expectGcpResources: true,
		},
	}

	// Required keys mirror the AWS security-groups module exactly. The
	// outputs.go doc comment lists "ecs", "alb", "rds", "msk", "redis",
	// "schema-registry" but the actual AWS implementation returns "m4b" in
	// place of "schema-registry"; this test reflects the implementation.
	requiredKeys := []string{"alb", "ecs", "rds", "msk", "redis", "m4b"}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			out := tc.runner(t)
			for _, k := range requiredKeys {
				if _, ok := out.SecurityGroupIds[k]; !ok {
					t.Errorf("provider %s: missing required SecurityGroupIds key %q", tc.name, k)
				}
			}
		})
	}
}

func countByType(records []fsResource, typeToken string) int {
	n := 0
	for _, r := range records {
		if r.TypeToken == typeToken {
			n++
		}
	}
	return n
}

// configWithProvider returns the complete Pulumi stack config required by
// LoadConfig + the network modules, with cloudProvider set to the requested
// value. The helper mirrors fullstackConfig() exactly so LoadConfig's
// Require() calls don't panic on missing downstream fields.
func configWithProvider(provider string) pulumi.RunOption {
	return func(info *pulumi.RunInfo) {
		info.Config = map[string]string{
			"aws:region":                                     "us-east-1",
			"gcp:project":                                    "kaizen-test",
			"gcp:region":                                     "us-central1",
			"kaizen:vpcCidr":                                 "10.0.0.0/16",
			"kaizen:natGatewayCount":                         "1",
			"kaizen-experimentation:environment":             "dev",
			"kaizen-experimentation:vpcCidr":                 "10.0.0.0/16",
			"kaizen-experimentation:rdsInstanceClass":        "db.t3.medium",
			"kaizen-experimentation:rdsMultiAz":              "false",
			"kaizen-experimentation:mskBrokerCount":          "3",
			"kaizen-experimentation:mskInstanceType":         "kafka.m5.large",
			"kaizen-experimentation:redisNodeType":           "cache.t3.medium",
			"kaizen-experimentation:m4bInstanceType":         "c5.xlarge",
			"kaizen-experimentation:natGatewayCount":         "1",
			"kaizen-experimentation:wafEnabled":              "false",
			"kaizen-experimentation:fargateMinTasks":         "1",
			"kaizen-experimentation:cloudwatchRetentionDays": "7",
			"kaizen-experimentation:domain":                  "example.com",
			"kaizen-experimentation:projectName":             "kaizen-experimentation",
			"kaizen-experimentation:cloudProvider":           provider,
			"kafka:saslUsername":                             "kaizen-msk-user",
			"kafka:saslPassword":                             "test-password",
		}
	}
}
