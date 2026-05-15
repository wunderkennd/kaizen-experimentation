package main

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/types"
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

// ---------------------------------------------------------------------------
// Issue #492 — M4a Analysis on Cloud Run
//
// M4a is a CPU-intensive batch Rust gRPC service (port 50053). These tests
// drive gcp.NewCompute directly with the same gcpFullstackMocks the
// Deploy()-level tests use (it already synthesizes the M4b GCE + Cloud Run +
// service-account outputs the factory chains on).
// ---------------------------------------------------------------------------

// gcpComputeInputs returns a representative set of upstream-stage outputs for
// driving gcp.NewCompute in isolation.
func gcpComputeInputs() (*kconfig.Config, types.NetworkOutputs, types.CICDOutputs, types.DatabaseOutputs, types.StreamingOutputs, types.SecretsOutputs, types.StorageOutputs, types.CacheOutputs) {
	cfg := &kconfig.Config{
		Project:      "kaizen",
		Environment:  "dev",
		Env:          kconfig.EnvDev,
		GCPProjectID: "kaizen-experimentation-dev",
		GCPRegion:    "us-central1",
	}
	netOut := types.NetworkOutputs{
		PrivateSubnetIds: pulumi.StringArray{
			pulumi.String("projects/kaizen-experimentation-dev/regions/us-central1/subnetworks/kaizen-dev-private"),
		}.ToStringArrayOutput(),
		ServiceDiscoveryId: pulumi.ID(
			"projects/kaizen-experimentation-dev/locations/us-central1/namespaces/kaizen-local").ToIDOutput(),
		VpcConnectorSelfLink: pulumi.String(
			"projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector").ToStringOutput(),
	}
	// gcp.NewCompute provisions every wired per-service Cloud Run service
	// in one call, so this fixture must satisfy each service's image lookup.
	cicdOut := types.CICDOutputs{
		RepositoryURLs: map[string]pulumi.StringOutput{
			"analysis":      pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-dev-analysis/analysis").ToStringOutput(),
			"orchestration": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/orchestration").ToStringOutput(),
			"ui":            pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/ui").ToStringOutput(),
			"assignment":    pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/assignment").ToStringOutput(),
			"metrics":       pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/metrics").ToStringOutput(),
			"flags":         pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/flags").ToStringOutput(),
			"pipeline":      pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/pipeline").ToStringOutput(),
		},
	}
	storageOut := types.StorageOutputs{
		DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
		DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
	}
	dbOut := types.DatabaseOutputs{
		Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
	}
	streamOut := types.StreamingOutputs{
		BootstrapBrokers: pulumi.String("seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092").ToStringOutput(),
	}
	secretsOut := types.SecretsOutputs{
		DatabaseSecretRef: pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-database").ToStringOutput(),
		KafkaSecretRef:    pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-kafka").ToStringOutput(),
		RedisSecretRef:    pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-redis").ToStringOutput(),
		AuthSecretRef:     pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-auth").ToStringOutput(),
	}
	cacheOut := types.CacheOutputs{
		Endpoint: pulumi.String("redis://10.99.1.1:6379").ToStringOutput(),
	}
	return cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut
}

func (m *gcpFullstackMocks) cloudRunByName(name string) (fsResource, bool) {
	for _, r := range m.byType("gcp:cloudrunv2/service:Service") {
		if v, ok := r.Inputs["name"]; ok && v.HasValue() && v.StringValue() == name {
			return r, true
		}
	}
	return fsResource{}, false
}

// Acceptance criterion 1: M4a appears in ComputeOutputs.ServiceEndpoints["m4a"].
func TestGCPCompute_M4aInServiceEndpoints(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	var out types.ComputeOutputs
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut := gcpComputeInputs()
		var e error
		out, e = gcp.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut)
		return e
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("gcp.NewCompute failed: %v", err)
	}
	if _, ok := out.ServiceEndpoints["m4a"]; !ok {
		t.Errorf("ComputeOutputs.ServiceEndpoints missing key %q; got keys %v",
			"m4a", keysOf(out.ServiceEndpoints))
	}
	if _, ok := out.ServiceArns["m4a"]; !ok {
		t.Errorf("ComputeOutputs.ServiceArns missing key %q", "m4a")
	}
}

func keysOf(m map[string]pulumi.StringOutput) []string {
	ks := make([]string, 0, len(m))
	for k := range m {
		ks = append(ks, k)
	}
	return ks
}

// Acceptance criterion 2: M4a Cloud Run service is the elevated CPU shape
// with an end-to-end gRPC health probe on 50053 (the deployable form of
// "health check returns 200").
func TestGCPCompute_M4aHealthProbeAndResources(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut := gcpComputeInputs()
		_, e := gcp.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut)
		return e
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("gcp.NewCompute failed: %v", err)
	}

	svc, ok := mocks.cloudRunByName("kaizen-dev-m4a-analysis")
	if !ok {
		t.Fatal("no Cloud Run service named kaizen-dev-m4a-analysis registered")
	}
	tmpl := svc.Inputs["template"].ObjectValue()
	container := tmpl["containers"].ArrayValue()[0].ObjectValue()

	res, ok := container["resources"]
	if !ok || !res.IsObject() {
		t.Fatal("M4a container.resources missing — not the elevated CPU shape")
	}
	limits := res.ObjectValue()["limits"].ObjectValue()
	if cpu := limits["cpu"]; !cpu.HasValue() || cpu.StringValue() == "" {
		t.Error("M4a resources.limits.cpu not set")
	}
	if mem := limits["memory"]; !mem.HasValue() || mem.StringValue() == "" {
		t.Error("M4a resources.limits.memory not set")
	}

	probe, ok := container["startupProbe"]
	if !ok || !probe.IsObject() {
		t.Fatal("M4a container.startupProbe missing — health check not verified end-to-end")
	}
	grpc, ok := probe.ObjectValue()["grpc"]
	if !ok || !grpc.IsObject() {
		t.Fatal("M4a startupProbe.grpc missing — M4a is a gRPC service")
	}
	if port := grpc.ObjectValue()["port"]; !port.HasValue() || port.NumberValue() != 50053 {
		t.Errorf("M4a startupProbe.grpc.port = %v, want 50053", grpc.ObjectValue()["port"])
	}
}

// Acceptance criterion 3: M4a can list & write objects in the data bucket
// (roles/storage.objectAdmin on kaizen-dev-data bound to the M4a runtime SA)
// and can read its DB credentials secret (roles/secretmanager.secretAccessor).
func TestGCPCompute_M4aDataBucketAndSecretIAM(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut := gcpComputeInputs()
		_, e := gcp.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut)
		return e
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("gcp.NewCompute failed: %v", err)
	}

	wantMember := "serviceAccount:dev-m4a-analysis-run@kaizen-experimentation-dev.iam.gserviceaccount.com"

	bucketBound := false
	for _, b := range mocks.byType("gcp:storage/bucketIAMMember:BucketIAMMember") {
		role := b.Inputs["role"]
		member := b.Inputs["member"]
		bucket := b.Inputs["bucket"]
		if role.HasValue() && role.StringValue() == "roles/storage.objectAdmin" &&
			member.HasValue() && member.StringValue() == wantMember &&
			bucket.HasValue() && bucket.StringValue() == "kaizen-dev-data" {
			bucketBound = true
		}
	}
	if !bucketBound {
		t.Error("no objectAdmin BucketIAMMember on kaizen-dev-data for the M4a runtime SA")
	}

	secretBound := false
	for _, s := range mocks.byType("gcp:secretmanager/secretIamMember:SecretIamMember") {
		role := s.Inputs["role"]
		member := s.Inputs["member"]
		sid := s.Inputs["secretId"]
		if role.HasValue() && role.StringValue() == "roles/secretmanager.secretAccessor" &&
			member.HasValue() && member.StringValue() == wantMember &&
			sid.HasValue() && strings.Contains(sid.StringValue(), "database") {
			secretBound = true
		}
	}
	if !secretBound {
		t.Error("no secretAccessor SecretIamMember on the database secret for the M4a runtime SA")
	}
}
