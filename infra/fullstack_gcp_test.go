package main

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// gcpFullstackMocks records all GCP resources Deploy() registers under
// cloudProvider=gcp. Phase 1 only covers Artifact Registry — once compute,
// network, etc. land, this file should grow type-token cases for them.
type gcpFullstackMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *gcpFullstackMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
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
	case "gcp:artifactregistry/repository:Repository":
		// Echo the inputs the AR module reads back via apply().
		if v, ok := args.Inputs["repositoryId"]; ok {
			outputs["repositoryId"] = v
			outputs["name"] = v
		}
		if v, ok := args.Inputs["location"]; ok {
			outputs["location"] = v
		}
		if v, ok := args.Inputs["project"]; ok {
			outputs["project"] = v
		}

	// --- GCP network prerequisites (VPC, subnets, firewall, etc.) ---
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
	case "gcp:compute/firewall:Firewall":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/firewalls/" + args.Name)
	case "gcp:servicedirectory/namespace:Namespace":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/" + args.Name)
	case "gcp:vpcaccess/connector:Connector":
		outputs["selfLink"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/connectors/" + args.Name)

	// --- PSA (Private Service Access) ---
	case "gcp:compute/globalAddress:GlobalAddress":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:servicenetworking/connection:Connection":
		outputs["peering"] = resource.NewStringProperty("servicenetworking-googleapis-com")

	// --- Cloud SQL ---
	case "gcp:sql/databaseInstance:DatabaseInstance":
		outputs["name"] = resource.NewStringProperty(args.Name)
		outputs["privateIpAddress"] = resource.NewStringProperty("10.99.0.3")
		outputs["connectionName"] = resource.NewStringProperty("kaizen-test:us-central1:" + args.Name)
	case "gcp:sql/database:Database":
		outputs["name"] = resource.NewStringProperty(args.Name)

	// --- Memorystore Redis ---
	case "gcp:redis/instance:Instance":
		outputs["host"] = resource.NewStringProperty("10.99.1.1")
		outputs["port"] = resource.NewNumberProperty(6379)
		outputs["name"] = resource.NewStringProperty(args.Name)

	// --- GCE / M4b compute ---
	case "gcp:compute/instance:Instance":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/zones/us-central1-a/instances/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/instanceGroupManager:InstanceGroupManager":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/disk:Disk":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/zones/us-central1-a/disks/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:servicedirectory/service:Service":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen/services/" + args.Name)
	case "gcp:servicedirectory/endpoint:Endpoint":
		outputs["name"] = resource.NewStringProperty(args.Name)
	}
	return id, outputs, nil
}

func (m *gcpFullstackMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *gcpFullstackMocks) byType(t string) []fsResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []fsResource
	for _, r := range m.resources {
		if r.TypeToken == t {
			out = append(out, r)
		}
	}
	return out
}

// gcpFullstackConfig sets the minimal stack config that Pulumi.gcp-dev.yaml
// declares plus a CI push principal so the IAM bindings get exercised.
func gcpFullstackConfig() pulumi.RunOption {
	return func(info *pulumi.RunInfo) {
		info.Config = map[string]string{
			"gcp:project":                                 "kaizen-experimentation-dev",
			"gcp:region":                                  "us-central1",
			"kaizen-experimentation:environment":          "dev",
			"kaizen-experimentation:cloudProvider":        "gcp",
			"kaizen-experimentation:gcpProjectId":         "kaizen-experimentation-dev",
			"kaizen-experimentation:gcpRegion":            "us-central1",
			"kaizen-experimentation:gcpArLocation":        "us",
			"kaizen-experimentation:gcpCiPushPrincipal":   "serviceAccount:ci@kaizen-experimentation-dev.iam.gserviceaccount.com",
			"kaizen-experimentation:gcpRunPullPrincipals": "serviceAccount:run@kaizen-experimentation-dev.iam.gserviceaccount.com",
		}
	}
}

// TestFullStackDeploy_GCP runs Deploy() with cloudProvider=gcp and verifies
// the CICD slice landed cleanly. This is the unit-level proxy for the
// `pulumi preview --stack gcp-dev` acceptance criterion in issue #482 — if
// this test fails, preview will fail.
func TestFullStackDeploy_GCP(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		gcpFullstackConfig(),
	)
	if err != nil {
		t.Fatalf("Deploy(gcp) failed: %v", err)
	}
}

// TestFullStackResourceCounts_GCP validates that the expected number of
// Artifact Registry repos and IAM members were registered. The expected
// counts are kept in lockstep with pkg/gcp/cicd.{ServiceNames,UtilityImageNames}.
func TestFullStackResourceCounts_GCP(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		gcpFullstackConfig(),
	)
	if err != nil {
		t.Fatalf("Deploy(gcp) failed: %v", err)
	}

	// 9 services + 1 utility = 10 repos; identical to the ECR count.
	gotRepos := len(mocks.byType("gcp:artifactregistry/repository:Repository"))
	if gotRepos != 10 {
		t.Errorf("Artifact Registry repositories: got %d, want 10", gotRepos)
	}

	// One writer (push) + one reader (pull) per repo → 20 IAM members.
	gotIam := len(mocks.byType("gcp:artifactregistry/repositoryIamMember:RepositoryIamMember"))
	if gotIam != 20 {
		t.Errorf("Artifact Registry IAM members: got %d, want 20", gotIam)
	}

	// Sanity: nothing AWS-specific should be registered when on GCP.
	for _, r := range mocks.resources {
		if len(r.TypeToken) > 4 && r.TypeToken[:4] == "aws:" {
			t.Errorf("unexpected AWS resource registered under cloudProvider=gcp: %s", r.TypeToken)
		}
	}
}

// TestFullStackDeploy_GCP_RejectsMissingProject ensures Deploy() fails fast
// when the GCP project is not configured. Without this guard, AR would 400
// at apply time and waste an apply cycle.
func TestFullStackDeploy_GCP_RejectsMissingProject(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		func(info *pulumi.RunInfo) {
			info.Config = map[string]string{
				"kaizen-experimentation:environment":   "dev",
				"kaizen-experimentation:cloudProvider": "gcp",
				// no gcpProjectId
			}
		},
	)
	if err == nil {
		t.Fatal("expected error for missing gcpProjectId, got nil")
	}
}
