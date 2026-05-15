package main

import (
	"strings"
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
	case "gcp:compute/instanceTemplate:InstanceTemplate":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/instanceTemplates/" + args.Name)
	case "gcp:compute/healthCheck:HealthCheck":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/healthChecks/" + args.Name)
	case "gcp:compute/address:Address":
		outputs["address"] = resource.NewStringProperty("10.0.16.20")
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/addresses/" + args.Name)

	// --- Stage 4 streaming: Redpanda Cloud (TF-bridge type tokens) ---
	// Mirrors infra/test/testutil_test.go so the Stage 4 arm now wired into
	// the GCP path (#490) resolves bootstrap brokers + schema registry.
	case "redpanda:index/resourceGroup:ResourceGroup":
		outputs["id"] = resource.NewStringProperty("rg_" + args.Name)
	case "redpanda:index/network:Network":
		outputs["id"] = resource.NewStringProperty("net_" + args.Name)
	case "redpanda:index/cluster:Cluster":
		outputs["bootstrapBrokers"] = resource.NewStringProperty(
			"seed-0." + args.Name + ".fmc.prd.cloud.redpanda.com:9092," +
				"seed-1." + args.Name + ".fmc.prd.cloud.redpanda.com:9092")
		outputs["schemaRegistryUrl"] = resource.NewStringProperty(
			"https://schema-registry-" + args.Name + ".fmc.prd.cloud.redpanda.com:30081")
		outputs["clusterApiUrl"] = resource.NewStringProperty(
			"https://api-" + args.Name + ".fmc.prd.cloud.redpanda.com:9644")
	case "redpanda:index/user:User",
		"redpanda:index/acl:Acl",
		"pulumi:providers:kafka",
		"kafka:index/topic:Topic":
		// Inputs already copied to outputs; no extra computed fields needed.

	// --- Stage 4 secrets: Secret Manager ---
	case "gcp:secretmanager/secret:Secret":
		secretID := args.Name
		if v, ok := args.Inputs["secretId"]; ok && v.HasValue() {
			secretID = v.StringValue()
		}
		outputs["name"] = resource.NewStringProperty(
			"projects/kaizen-experimentation-dev/secrets/" + secretID)
		outputs["secretId"] = resource.NewStringProperty(secretID)
	case "gcp:secretmanager/secretVersion:SecretVersion",
		"gcp:secretmanager/secretIamMember:SecretIamMember":
		// Default copy is sufficient (no computed fields read downstream).

	// --- Stage 5 compute: per-service Cloud Run runtime identity + service ---
	case "gcp:serviceaccount/account:Account":
		accountID := ""
		if v, ok := args.Inputs["accountId"]; ok && v.HasValue() {
			accountID = v.StringValue()
		}
		project := ""
		if v, ok := args.Inputs["project"]; ok && v.HasValue() {
			project = v.StringValue()
		}
		email := accountID + "@" + project + ".iam.gserviceaccount.com"
		outputs["email"] = resource.NewStringProperty(email)
		outputs["name"] = resource.NewStringProperty(
			"projects/" + project + "/serviceAccounts/" + email)
		outputs["uniqueId"] = resource.NewStringProperty("100000000000000000001")
	case "gcp:cloudrunv2/service:Service":
		name := args.Name
		if v, ok := args.Inputs["name"]; ok && v.HasValue() {
			name = v.StringValue()
		}
		region := "us-central1"
		if v, ok := args.Inputs["location"]; ok && v.HasValue() {
			region = v.StringValue()
		}
		outputs["uri"] = resource.NewStringProperty(
			"https://" + name + "-mock-" + region + ".a.run.app")
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
// declares plus a CI push principal so the IAM bindings get exercised. The
// redpanda:* keys mirror Pulumi.gcp-dev.yaml so the Stage 4 streaming arm
// (now wired into the GCP path by #490) reads valid config; kafkaPassword is
// supplied as a plain value because RequireSecret reads test config the same
// way Require does.
func gcpFullstackConfig() pulumi.RunOption {
	return func(info *pulumi.RunInfo) {
		info.Config = map[string]string{
			"gcp:project":                                 "kaizen-experimentation-dev",
			"gcp:region":                                  "us-central1",
			"kaizen-experimentation:environment":          "dev",
			"kaizen-experimentation:cloudProvider":        "gcp",
			"kaizen-experimentation:streamingProvider":    "redpanda",
			"kaizen-experimentation:gcpProjectId":         "kaizen-experimentation-dev",
			"kaizen-experimentation:gcpRegion":            "us-central1",
			"kaizen-experimentation:gcpArLocation":        "us",
			"kaizen-experimentation:gcpCiPushPrincipal":   "serviceAccount:ci@kaizen-experimentation-dev.iam.gserviceaccount.com",
			"kaizen-experimentation:gcpRunPullPrincipals": "serviceAccount:run@kaizen-experimentation-dev.iam.gserviceaccount.com",
			"redpanda:cloudProvider":                      "gcp",
			"redpanda:region":                             "us-central1",
			"redpanda:zones":                              "us-central1-a,us-central1-b,us-central1-c",
			"redpanda:throughputTier":                     "tier-1-gcp-v2",
			"redpanda:clusterType":                        "dedicated",
			"redpanda:connectionType":                     "private",
			"redpanda:kafkaUsername":                      "kaizen-redpanda-user",
			"redpanda:kafkaPassword":                      "test-kafka-password",
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
//
// The config includes streamingProvider=redpanda (+ the minimum Redpanda
// keys Stage 4a needs) so the streaming stage passes and Deploy() actually
// reaches the gcpProjectId check in gcp.NewCICD (Stage 4c). Without these
// keys the streaming switch rejects "msk" first, masking the real
// validation this test is meant to pin.
func TestFullStackDeploy_GCP_RejectsMissingProject(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		func(info *pulumi.RunInfo) {
			info.Config = map[string]string{
				"kaizen-experimentation:environment":       "dev",
				"kaizen-experimentation:cloudProvider":     "gcp",
				"kaizen-experimentation:streamingProvider": "redpanda",
				// Minimum Redpanda keys so Stage 4a (streaming) passes.
				"redpanda:region":         "us-central1",
				"redpanda:zones":          "us-central1-a,us-central1-b,us-central1-c",
				"redpanda:throughputTier": "tier-1-gcp-v2",
				"redpanda:kafkaUsername":  "kaizen-redpanda-user",
				"redpanda:kafkaPassword":  "test-kafka-password",
				// no gcpProjectId — this is what we are testing
			}
		},
	)
	if err == nil {
		t.Fatal("expected error for missing gcpProjectId, got nil")
	}
	if !strings.Contains(err.Error(), "gcpProjectId") && !strings.Contains(err.Error(), "GCPProjectID") {
		t.Fatalf("expected gcpProjectId validation error, got: %v", err)
	}
}
