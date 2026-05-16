package main

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/gcp/services"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m6Mocks records every resource gcp.NewCompute registers and enriches the
// type tokens the M6 path touches (Cloud Run URL, SA email, secret name) so
// the SD-endpoint stripScheme/ApplyT chain and the saMember binding resolve
// to realistic strings under pulumi.WithMocks.
type m6Mocks struct {
	mu        sync.Mutex
	resources []m6Resource
}

type m6Resource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *m6Mocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, m6Resource{TypeToken: args.TypeToken, Name: args.Name, Inputs: args.Inputs})
	m.mu.Unlock()

	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}
	switch args.TypeToken {
	case "gcp:serviceaccount/account:Account":
		acct, proj := "", ""
		if v, ok := args.Inputs["accountId"]; ok && v.HasValue() {
			acct = v.StringValue()
		}
		if v, ok := args.Inputs["project"]; ok && v.HasValue() {
			proj = v.StringValue()
		}
		outputs["email"] = resource.NewStringProperty(acct + "@" + proj + ".iam.gserviceaccount.com")
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
		if v, ok := args.Inputs["serviceId"]; ok {
			outputs["serviceId"] = v
		}
	}
	return args.Name + "_id", outputs, nil
}

func (m *m6Mocks) Call(_ pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *m6Mocks) byType(tok string) []m6Resource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []m6Resource
	for _, r := range m.resources {
		if r.TypeToken == tok {
			out = append(out, r)
		}
	}
	return out
}

// runM6Compute exercises gcp.NewCompute with the auth secret + CICD inputs M6
// (#494) requires, and returns the recorded resources plus the resolved
// ServiceEndpoints map.
func runM6Compute(t *testing.T) (*m6Mocks, map[string]string) {
	t.Helper()
	mocks := &m6Mocks{}
	endpoints := map[string]string{}
	var mu sync.Mutex

	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &kconfig.Config{
			Project:      "kaizen",
			Environment:  "dev",
			Env:          kconfig.EnvDev,
			GCPProjectID: "kaizen-experimentation-dev",
			GCPRegion:    "us-central1",
		}
		netOut := types.NetworkOutputs{
			PrivateSubnetIds: pulumi.StringArray{
				pulumi.String("https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/subnetworks/kaizen-private"),
			}.ToStringArrayOutput(),
			ServiceDiscoveryId:   pulumi.ID("projects/test/locations/us-central1/namespaces/kaizen-local").ToIDOutput(),
			VpcConnectorSelfLink: pulumi.String("projects/test/locations/us-central1/connectors/kaizen-vpc").ToStringOutput(),
		}
		cicdOut := types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"ui": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/ui").ToStringOutput(),
				// NewCompute provisions every wired per-service Cloud Run service
				// in one call, so this fixture must satisfy each service's image
				// lookup even when the test is scoped to M6.
				"assignment": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/assignment",
				).ToStringOutput(),
				"orchestration": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/orchestration",
				).ToStringOutput(),
				"analysis": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/analysis",
				).ToStringOutput(),
				"metrics": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/metrics",
				).ToStringOutput(),
				"flags": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/flags",
				).ToStringOutput(),
				"pipeline": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/pipeline",
				).ToStringOutput(),
				"management": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/management",
				).ToStringOutput(),
			},
		}
		dbOut := types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
			Port:     pulumi.Int(5432).ToIntOutput(),
		}
		streamOut := types.StreamingOutputs{
			BootstrapBrokers: pulumi.String(
				"seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092",
			).ToStringOutput(),
		}
		secretsOut := types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef:    pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-kafka").ToStringOutput(),
			RedisSecretRef:    pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-redis").ToStringOutput(),
			AuthSecretRef:     pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-auth").ToStringOutput(),
		}
		storageOut := types.StorageOutputs{
			DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
			DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
		}
		cacheOut := types.CacheOutputs{
			Endpoint: pulumi.String("redis://10.99.1.1:6379").ToStringOutput(),
		}

		out, err := gcp.NewCompute(ctx, cfg, services.StageOutputs{
			Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
			Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
		})
		if err != nil {
			return err
		}
		for k, v := range out.ServiceEndpoints {
			key := k
			v.ApplyT(func(s string) string {
				mu.Lock()
				endpoints[key] = s
				mu.Unlock()
				return s
			})
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("gcp.NewCompute failed: %v", err)
	}
	return mocks, endpoints
}

// AC1: "M6 Cloud Run service appears in ComputeOutputs.ServiceEndpoints["m6"]."
func TestGCPCompute_M6_InServiceEndpoints(t *testing.T) {
	_, endpoints := runM6Compute(t)

	url, ok := endpoints["m6"]
	if !ok {
		t.Fatalf("ServiceEndpoints missing key %q; got keys %v", "m6", endpointKeys(endpoints))
	}
	if !strings.Contains(url, "m6-ui") || !strings.HasPrefix(url, "https://") {
		t.Errorf("ServiceEndpoints[\"m6\"] = %q, want the m6-ui Cloud Run https URL", url)
	}
}

// AC2 proxy: the Cloud Run service must listen on M6's port (3000, the Next.js
// SSR server port) so Cloud Run's startup/health probe — and the documented
// "health check returns 200" acceptance gate — targets the right port. The
// real deployed-stack 200 is a CI/manual step (no GCP creds in unit tests).
func TestGCPCompute_M6_CloudRunPortAndImage(t *testing.T) {
	mocks, _ := runM6Compute(t)

	var m6 *m6Resource
	for _, r := range mocks.byType("gcp:cloudrunv2/service:Service") {
		if v, ok := r.Inputs["name"]; ok && v.StringValue() == "kaizen-dev-m6-ui" {
			rr := r
			m6 = &rr
		}
	}
	if m6 == nil {
		t.Fatal("no Cloud Run service named kaizen-dev-m6-ui registered")
	}

	tmpl := m6.Inputs["template"].ObjectValue()
	containers := tmpl["containers"].ArrayValue()
	if len(containers) != 1 {
		t.Fatalf("expected 1 container, got %d", len(containers))
	}
	c := containers[0].ObjectValue()
	port := c["ports"].ObjectValue()["containerPort"]
	if !port.HasValue() || port.NumberValue() != 3000 {
		t.Errorf("M6 containerPort = %v, want 3000", port)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "/kaizen/ui") {
		t.Errorf("M6 image = %q, want the Artifact Registry ui repo", img)
	}
}

// "IAM scopes: read auth secret." The factory must mint a SecretIamMember
// granting M6's runtime SA roles/secretmanager.secretAccessor on the auth
// secret, and the container must read it via a secret env ref.
func TestGCPCompute_M6_AuthSecretBindingAndEnv(t *testing.T) {
	mocks, _ := runM6Compute(t)

	bindings := mocks.byType("gcp:secretmanager/secretIamMember:SecretIamMember")
	foundAuthAccessor := false
	for _, b := range bindings {
		roleVal, _ := b.Inputs["role"]
		sidVal, _ := b.Inputs["secretId"]
		if roleVal.HasValue() && roleVal.StringValue() == "roles/secretmanager.secretAccessor" &&
			sidVal.HasValue() && strings.Contains(sidVal.StringValue(), "kaizen-dev-auth") {
			foundAuthAccessor = true
		}
	}
	if !foundAuthAccessor {
		t.Error("no SecretIamMember granting roles/secretmanager.secretAccessor on the auth secret to M6's SA")
	}

	var m6 *m6Resource
	for _, r := range mocks.byType("gcp:cloudrunv2/service:Service") {
		if v, ok := r.Inputs["name"]; ok && v.StringValue() == "kaizen-dev-m6-ui" {
			rr := r
			m6 = &rr
		}
	}
	if m6 == nil {
		t.Fatal("no kaizen-dev-m6-ui Cloud Run service")
	}
	envs := m6.Inputs["template"].ObjectValue()["containers"].ArrayValue()[0].ObjectValue()["envs"].ArrayValue()
	hasAuthSecretEnv := false
	for _, e := range envs {
		eo := e.ObjectValue()
		if eo["name"].StringValue() != "AUTH_SECRET" {
			continue
		}
		if vs, ok := eo["valueSource"]; ok && vs.IsObject() {
			hasAuthSecretEnv = true
		}
	}
	if !hasAuthSecretEnv {
		t.Error("M6 container missing AUTH_SECRET env sourced from Secret Manager (valueSource.secretKeyRef)")
	}
}

// AC3: "M6 can resolve and reach at least one backend service through the
// Service Directory." M6 itself must register an SD service/endpoint (so peers
// can resolve it), and it must carry a backend endpoint env var pointing at an
// SD-registered service. M4b is the one backend that exists at this stage.
func TestGCPCompute_M6_ServiceDirectoryAndBackendEnv(t *testing.T) {
	mocks, _ := runM6Compute(t)

	m6SD := false
	for _, s := range mocks.byType("gcp:servicedirectory/service:Service") {
		if v, ok := s.Inputs["serviceId"]; ok && v.StringValue() == "m6-ui" {
			m6SD = true
		}
	}
	if !m6SD {
		t.Error("M6 did not register a Service Directory service with serviceId=m6-ui")
	}

	var m6 *m6Resource
	for _, r := range mocks.byType("gcp:cloudrunv2/service:Service") {
		if v, ok := r.Inputs["name"]; ok && v.StringValue() == "kaizen-dev-m6-ui" {
			rr := r
			m6 = &rr
		}
	}
	if m6 == nil {
		t.Fatal("no kaizen-dev-m6-ui Cloud Run service")
	}
	envs := m6.Inputs["template"].ObjectValue()["containers"].ArrayValue()[0].ObjectValue()["envs"].ArrayValue()
	hasBackend := false
	for _, e := range envs {
		if e.ObjectValue()["name"].StringValue() == "M4B_POLICY_ENDPOINT" {
			hasBackend = true
		}
	}
	if !hasBackend {
		t.Error("M6 missing M4B_POLICY_ENDPOINT env — cannot reach a backend resolved via Service Directory")
	}
}

func endpointKeys(m map[string]string) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}
