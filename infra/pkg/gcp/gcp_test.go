// Package gcp — topology tests for the GCP Deploy() facade aggregators.
//
// This file owns the CI-gated regression guard for issue #491 (M3 Metrics on
// Cloud Run). It lives in package gcp (run by CI's `./pkg/...` infra gate),
// unlike infra/fullstack_gcp_test.go which is a package-main dev proxy CI
// does not execute. Assertions here pin the M3 wiring contract:
//
//   - M3 appears in ComputeOutputs.ServiceEndpoints["m3"]                (AC 1)
//   - M3 Cloud Run service on its real port + Service Directory service  (AC 2)
//   - M3 runtime SA can read DB creds (secret env + accessor + cloudsql) (AC —)
//   - M3 runtime SA has storage.objectAdmin on the data bucket           (AC 3)
//
// The factory + M4b sub-packages own their own resource-shape tests
// (pkg/gcp/compute/{compute,m4b}_test.go, pkg/gcp/secrets/secrets_test.go);
// these tests only assert the aggregator wires M3's inputs correctly.
package gcp

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

const (
	m3RunName  = "kaizen-dev-m3-metrics"
	m3SAMember = "serviceAccount:dev-m3-metrics-run@kaizen-experimentation-dev.iam.gserviceaccount.com"
)

// computeMocks records every resource NewCompute registers and enriches the
// outputs NewCompute's apply() chains depend on (SA email, Cloud Run uri,
// M4b self-links). Modeled on pkg/gcp/compute/m4b_test.go +
// infra/test/gcp_compute_topology_test.go.
type computeMocks struct {
	mu        sync.Mutex
	resources []recordedResource
}

type recordedResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *computeMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, recordedResource{
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
	case "gcp:servicedirectory/service:Service":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/services/" + args.Name)
	}
	return id, outputs, nil
}

func (m *computeMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *computeMocks) byType(tt string) []recordedResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []recordedResource
	for _, r := range m.resources {
		if r.TypeToken == tt {
			out = append(out, r)
		}
	}
	return out
}

// runNewCompute exercises NewCompute once with representative inputs and
// returns the recorded mock + the typed outputs. A single mocked run drives
// every M3 assertion below.
func runNewCompute(t *testing.T) (*computeMocks, types.ComputeOutputs) {
	t.Helper()
	mocks := &computeMocks{}
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
				"projects/kaizen-experimentation-dev/regions/us-central1/subnetworks/kaizen-private",
			}).ToStringArrayOutput(),
			ServiceDiscoveryId: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/namespaces/kaizen-local",
			).ToStringOutput().ApplyT(func(s string) pulumi.ID { return pulumi.ID(s) }).(pulumi.IDOutput),
			VpcConnectorSelfLink: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector",
			).ToStringOutput(),
		}
		cicdOut := types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"metrics": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/metrics",
				).ToStringOutput(),
				// NewCompute provisions every wired per-service Cloud Run service
				// in one call, so this fixture must satisfy each service's image
				// lookup even when the test is scoped to M3.
				"orchestration": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/orchestration",
				).ToStringOutput(),
				"ui": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/ui",
				).ToStringOutput(),
				"analysis": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/analysis",
				).ToStringOutput(),
				"assignment": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/assignment",
				).ToStringOutput(),
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

		var cerr error
		out, cerr = NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut)
		return cerr
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewCompute failed: %v", err)
	}
	return mocks, out
}

func m3RunService(t *testing.T, mocks *computeMocks) recordedResource {
	t.Helper()
	for _, r := range mocks.byType("gcp:cloudrunv2/service:Service") {
		if v, ok := r.Inputs["name"]; ok && v.HasValue() && v.StringValue() == m3RunName {
			return r
		}
	}
	t.Fatalf("no gcp:cloudrunv2/service:Service named %q — M3 not wired into NewCompute", m3RunName)
	return recordedResource{}
}

func m3ContainerObj(t *testing.T, svc recordedResource) resource.PropertyMap {
	t.Helper()
	tmpl, ok := svc.Inputs["template"]
	if !ok || !tmpl.IsObject() {
		t.Fatal("M3 service missing template object")
	}
	containers, ok := tmpl.ObjectValue()["containers"]
	if !ok || !containers.IsArray() || len(containers.ArrayValue()) != 1 {
		t.Fatal("M3 template.containers is not a 1-element array")
	}
	c := containers.ArrayValue()[0]
	if !c.IsObject() {
		t.Fatal("M3 template.containers[0] not an object")
	}
	return c.ObjectValue()
}

// TestNewComputeWiresM3IntoServiceEndpoints is acceptance criterion 1: the
// M3 Cloud Run URL must be reachable through ComputeOutputs.ServiceEndpoints
// under the documented key "m3" (what edge/observability stages consume).
func TestNewComputeWiresM3IntoServiceEndpoints(t *testing.T) {
	_, out := runNewCompute(t)
	if out.ServiceEndpoints == nil {
		t.Fatal("ComputeOutputs.ServiceEndpoints is nil")
	}
	if _, ok := out.ServiceEndpoints["m3"]; !ok {
		t.Errorf("ServiceEndpoints missing key \"m3\"; have %v", keysOf(out.ServiceEndpoints))
	}
	if _, ok := out.ServiceArns["m3"]; !ok {
		t.Errorf("ServiceArns missing key \"m3\"")
	}
}

// TestNewComputeM3CloudRunShape pins the M3 deploy shape: its own port
// (50056, mirroring pkg/aws/compute + CLAUDE.md) and default min-instances=0
// (only M1/M7 pin 1 per the spec Compute Model).
func TestNewComputeM3CloudRunShape(t *testing.T) {
	mocks, _ := runNewCompute(t)
	svc := m3RunService(t, mocks)

	if v, ok := svc.Inputs["location"]; !ok || v.StringValue() != "us-central1" {
		t.Errorf("M3 location = %v, want us-central1", svc.Inputs["location"])
	}

	scaling, ok := svc.Inputs["template"].ObjectValue()["scaling"]
	if !ok || !scaling.IsObject() {
		t.Fatal("M3 template.scaling missing")
	}
	if min, ok := scaling.ObjectValue()["minInstanceCount"]; !ok || min.NumberValue() != 0 {
		t.Errorf("M3 minInstanceCount = %v, want 0 (default)", scaling.ObjectValue()["minInstanceCount"])
	}

	container := m3ContainerObj(t, svc)
	ports, ok := container["ports"]
	if !ok || !ports.IsObject() {
		t.Fatal("M3 container.ports missing")
	}
	if cp, ok := ports.ObjectValue()["containerPort"]; !ok || cp.NumberValue() != 50056 {
		t.Errorf("M3 containerPort = %v, want 50056", ports.ObjectValue()["containerPort"])
	}

	var sawSD bool
	for _, r := range mocks.byType("gcp:servicedirectory/service:Service") {
		if v, ok := r.Inputs["serviceId"]; ok && v.HasValue() && v.StringValue() == "m3-metrics" {
			sawSD = true
		}
	}
	if !sawSD {
		t.Error("no Service Directory service with serviceId m3-metrics — M3 not discoverable")
	}
}

// TestNewComputeM3EnvVars covers the issue's in-scope literal env vars:
// DB endpoint + data bucket name + data bucket gs:// ref.
func TestNewComputeM3EnvVars(t *testing.T) {
	mocks, _ := runNewCompute(t)
	container := m3ContainerObj(t, m3RunService(t, mocks))

	envs, ok := container["envs"]
	if !ok || !envs.IsArray() {
		t.Fatal("M3 container.envs missing")
	}
	literals := map[string]bool{}
	var dbSecretEnv bool
	for _, e := range envs.ArrayValue() {
		if !e.IsObject() {
			continue
		}
		obj := e.ObjectValue()
		name, ok := obj["name"]
		if !ok || !name.HasValue() {
			continue
		}
		if _, isLiteral := obj["value"]; isLiteral {
			literals[name.StringValue()] = true
		}
		if name.StringValue() == "DATABASE_SECRET" {
			if vs, ok := obj["valueSource"]; ok && vs.IsObject() {
				if _, hasRef := vs.ObjectValue()["secretKeyRef"]; hasRef {
					dbSecretEnv = true
				}
			}
		}
	}
	for _, want := range []string{"DATABASE_ENDPOINT", "DATA_BUCKET", "DATA_BUCKET_URI"} {
		if !literals[want] {
			t.Errorf("M3 missing literal env %q (have %v)", want, literals)
		}
	}
	if !dbSecretEnv {
		t.Error("M3 missing DATABASE_SECRET env sourced from Secret Manager (secretKeyRef)")
	}
}

// TestNewComputeM3IAMBindings covers "read DB creds" + acceptance criterion
// 3 (read/write data bucket): every binding must land on the M3 runtime SA.
func TestNewComputeM3IAMBindings(t *testing.T) {
	mocks, _ := runNewCompute(t)

	want := []struct {
		typeToken string
		role      string
		desc      string
	}{
		{"gcp:projects/iAMMember:IAMMember", "roles/cloudsql.client", "Cloud SQL access for metric defs"},
		{"gcp:secretmanager/secretIamMember:SecretIamMember", "roles/secretmanager.secretAccessor", "read DB credentials secret"},
		{"gcp:storage/bucketIAMMember:BucketIAMMember", "roles/storage.objectAdmin", "list/read/write data bucket"},
	}
	for _, tc := range want {
		t.Run(tc.role, func(t *testing.T) {
			var found bool
			for _, r := range mocks.byType(tc.typeToken) {
				role, _ := r.Inputs["role"]
				member, _ := r.Inputs["member"]
				if role.HasValue() && role.StringValue() == tc.role &&
					member.HasValue() && member.StringValue() == m3SAMember {
					found = true
					if tc.typeToken == "gcp:storage/bucketIAMMember:BucketIAMMember" {
						if b, ok := r.Inputs["bucket"]; !ok || b.StringValue() != "kaizen-dev-data" {
							t.Errorf("objectAdmin bound to bucket %v, want kaizen-dev-data", r.Inputs["bucket"])
						}
					}
				}
			}
			if !found {
				t.Errorf("no %s (%s) bound to M3 SA %s [%s]", tc.role, tc.typeToken, m3SAMember, tc.desc)
			}
		})
	}
}

func keysOf(m map[string]pulumi.StringOutput) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}
