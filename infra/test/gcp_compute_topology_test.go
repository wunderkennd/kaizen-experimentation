// Package test — topology test for the GCP Cloud Run service factory
// (pkg/gcp/compute.NewCloudRunService).
//
// Closes the topology coverage gap called out in the multi-cloud spec
// (Testing Strategy → Gap Mitigations #2): every Cloud Run service must
// have a Workload Identity service account binding with the expected
// scopes, and every Cloud Run service must register a Service Directory
// endpoint so peer services can resolve it.
//
// The test runs the factory under pulumi.WithMocks so no GCP credentials
// or network calls are required. A local mock monitor records every
// resource the factory registers; the assertions inspect that recording.
package test

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	gcpcompute "github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// ---------------------------------------------------------------------------
// Mock monitor
// ---------------------------------------------------------------------------

// computeMocks records every resource registration the factory emits and
// enriches outputs for the GCP type tokens NewCloudRunService touches.
//
// Kept local (not on universalMocks) because universalMocks is AWS-shaped;
// adding GCP cases there would couple two unrelated test stacks.
type computeMocks struct {
	mu        sync.Mutex
	resources []recordedComputeResource
}

type recordedComputeResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *computeMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, recordedComputeResource{
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
	case "gcp:serviceaccount/account:Account":
		// The factory chains saMember from sa.Email; without an email
		// output, downstream IAM bindings would receive
		// "serviceAccount:" with no local part.
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
		// Cloud Run synthesizes the runtime URL after deployment;
		// echo a deterministic stand-in so the SD endpoint stripScheme
		// + ApplyT chain has a string to work on.
		name := ""
		if v, ok := args.Inputs["name"]; ok && v.HasValue() {
			name = v.StringValue()
		}
		region := ""
		if v, ok := args.Inputs["location"]; ok && v.HasValue() {
			region = v.StringValue()
		}
		outputs["uri"] = resource.NewStringProperty(
			"https://" + name + "-mock-" + region + ".a.run.app")

	case "gcp:servicedirectory/service:Service":
		// Echo serviceId so test assertions can match on it.
		if v, ok := args.Inputs["serviceId"]; ok {
			outputs["serviceId"] = v
		}
		if v, ok := args.Inputs["namespace"]; ok {
			outputs["namespace"] = v
		}
	}

	return id, outputs, nil
}

func (m *computeMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *computeMocks) byType(typeToken string) []recordedComputeResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []recordedComputeResource
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			out = append(out, r)
		}
	}
	return out
}

func (m *computeMocks) count(typeToken string) int {
	return len(m.byType(typeToken))
}

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

// runComputeFactory exercises NewCloudRunService against a representative
// options bundle (one of each binding type) so a single mocked run
// exercises every factory code path.
func runComputeFactory(t *testing.T, name string, opts *gcpcompute.Options) *computeMocks {
	t.Helper()
	mocks := &computeMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &kconfig.Config{
			Project:      "kaizen",
			Environment:  "dev",
			Env:          kconfig.EnvDev,
			GCPProjectID: "kaizen-experimentation-dev",
			GCPRegion:    "us-central1",
		}
		inputs := &gcpcompute.Inputs{
			Project: "kaizen-experimentation-dev",
			Region:  "us-central1",
			VpcConnectorSelfLink: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector",
			).ToStringOutput(),
			ServiceDirectoryNamespaceID: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/namespaces/kaizen-local",
			).ToStringOutput(),
		}

		_, err := gcpcompute.NewCloudRunService(ctx, cfg, inputs, name, opts)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewCloudRunService failed: %v", err)
	}
	return mocks
}

// representativeOpts returns an Options struct that exercises every
// factory side-effect at least once: one project role, one secret,
// one bucket, two literal env vars.
func representativeOpts() *gcpcompute.Options {
	return &gcpcompute.Options{
		Image:         pulumi.String("us-central1-docker.pkg.dev/kaizen-experimentation-dev/kaizen/m1-assignment:latest"),
		ContainerPort: 50051,
		MinInstances:  1, // M1 SLA — verifies override works.
		MaxInstances:  20,
		EnvVars: []gcpcompute.EnvVar{
			{Name: "RUST_LOG", Value: pulumi.String("info")},
			{Name: "ENVIRONMENT", Value: pulumi.String("dev")},
		},
		Secrets: []gcpcompute.SecretEnv{
			{
				EnvName:  "DATABASE_SECRET",
				SecretID: pulumi.String("kaizen-dev-database"),
				Version:  "latest",
			},
		},
		Buckets: []pulumi.StringInput{
			pulumi.String("kaizen-dev-data"),
		},
		ProjectRoles: []string{
			"roles/cloudsql.client",
		},
	}
}

// ---------------------------------------------------------------------------
// Gap mitigation #2: Workload Identity binding exists with expected scopes
// ---------------------------------------------------------------------------

// TestCloudRunServiceHasWorkloadIdentitySA asserts each Cloud Run service
// gets exactly one runtime SA, and that SA's email is set on the Cloud
// Run revision template.serviceAccount field. This locks the WI invariant
// at test time so a future refactor cannot silently demote the service to
// the project-default SA (which would re-introduce the IAM-binding-drift
// risk the spec calls out).
func TestCloudRunServiceHasWorkloadIdentitySA(t *testing.T) {
	mocks := runComputeFactory(t, "m1-assignment", representativeOpts())

	sas := mocks.byType("gcp:serviceaccount/account:Account")
	if len(sas) != 1 {
		t.Fatalf("expected 1 service account, got %d", len(sas))
	}

	saAccountID, ok := sas[0].Inputs["accountId"]
	if !ok || !saAccountID.HasValue() {
		t.Fatal("service account missing accountId input")
	}
	if saAccountID.StringValue() != "dev-m1-assignment-run" {
		t.Errorf("accountId = %q, want %q", saAccountID.StringValue(), "dev-m1-assignment-run")
	}

	// Cloud Run service must reference the SA on its revision template.
	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	template, ok := runSvcs[0].Inputs["template"]
	if !ok || !template.IsObject() {
		t.Fatal("Cloud Run service missing template input")
	}
	templateObj := template.ObjectValue()
	saField, ok := templateObj["serviceAccount"]
	if !ok || !saField.HasValue() {
		t.Fatal("Cloud Run template missing serviceAccount field — would default to project SA, violating WI invariant")
	}
	wantSA := "dev-m1-assignment-run@kaizen-experimentation-dev.iam.gserviceaccount.com"
	if saField.StringValue() != wantSA {
		t.Errorf("template.serviceAccount = %q, want %q", saField.StringValue(), wantSA)
	}
}

// TestCloudRunServiceWorkloadIdentityScopes asserts every IAM binding
// the factory creates references the same per-service SA as a member.
// This prevents the "right binding, wrong identity" failure mode where
// the secret accessor role would land on the default SA but the
// container would run as the per-service SA (or vice versa).
func TestCloudRunServiceWorkloadIdentityScopes(t *testing.T) {
	mocks := runComputeFactory(t, "m1-assignment", representativeOpts())

	wantMember := "serviceAccount:dev-m1-assignment-run@kaizen-experimentation-dev.iam.gserviceaccount.com"

	cases := []struct {
		typeToken string
		minCount  int
		role      string
	}{
		{"gcp:projects/iAMMember:IAMMember", 1, "roles/cloudsql.client"},
		{"gcp:secretmanager/secretIamMember:SecretIamMember", 1, "roles/secretmanager.secretAccessor"},
		{"gcp:storage/bucketIAMMember:BucketIAMMember", 1, "roles/storage.objectAdmin"},
	}

	for _, tc := range cases {
		t.Run(tc.typeToken, func(t *testing.T) {
			members := mocks.byType(tc.typeToken)
			if len(members) < tc.minCount {
				t.Fatalf("%s: got %d, want >=%d", tc.typeToken, len(members), tc.minCount)
			}
			for _, m := range members {
				memberVal, ok := m.Inputs["member"]
				if !ok || !memberVal.HasValue() {
					t.Errorf("%s/%s: missing member input", tc.typeToken, m.Name)
					continue
				}
				if memberVal.StringValue() != wantMember {
					t.Errorf("%s/%s: member = %q, want %q",
						tc.typeToken, m.Name, memberVal.StringValue(), wantMember)
				}
				roleVal, ok := m.Inputs["role"]
				if !ok || !roleVal.HasValue() {
					t.Errorf("%s/%s: missing role input", tc.typeToken, m.Name)
					continue
				}
				if roleVal.StringValue() != tc.role {
					t.Errorf("%s/%s: role = %q, want %q",
						tc.typeToken, m.Name, roleVal.StringValue(), tc.role)
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Acceptance criterion 3: Service Directory endpoint registration verified
// ---------------------------------------------------------------------------

// TestCloudRunServiceRegistersServiceDirectory asserts that for every
// Cloud Run service the factory creates, the matching Service Directory
// service + endpoint pair is registered under the namespace that came in
// via inputs.ServiceDirectoryNamespaceID.
func TestCloudRunServiceRegistersServiceDirectory(t *testing.T) {
	mocks := runComputeFactory(t, "m1-assignment", representativeOpts())

	sdSvcs := mocks.byType("gcp:servicedirectory/service:Service")
	if len(sdSvcs) != 1 {
		t.Fatalf("expected 1 Service Directory service, got %d", len(sdSvcs))
	}
	if v, ok := sdSvcs[0].Inputs["serviceId"]; !ok || v.StringValue() != "m1-assignment" {
		t.Errorf("SD serviceId = %v, want %q", sdSvcs[0].Inputs["serviceId"], "m1-assignment")
	}
	if v, ok := sdSvcs[0].Inputs["namespace"]; !ok || !strings.HasSuffix(v.StringValue(), "/namespaces/kaizen-local") {
		t.Errorf("SD namespace = %v, want a path ending in /namespaces/kaizen-local", sdSvcs[0].Inputs["namespace"])
	}

	sdEndpoints := mocks.byType("gcp:servicedirectory/endpoint:Endpoint")
	if len(sdEndpoints) != 1 {
		t.Fatalf("expected 1 Service Directory endpoint, got %d", len(sdEndpoints))
	}
	if v, ok := sdEndpoints[0].Inputs["endpointId"]; !ok || v.StringValue() != "primary" {
		t.Errorf("SD endpointId = %v, want %q", sdEndpoints[0].Inputs["endpointId"], "primary")
	}
	if v, ok := sdEndpoints[0].Inputs["port"]; !ok || v.NumberValue() != 443 {
		t.Errorf("SD port = %v, want 443", sdEndpoints[0].Inputs["port"])
	}
}

// ---------------------------------------------------------------------------
// VPC connector wiring — feeds into the same gap mitigation
// ---------------------------------------------------------------------------

// TestCloudRunServiceWiresVpcConnector asserts the Cloud Run service
// references the connector self-link from inputs.VpcConnectorSelfLink.
// Without this, Cloud Run egress would bypass the VPC and Cloud SQL /
// Memorystore calls would 503 at runtime.
func TestCloudRunServiceWiresVpcConnector(t *testing.T) {
	mocks := runComputeFactory(t, "m1-assignment", representativeOpts())

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	template, ok := runSvcs[0].Inputs["template"]
	if !ok || !template.IsObject() {
		t.Fatal("template missing")
	}
	vpcAccess, ok := template.ObjectValue()["vpcAccess"]
	if !ok || !vpcAccess.IsObject() {
		t.Fatal("template.vpcAccess missing — Cloud Run will not use the connector")
	}
	connector, ok := vpcAccess.ObjectValue()["connector"]
	if !ok || !connector.HasValue() {
		t.Fatal("template.vpcAccess.connector missing")
	}
	wantConnector := "projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector"
	if connector.StringValue() != wantConnector {
		t.Errorf("connector = %q, want %q", connector.StringValue(), wantConnector)
	}
}

// ---------------------------------------------------------------------------
// Default min-instances behavior + per-service override
// ---------------------------------------------------------------------------

// TestCloudRunServiceMinInstancesDefault asserts the factory defaults to
// MinInstances=0 when callers do not opt in. Codifies the spec's "min-
// instances=0 by default" Cloud Run cost-control invariant.
func TestCloudRunServiceMinInstancesDefault(t *testing.T) {
	opts := representativeOpts()
	opts.MinInstances = 0 // explicit zero — most services
	mocks := runComputeFactory(t, "m3-metrics", opts)

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	scaling := runSvcs[0].Inputs["template"].ObjectValue()["scaling"]
	if !scaling.IsObject() {
		t.Fatal("template.scaling missing")
	}
	min, ok := scaling.ObjectValue()["minInstanceCount"]
	if !ok || !min.HasValue() {
		t.Fatal("scaling.minInstanceCount missing")
	}
	if min.NumberValue() != 0 {
		t.Errorf("default minInstanceCount = %v, want 0", min.NumberValue())
	}
}

// TestCloudRunServiceMinInstancesOverride asserts that M1/M7-style
// overrides (set MinInstances=1 to hold the p99 < 5ms SLA per the spec)
// land on the Cloud Run scaling block.
func TestCloudRunServiceMinInstancesOverride(t *testing.T) {
	opts := representativeOpts() // sets MinInstances: 1
	mocks := runComputeFactory(t, "m1-assignment", opts)

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	scaling := runSvcs[0].Inputs["template"].ObjectValue()["scaling"]
	if !scaling.IsObject() {
		t.Fatal("scaling missing")
	}
	min, ok := scaling.ObjectValue()["minInstanceCount"]
	if !ok || min.NumberValue() != 1 {
		t.Errorf("MinInstances=1 override did not propagate: got %v", min)
	}
	max, ok := scaling.ObjectValue()["maxInstanceCount"]
	if !ok || max.NumberValue() != 20 {
		t.Errorf("MaxInstances=20 override did not propagate: got %v", max)
	}
}

// ---------------------------------------------------------------------------
// Resource limits + health probe (issue #492 — M4a Analysis is a
// CPU-intensive batch Rust gRPC service that needs elevated CPU/memory and
// an end-to-end-verifiable health check)
// ---------------------------------------------------------------------------

// firstContainer pulls template.containers[0] out of a recorded Cloud Run
// service so the resource/probe assertions stay readable.
func firstContainer(t *testing.T, svc recordedComputeResource) resource.PropertyMap {
	t.Helper()
	template, ok := svc.Inputs["template"]
	if !ok || !template.IsObject() {
		t.Fatal("Cloud Run service missing template")
	}
	containers, ok := template.ObjectValue()["containers"]
	if !ok || !containers.IsArray() {
		t.Fatal("template.containers missing or not an array")
	}
	arr := containers.ArrayValue()
	if len(arr) != 1 {
		t.Fatalf("expected exactly 1 container, got %d", len(arr))
	}
	if !arr[0].IsObject() {
		t.Fatal("template.containers[0] is not an object")
	}
	return arr[0].ObjectValue()
}

// TestCloudRunServiceResourceLimits asserts CPULimit/MemoryLimit propagate
// to template.containers[0].resources.limits. M4a is "CPU-intensive batch"
// per the multi-cloud spec Compute Model, so the factory must let callers
// raise the per-container CPU/memory ceiling above the Cloud Run default.
func TestCloudRunServiceResourceLimits(t *testing.T) {
	opts := representativeOpts()
	opts.CPULimit = "2"
	opts.MemoryLimit = "4Gi"
	mocks := runComputeFactory(t, "m4a-analysis", opts)

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	container := firstContainer(t, runSvcs[0])

	res, ok := container["resources"]
	if !ok || !res.IsObject() {
		t.Fatal("container.resources missing — CPU/memory ceiling not applied")
	}
	limits, ok := res.ObjectValue()["limits"]
	if !ok || !limits.IsObject() {
		t.Fatal("container.resources.limits missing")
	}
	limitsObj := limits.ObjectValue()
	if cpu, ok := limitsObj["cpu"]; !ok || cpu.StringValue() != "2" {
		t.Errorf("resources.limits.cpu = %v, want \"2\"", limitsObj["cpu"])
	}
	if mem, ok := limitsObj["memory"]; !ok || mem.StringValue() != "4Gi" {
		t.Errorf("resources.limits.memory = %v, want \"4Gi\"", limitsObj["memory"])
	}
	// cpuIdle must be explicitly set when resources is set, otherwise Cloud
	// Run flips request-scoped CPU billing semantics (SDK doc note on
	// ServiceTemplateContainerResourcesArgs.CpuIdle).
	if idle, ok := res.ObjectValue()["cpuIdle"]; !ok || !idle.HasValue() {
		t.Error("container.resources.cpuIdle must be set explicitly when resources is set")
	}
}

// TestCloudRunServiceOmitsResourcesByDefault locks the cost-safety
// invariant: callers that don't ask for elevated limits get the Cloud Run
// platform default (no resources block), not a hard-coded ceiling.
func TestCloudRunServiceOmitsResourcesByDefault(t *testing.T) {
	opts := representativeOpts() // no CPULimit / MemoryLimit
	mocks := runComputeFactory(t, "m2-pipeline", opts)

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	container := firstContainer(t, runSvcs[0])
	if res, ok := container["resources"]; ok && res.IsObject() {
		if _, hasLimits := res.ObjectValue()["limits"]; hasLimits {
			t.Error("resources.limits set without CPULimit/MemoryLimit — must default to Cloud Run platform sizing")
		}
	}
}

// TestCloudRunServiceGrpcStartupProbe asserts a gRPC HealthCheck propagates
// to template.containers[0].startupProbe.grpc on the configured port. M4a
// Analysis exposes the standard gRPC Health Checking Protocol on 50053
// (AWS parity: grpc_health_probe -addr=:50053); this is the end-to-end
// "health check returns 200" acceptance criterion in mock form.
func TestCloudRunServiceGrpcStartupProbe(t *testing.T) {
	opts := representativeOpts()
	opts.HealthCheck = &gcpcompute.HealthProbe{
		Type:                "grpc",
		Port:                50053,
		InitialDelaySeconds: 10,
		PeriodSeconds:       10,
		FailureThreshold:    6,
	}
	mocks := runComputeFactory(t, "m4a-analysis", opts)

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	container := firstContainer(t, runSvcs[0])

	probe, ok := container["startupProbe"]
	if !ok || !probe.IsObject() {
		t.Fatal("container.startupProbe missing — health check not wired")
	}
	grpc, ok := probe.ObjectValue()["grpc"]
	if !ok || !grpc.IsObject() {
		t.Fatal("startupProbe.grpc missing — gRPC health probe not configured")
	}
	port, ok := grpc.ObjectValue()["port"]
	if !ok || port.NumberValue() != 50053 {
		t.Errorf("startupProbe.grpc.port = %v, want 50053", grpc.ObjectValue()["port"])
	}
	if ft, ok := probe.ObjectValue()["failureThreshold"]; !ok || ft.NumberValue() != 6 {
		t.Errorf("startupProbe.failureThreshold = %v, want 6", probe.ObjectValue()["failureThreshold"])
	}
}

// TestCloudRunServiceHttpStartupProbe asserts the factory also supports an
// HTTP probe (M3/M5/M6 expose wget-able /healthz on AWS) so the same
// helper covers every Kaizen service's health-check style.
func TestCloudRunServiceHttpStartupProbe(t *testing.T) {
	opts := representativeOpts()
	opts.HealthCheck = &gcpcompute.HealthProbe{
		Type: "http",
		Path: "/healthz",
		Port: 50056,
	}
	mocks := runComputeFactory(t, "m3-metrics", opts)

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(runSvcs))
	}
	container := firstContainer(t, runSvcs[0])

	probe, ok := container["startupProbe"]
	if !ok || !probe.IsObject() {
		t.Fatal("container.startupProbe missing")
	}
	httpGet, ok := probe.ObjectValue()["httpGet"]
	if !ok || !httpGet.IsObject() {
		t.Fatal("startupProbe.httpGet missing")
	}
	if path, ok := httpGet.ObjectValue()["path"]; !ok || path.StringValue() != "/healthz" {
		t.Errorf("startupProbe.httpGet.path = %v, want /healthz", httpGet.ObjectValue()["path"])
	}
}

// ---------------------------------------------------------------------------
// Multi-service topology assertion (the per-service invariant scaled up)
// ---------------------------------------------------------------------------

// TestEveryCloudRunServiceHasWIBinding runs the factory across a small
// fleet of services and asserts the spec gap mitigation #2 invariant
// holds for each: every Cloud Run service in the recorded stack has a
// matching Workload Identity service account binding.
//
// This is the "for each Cloud Run service in the test stack" form of the
// acceptance criterion — beyond the single-service tests above.
func TestEveryCloudRunServiceHasWIBinding(t *testing.T) {
	mocks := &computeMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &kconfig.Config{
			Project:      "kaizen",
			Environment:  "dev",
			Env:          kconfig.EnvDev,
			GCPProjectID: "kaizen-experimentation-dev",
			GCPRegion:    "us-central1",
		}
		inputs := &gcpcompute.Inputs{
			Project: "kaizen-experimentation-dev",
			Region:  "us-central1",
			VpcConnectorSelfLink: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector",
			).ToStringOutput(),
			ServiceDirectoryNamespaceID: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/namespaces/kaizen-local",
			).ToStringOutput(),
		}
		// A small fleet that exercises both the default (min=0) and
		// override (min=1) paths the spec calls out.
		fleet := []struct {
			name string
			min  int
		}{
			{"m1-assignment", 1},
			{"m2-pipeline", 0},
			{"m7-flags", 1},
		}
		for _, svc := range fleet {
			_, err := gcpcompute.NewCloudRunService(ctx, cfg, inputs, svc.name,
				&gcpcompute.Options{
					Image:         pulumi.String("placeholder/" + svc.name + ":latest"),
					ContainerPort: 8080,
					MinInstances:  svc.min,
				})
			if err != nil {
				return err
			}
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("multi-service factory run failed: %v", err)
	}

	runSvcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(runSvcs) != 3 {
		t.Fatalf("Cloud Run services: got %d, want 3", len(runSvcs))
	}

	// One SA per service — that's the WI invariant.
	if got := mocks.count("gcp:serviceaccount/account:Account"); got != 3 {
		t.Errorf("service accounts: got %d, want 3 (one per Cloud Run service)", got)
	}

	// One SD service + endpoint per Cloud Run service.
	if got := mocks.count("gcp:servicedirectory/service:Service"); got != 3 {
		t.Errorf("Service Directory services: got %d, want 3", got)
	}
	if got := mocks.count("gcp:servicedirectory/endpoint:Endpoint"); got != 3 {
		t.Errorf("Service Directory endpoints: got %d, want 3", got)
	}

	// Cross-check: every Cloud Run service references a unique
	// per-service SA email (not the project default).
	saEmailsPerService := map[string]string{}
	for _, svc := range runSvcs {
		template := svc.Inputs["template"].ObjectValue()
		saField := template["serviceAccount"]
		if !saField.HasValue() {
			t.Errorf("Cloud Run service %q missing template.serviceAccount", svc.Name)
			continue
		}
		saEmailsPerService[svc.Name] = saField.StringValue()
	}
	seen := map[string]bool{}
	for name, email := range saEmailsPerService {
		if seen[email] {
			t.Errorf("service %q reuses SA email %q — WI binding must be unique per service",
				name, email)
		}
		seen[email] = true
	}
}

// ---------------------------------------------------------------------------
// Negative-path: factory rejects misconfiguration loudly
// ---------------------------------------------------------------------------

// TestCloudRunServiceRejectsMissingImage locks the validation the factory
// does at program-build time so callers get a fast, actionable error
// instead of an opaque Cloud Run 400 at apply time.
func TestCloudRunServiceRejectsMissingImage(t *testing.T) {
	mocks := &computeMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &kconfig.Config{
			Project:      "kaizen",
			Environment:  "dev",
			GCPProjectID: "kaizen-experimentation-dev",
			GCPRegion:    "us-central1",
		}
		inputs := &gcpcompute.Inputs{
			Project: "kaizen-experimentation-dev",
			Region:  "us-central1",
			VpcConnectorSelfLink: pulumi.String("projects/p/locations/r/connectors/c").
				ToStringOutput(),
			ServiceDirectoryNamespaceID: pulumi.String("projects/p/locations/r/namespaces/n").
				ToStringOutput(),
		}
		_, err := gcpcompute.NewCloudRunService(ctx, cfg, inputs, "m1-assignment",
			&gcpcompute.Options{
				// Image deliberately omitted.
				ContainerPort: 50051,
			})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil {
		t.Fatal("expected error for missing Image, got nil")
	}
	if !strings.Contains(err.Error(), "opts.Image is required") {
		t.Errorf("error %q does not mention missing image", err.Error())
	}
}
