// Package test — topology test for issue #495, Deploy M7 Flags to Cloud Run.
//
// Exercises the gcp.NewCompute *facade* (the function Deploy() calls) so the
// assertions cover the real wiring, not just the lower-level
// compute.NewCloudRunService factory (already covered by
// gcp_compute_topology_test.go). Lives in package test so `just test-infra`
// (cd infra && go test ./pkg/... ./test/...) runs it in CI — the root infra
// package is intentionally not on that path.
//
// Acceptance criteria locked here (mock-verifiable subset):
//   - M7 appears in ComputeOutputs.ServiceEndpoints["m7"] and ServiceArns["m7"].
//   - min-instances = 1 on the m7-flags Cloud Run service (p99 < 5ms SLA;
//     spec Compute Model → Cold starts — same profile as M1).
//   - container port 50057 (M7 gRPC port; parity with the AWS ECS task def).
//   - a dedicated Workload Identity SA (dev-m7-flags-run).
//   - roles/secretmanager.secretAccessor on the database AND redis secrets
//     ("IAM scopes: read DB creds, read Redis auth").
//   - roles/cloudsql.client at project level ("connect to Cloud SQL").
//   - DATABASE_ENDPOINT/REDIS_ENDPOINT literal env vars + DATABASE_SECRET/
//     REDIS_SECRET secret-backed env vars.
//   - a Service Directory service+endpoint named m7-flags.
//
// Runtime acceptance ("health check returns 200 in a deployed dev stack",
// live Cloud SQL / Memorystore connectivity) is verified by the smoke load
// test (#500) against a real GCP project — it cannot be asserted under
// pulumi mocks. This test locks the topology those runtime checks depend on.
package test

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

// ---------------------------------------------------------------------------
// Mock monitor — covers every resource gcp.NewCompute registers: the M4b GCE
// slice, the Cloud Run canary, and the M7 Flags service.
// ---------------------------------------------------------------------------

type m7FacadeMocks struct {
	mu        sync.Mutex
	resources []m7Recorded
}

type m7Recorded struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *m7FacadeMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, m7Recorded{args.TypeToken, args.Name, args.Inputs})
	m.mu.Unlock()

	id := args.Name + "_id"
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	switch args.TypeToken {
	// ── Cloud Run service factory ────────────────────────────────────────
	case "gcp:serviceaccount/account:Account":
		accountID, project := "", ""
		if v, ok := args.Inputs["accountId"]; ok && v.HasValue() {
			accountID = v.StringValue()
		}
		if v, ok := args.Inputs["project"]; ok && v.HasValue() {
			project = v.StringValue()
		}
		email := accountID + "@" + project + ".iam.gserviceaccount.com"
		outputs["email"] = resource.NewStringProperty(email)
		outputs["name"] = resource.NewStringProperty("projects/" + project + "/serviceAccounts/" + email)
	case "gcp:cloudrunv2/service:Service":
		name, region := "", ""
		if v, ok := args.Inputs["name"]; ok && v.HasValue() {
			name = v.StringValue()
		}
		if v, ok := args.Inputs["location"]; ok && v.HasValue() {
			region = v.StringValue()
		}
		outputs["uri"] = resource.NewStringProperty("https://" + name + "-mock-" + region + ".a.run.app")
	case "gcp:servicedirectory/service:Service":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/services/" + args.Name)
	case "gcp:servicedirectory/endpoint:Endpoint":
		outputs["name"] = resource.NewStringProperty(args.Name)
		if v, ok := args.Inputs["address"]; ok {
			outputs["address"] = v
		}
		if v, ok := args.Inputs["port"]; ok {
			outputs["port"] = v
		}

	// ── M4b GCE slice (created by NewCompute before the Cloud Run block) ──
	case "gcp:compute/disk:Disk":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/zones/us-central1-a/disks/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/address:Address":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/addresses/" + args.Name)
		outputs["address"] = resource.NewStringProperty("10.0.16.42")
	case "gcp:compute/healthCheck:HealthCheck":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/healthChecks/" + args.Name)
	case "gcp:compute/instanceTemplate:InstanceTemplate":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/instanceTemplates/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/instanceGroupManager:InstanceGroupManager":
		outputs["name"] = resource.NewStringProperty(args.Name)
		outputs["instanceGroup"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/zones/us-central1-a/instanceGroups/" + args.Name)
	}
	return id, outputs, nil
}

func (m *m7FacadeMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *m7FacadeMocks) m7(typeToken string) []m7Recorded {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []m7Recorded
	for _, r := range m.resources {
		if r.TypeToken == typeToken && strings.Contains(r.Name, "m7-flags") {
			out = append(out, r)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Fixture: run gcp.NewCompute with mocked stage inputs.
// ---------------------------------------------------------------------------

// runM7Facade invokes gcp.NewCompute exactly as Deploy()'s gcp arm does and
// returns the recorded resources plus the ComputeOutputs it produced.
func runM7Facade(t *testing.T) (*m7FacadeMocks, types.ComputeOutputs) {
	t.Helper()
	mocks := &m7FacadeMocks{}
	var out types.ComputeOutputs
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &kconfig.Config{
			Project:      "kaizen",
			Environment:  "dev",
			Env:          kconfig.EnvDev,
			GCPProjectID: "kaizen-experimentation-dev",
			GCPRegion:    "us-central1",
		}
		netOut := types.NetworkOutputs{
			PrivateSubnetIds: pulumi.ToStringArray([]string{
				"https://www.googleapis.com/compute/v1/projects/p/regions/us-central1/subnetworks/private",
			}).ToStringArrayOutput(),
			ServiceDiscoveryId: pulumi.ID(
				"projects/kaizen-experimentation-dev/locations/us-central1/namespaces/kaizen-local",
			).ToIDOutput(),
			VpcConnectorSelfLink: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector",
			).ToStringOutput(),
		}
		cicdOut := types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"flags": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-flags").ToStringOutput(),
				// NewCompute provisions every wired per-service Cloud Run service
				// in one call, so this fixture must satisfy each service's image
				// lookup even when the test is scoped to M7.
				"orchestration": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/orchestration").ToStringOutput(),
				"ui": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/ui").ToStringOutput(),
				"analysis": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/analysis").ToStringOutput(),
				"assignment": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/assignment").ToStringOutput(),
				"metrics": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/metrics").ToStringOutput(),
				"pipeline": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/pipeline").ToStringOutput(),
				"management": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/management").ToStringOutput(),
			},
		}
		dbOut := types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
			Port:     pulumi.Int(5432).ToIntOutput(),
		}
		streamOut := types.StreamingOutputs{
			BootstrapBrokers: pulumi.String(
				"seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092").ToStringOutput(),
		}
		cacheOut := types.CacheOutputs{
			Endpoint: pulumi.String("redis://10.99.1.1:6379").ToStringOutput(),
		}
		storageOut := types.StorageOutputs{
			DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
			DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
		}
		// Bare `projects/<P>/secrets/<S>` paths — matches what
		// gcp.NewSecrets produces in production (see its contract note);
		// Cloud Run secretKeyRef and Secret Manager IAM bindings consume
		// this format directly. The version is supplied separately via the
		// Secrets[i].Version field.
		secretsOut := types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String(
				"projects/kaizen-experimentation-dev/secrets/kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef: pulumi.String(
				"projects/kaizen-experimentation-dev/secrets/kaizen-dev-kafka").ToStringOutput(),
			RedisSecretRef: pulumi.String(
				"projects/kaizen-experimentation-dev/secrets/kaizen-dev-redis").ToStringOutput(),
			AuthSecretRef: pulumi.String(
				"projects/kaizen-experimentation-dev/secrets/kaizen-dev-auth").ToStringOutput(),
		}

		var cerr error
		out, cerr = gcp.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut)
		return cerr
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("gcp.NewCompute failed: %v", err)
	}
	return mocks, out
}

// ---------------------------------------------------------------------------
// Acceptance criterion 1: M7 in ServiceEndpoints["m7"] / ServiceArns["m7"].
// ---------------------------------------------------------------------------

func TestM7FlagsInComputeOutputs(t *testing.T) {
	mocks, out := runM7Facade(t)

	// The map key must exist and carry a real Cloud Run URL output (not a
	// zero StringOutput). Presence of the key + the matching recorded
	// cloudrunv2 service together prove the endpoint points at M7.
	if _, ok := out.ServiceEndpoints["m7"]; !ok {
		t.Fatal("ComputeOutputs.ServiceEndpoints[\"m7\"] missing — M7 not wired into the compute facade")
	}
	if _, ok := out.ServiceArns["m7"]; !ok {
		t.Error("ComputeOutputs.ServiceArns[\"m7\"] missing — M7 service ID not exposed")
	}
	if got := len(mocks.m7("gcp:cloudrunv2/service:Service")); got != 1 {
		t.Errorf("expected exactly 1 recorded m7-flags Cloud Run service backing ServiceEndpoints[\"m7\"], got %d", got)
	}
}

// ---------------------------------------------------------------------------
// Acceptance criterion 2: min-instances = 1 (+ port 50057).
// ---------------------------------------------------------------------------

func TestM7FlagsMinInstancesAndPort(t *testing.T) {
	mocks, _ := runM7Facade(t)

	svcs := mocks.m7("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected exactly 1 m7-flags Cloud Run service, got %d", len(svcs))
	}
	tmpl := svcs[0].Inputs["template"]
	if !tmpl.IsObject() {
		t.Fatal("m7-flags service missing template")
	}
	scaling := tmpl.ObjectValue()["scaling"]
	if !scaling.IsObject() {
		t.Fatal("m7-flags template missing scaling")
	}
	min := scaling.ObjectValue()["minInstanceCount"]
	if !min.HasValue() || min.NumberValue() != 1 {
		t.Errorf("m7-flags minInstanceCount = %v, want 1 (p99 < 5ms SLA)", min)
	}

	containers := tmpl.ObjectValue()["containers"]
	if !containers.IsArray() || len(containers.ArrayValue()) != 1 {
		t.Fatal("m7-flags template missing single container")
	}
	ports := containers.ArrayValue()[0].ObjectValue()["ports"]
	cp := ports.ObjectValue()["containerPort"]
	if !cp.HasValue() || cp.NumberValue() != 50057 {
		t.Errorf("m7-flags containerPort = %v, want 50057", cp)
	}
}

// ---------------------------------------------------------------------------
// Acceptance criterion 4 (topology): WI SA + IAM scopes for DB/Redis.
// ---------------------------------------------------------------------------

func TestM7FlagsWorkloadIdentityAndIAMScopes(t *testing.T) {
	mocks, _ := runM7Facade(t)

	sas := mocks.m7("gcp:serviceaccount/account:Account")
	if len(sas) != 1 {
		t.Fatalf("expected 1 m7-flags service account, got %d", len(sas))
	}
	if v := sas[0].Inputs["accountId"]; !v.HasValue() || v.StringValue() != "dev-m7-flags-run" {
		t.Errorf("m7-flags SA accountId = %v, want dev-m7-flags-run", sas[0].Inputs["accountId"])
	}

	secretBindings := mocks.m7("gcp:secretmanager/secretIamMember:SecretIamMember")
	if len(secretBindings) != 2 {
		t.Fatalf("expected 2 m7-flags secret accessor bindings (db + redis), got %d", len(secretBindings))
	}
	for _, b := range secretBindings {
		if r := b.Inputs["role"]; !r.HasValue() || r.StringValue() != "roles/secretmanager.secretAccessor" {
			t.Errorf("m7-flags secret binding %s role = %v, want roles/secretmanager.secretAccessor",
				b.Name, b.Inputs["role"])
		}
	}

	projBindings := mocks.m7("gcp:projects/iAMMember:IAMMember")
	sawCloudSQL := false
	for _, b := range projBindings {
		if r := b.Inputs["role"]; r.HasValue() && r.StringValue() == "roles/cloudsql.client" {
			sawCloudSQL = true
		}
	}
	if !sawCloudSQL {
		t.Errorf("m7-flags missing project IAM binding roles/cloudsql.client (Cloud SQL connectivity); %d bindings seen",
			len(projBindings))
	}
}

// ---------------------------------------------------------------------------
// Env + secret wiring and Service Directory registration.
// ---------------------------------------------------------------------------

func TestM7FlagsEnvSecretAndServiceDirectory(t *testing.T) {
	mocks, _ := runM7Facade(t)

	svcs := mocks.m7("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 m7-flags Cloud Run service, got %d", len(svcs))
	}
	container := svcs[0].Inputs["template"].ObjectValue()["containers"].ArrayValue()[0]
	envs := container.ObjectValue()["envs"]
	if !envs.IsArray() {
		t.Fatal("m7-flags container missing envs array")
	}
	literal, secret := map[string]bool{}, map[string]bool{}
	for _, e := range envs.ArrayValue() {
		eo := e.ObjectValue()
		name := eo["name"].StringValue()
		if vs, ok := eo["valueSource"]; ok && vs.IsObject() {
			secret[name] = true
		} else {
			literal[name] = true
		}
	}
	for _, want := range []string{"DATABASE_ENDPOINT", "REDIS_ENDPOINT"} {
		if !literal[want] {
			t.Errorf("m7-flags missing literal env var %s", want)
		}
	}
	for _, want := range []string{"DATABASE_SECRET", "REDIS_SECRET"} {
		if !secret[want] {
			t.Errorf("m7-flags missing secret-backed env var %s", want)
		}
	}

	sdSvcs := mocks.m7("gcp:servicedirectory/service:Service")
	if len(sdSvcs) != 1 {
		t.Fatalf("expected 1 m7-flags SD service, got %d", len(sdSvcs))
	}
	if v := sdSvcs[0].Inputs["serviceId"]; !v.HasValue() || v.StringValue() != "m7-flags" {
		t.Errorf("m7-flags SD serviceId = %v, want m7-flags", sdSvcs[0].Inputs["serviceId"])
	}
	if len(mocks.m7("gcp:servicedirectory/endpoint:Endpoint")) != 1 {
		t.Errorf("expected 1 m7-flags SD endpoint, got %d",
			len(mocks.m7("gcp:servicedirectory/endpoint:Endpoint")))
	}
}
