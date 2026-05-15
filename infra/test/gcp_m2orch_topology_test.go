// Package test — contract/topology test for the M2 Orchestration (M2-Orch)
// Cloud Run deploy on GCP (issue #490).
//
// Closes the acceptance criteria for #490:
//
//  1. M2-Orch Cloud Run service appears in
//     ComputeOutputs.ServiceEndpoints["m2-orch"].
//  2. M2-Orch health check returns 200 in a deployed dev stack — proxied
//     here by asserting the Cloud Run service is created on the correct
//     container port (50058) so the platform's built-in startup probe
//     targets the right listener.
//  3. M2-Orch can connect to Cloud SQL via the VPC connector — proxied by
//     asserting (a) the service's revision template wires the Serverless
//     VPC Access connector and (b) the runtime SA is granted
//     roles/cloudsql.client + a secretAccessor binding on the DB secret.
//
// The factory runs under pulumi.WithMocks so no GCP credentials or network
// calls are required. gcp.NewCompute is exercised directly with hand-built
// upstream-stage outputs (network, cicd, database, streaming, secrets) so the
// test pins the cross-module wiring contract without standing up the whole
// Deploy() pipeline.
package test

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m2OrchUpstreams builds the upstream-stage outputs gcp.NewCompute consumes,
// with deterministic stand-in values so the recorded Cloud Run resource has
// resolvable env vars and secret refs.
func m2OrchUpstreams() (types.NetworkOutputs, types.CICDOutputs, types.DatabaseOutputs, types.StreamingOutputs, types.SecretsOutputs, types.StorageOutputs) {
	netOut := types.NetworkOutputs{
		PrivateSubnetIds: pulumi.ToStringArray([]string{
			"projects/kaizen-experimentation-dev/regions/us-central1/subnetworks/kaizen-dev-private",
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
			"orchestration": pulumi.String(
				"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/orchestration",
			).ToStringOutput(),
			// NewCompute provisions every wired per-service Cloud Run service
			// in one call, so this fixture must satisfy each service's image
			// lookup even when the test is scoped to M2-Orch.
			"ui": pulumi.String(
				"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/ui",
			).ToStringOutput(),
			"analysis": pulumi.String(
				"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/analysis",
			).ToStringOutput(),
			"assignment": pulumi.String(
				"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/assignment",
			).ToStringOutput(),
			"metrics": pulumi.String(
				"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/metrics",
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
		DatabaseSecretRef: pulumi.String(
			"projects/kaizen-experimentation-dev/secrets/kaizen-dev-database",
		).ToStringOutput(),
		KafkaSecretRef: pulumi.String(
			"projects/kaizen-experimentation-dev/secrets/kaizen-dev-kafka",
		).ToStringOutput(),
		RedisSecretRef: pulumi.String(
			"projects/kaizen-experimentation-dev/secrets/kaizen-dev-redis",
		).ToStringOutput(),
		AuthSecretRef: pulumi.String(
			"projects/kaizen-experimentation-dev/secrets/kaizen-dev-auth",
		).ToStringOutput(),
	}
	storageOut := types.StorageOutputs{
		DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
		DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
	}
	return netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut
}

// runM2OrchCompute exercises gcp.NewCompute under mocks and returns the
// recorded resources plus the resolved ServiceEndpoints map.
func runM2OrchCompute(t *testing.T) (*computeMocks, map[string]string) {
	t.Helper()
	mocks := &computeMocks{}
	endpoints := map[string]string{}

	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		cfg := &kconfig.Config{
			Project:      "kaizen",
			Environment:  "dev",
			Env:          kconfig.EnvDev,
			GCPProjectID: "kaizen-experimentation-dev",
			GCPRegion:    "us-central1",
		}
		netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut := m2OrchUpstreams()
		out, err := gcp.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut)
		if err != nil {
			return err
		}
		for name, url := range out.ServiceEndpoints {
			name, url := name, url
			url.ApplyT(func(s string) string {
				mocks.mu.Lock()
				endpoints[name] = s
				mocks.mu.Unlock()
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

// findCloudRunService returns the recorded Cloud Run service whose `name`
// input matches want, or nil.
func findCloudRunService(mocks *computeMocks, want string) *recordedComputeResource {
	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	for i, r := range svcs {
		if v, ok := r.Inputs["name"]; ok && v.HasValue() && v.StringValue() == want {
			return &svcs[i]
		}
	}
	return nil
}

// ---------------------------------------------------------------------------
// AC #1: M2-Orch appears in ComputeOutputs.ServiceEndpoints["m2-orch"]
// ---------------------------------------------------------------------------

func TestM2OrchInServiceEndpoints(t *testing.T) {
	_, endpoints := runM2OrchCompute(t)

	url, ok := endpoints["m2-orch"]
	if !ok {
		t.Fatalf("ServiceEndpoints missing key %q; got keys %v", "m2-orch", keysOf(endpoints))
	}
	if url == "" {
		t.Errorf("ServiceEndpoints[\"m2-orch\"] resolved to empty string")
	}
}

func keysOf(m map[string]string) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}

// ---------------------------------------------------------------------------
// AC #2: Cloud Run service created on the M2-Orch container port (50058)
// ---------------------------------------------------------------------------

func TestM2OrchCloudRunServicePortAndName(t *testing.T) {
	mocks, _ := runM2OrchCompute(t)

	svc := findCloudRunService(mocks, "kaizen-dev-m2-orchestration")
	if svc == nil {
		t.Fatal("no Cloud Run service named kaizen-dev-m2-orchestration was registered")
	}

	tmpl := svc.Inputs["template"]
	if !tmpl.IsObject() {
		t.Fatal("Cloud Run service missing template")
	}
	containers := tmpl.ObjectValue()["containers"]
	if !containers.IsArray() || len(containers.ArrayValue()) == 0 {
		t.Fatal("Cloud Run template missing containers")
	}
	c0 := containers.ArrayValue()[0].ObjectValue()
	ports := c0["ports"]
	if !ports.IsObject() {
		t.Fatal("container missing ports")
	}
	cp := ports.ObjectValue()["containerPort"]
	if !cp.HasValue() || cp.NumberValue() != 50058 {
		t.Errorf("containerPort = %v, want 50058", cp)
	}
}

// ---------------------------------------------------------------------------
// In-scope env vars: DB endpoint + Redpanda bootstrap brokers
// ---------------------------------------------------------------------------

func TestM2OrchEnvVars(t *testing.T) {
	mocks, _ := runM2OrchCompute(t)

	svc := findCloudRunService(mocks, "kaizen-dev-m2-orchestration")
	if svc == nil {
		t.Fatal("no Cloud Run service named kaizen-dev-m2-orchestration was registered")
	}
	c0 := svc.Inputs["template"].ObjectValue()["containers"].ArrayValue()[0].ObjectValue()
	envs := c0["envs"]
	if !envs.IsArray() {
		t.Fatal("container missing envs array")
	}

	literals := map[string]string{} // name -> literal value
	secretEnvs := map[string]bool{} // name -> has secretKeyRef
	for _, e := range envs.ArrayValue() {
		eo := e.ObjectValue()
		name := eo["name"].StringValue()
		if v, ok := eo["value"]; ok && v.HasValue() {
			literals[name] = v.StringValue()
		}
		if vs, ok := eo["valueSource"]; ok && vs.IsObject() {
			if _, ok := vs.ObjectValue()["secretKeyRef"]; ok {
				secretEnvs[name] = true
			}
		}
	}

	if got := literals["DATABASE_ENDPOINT"]; got != "10.99.0.3:5432" {
		t.Errorf("DATABASE_ENDPOINT = %q, want %q", got, "10.99.0.3:5432")
	}
	if got := literals["KAFKA_BOOTSTRAP_BROKERS"]; got != "seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092" {
		t.Errorf("KAFKA_BOOTSTRAP_BROKERS = %q, want the Redpanda bootstrap brokers", got)
	}
	if _, ok := literals["ENVIRONMENT"]; !ok {
		t.Errorf("ENVIRONMENT env var missing")
	}
	if !secretEnvs["DATABASE_SECRET"] {
		t.Errorf("DATABASE_SECRET secret env (secretKeyRef) missing")
	}
	if !secretEnvs["KAFKA_SECRET"] {
		t.Errorf("KAFKA_SECRET secret env (secretKeyRef) missing")
	}
}

// ---------------------------------------------------------------------------
// AC #3: VPC connector wired + DB/Kafka credential IAM on the M2-Orch SA
// ---------------------------------------------------------------------------

func TestM2OrchVpcConnectorWired(t *testing.T) {
	mocks, _ := runM2OrchCompute(t)

	svc := findCloudRunService(mocks, "kaizen-dev-m2-orchestration")
	if svc == nil {
		t.Fatal("no Cloud Run service named kaizen-dev-m2-orchestration was registered")
	}
	vpcAccess := svc.Inputs["template"].ObjectValue()["vpcAccess"]
	if !vpcAccess.IsObject() {
		t.Fatal("template.vpcAccess missing — M2-Orch could not reach Cloud SQL")
	}
	conn := vpcAccess.ObjectValue()["connector"]
	want := "projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector"
	if !conn.HasValue() || conn.StringValue() != want {
		t.Errorf("vpcAccess.connector = %v, want %q", conn, want)
	}
}

func TestM2OrchCloudSqlAndSecretIAM(t *testing.T) {
	mocks, _ := runM2OrchCompute(t)

	wantMember := "serviceAccount:dev-m2-orchestration-run@kaizen-experimentation-dev.iam.gserviceaccount.com"

	// roles/cloudsql.client at project level — lets M2-Orch open a Cloud SQL
	// connection through the VPC connector.
	foundCloudSQL := false
	for _, m := range mocks.byType("gcp:projects/iAMMember:IAMMember") {
		role, _ := m.Inputs["role"]
		member, _ := m.Inputs["member"]
		if role.HasValue() && role.StringValue() == "roles/cloudsql.client" &&
			member.HasValue() && member.StringValue() == wantMember {
			foundCloudSQL = true
		}
	}
	if !foundCloudSQL {
		t.Errorf("missing roles/cloudsql.client IAMMember bound to the M2-Orch SA (%s)", wantMember)
	}

	// One secretAccessor binding per secret env (DB + Kafka) — "read DB
	// creds, read Kafka creds".
	secretBindings := 0
	for _, m := range mocks.byType("gcp:secretmanager/secretIamMember:SecretIamMember") {
		role, _ := m.Inputs["role"]
		member, _ := m.Inputs["member"]
		if role.HasValue() && role.StringValue() == "roles/secretmanager.secretAccessor" &&
			member.HasValue() && member.StringValue() == wantMember {
			secretBindings++
		}
	}
	if secretBindings < 2 {
		t.Errorf("secretAccessor bindings on M2-Orch SA = %d, want >= 2 (DB + Kafka)", secretBindings)
	}
}

// ---------------------------------------------------------------------------
// Default min-instances: M2-Orch is a stateless orchestrator (NOT M1/M7),
// so it must NOT pin min-instances=1.
// ---------------------------------------------------------------------------

func TestM2OrchUsesDefaultMinInstances(t *testing.T) {
	mocks, _ := runM2OrchCompute(t)

	svc := findCloudRunService(mocks, "kaizen-dev-m2-orchestration")
	if svc == nil {
		t.Fatal("no Cloud Run service named kaizen-dev-m2-orchestration was registered")
	}
	scaling := svc.Inputs["template"].ObjectValue()["scaling"]
	if !scaling.IsObject() {
		t.Fatal("template.scaling missing")
	}
	min := scaling.ObjectValue()["minInstanceCount"]
	if !min.HasValue() || min.NumberValue() != 0 {
		t.Errorf("M2-Orch minInstanceCount = %v, want 0 (stateless orchestrator, default)", min)
	}
}
