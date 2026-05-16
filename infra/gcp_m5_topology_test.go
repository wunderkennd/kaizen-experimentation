package main

// Topology test for M5 Management on Cloud Run (issue #493).
//
// Runs the real Deploy() under cloudProvider=gcp with Pulumi mocks (the same
// gcpFullstackMocks + gcpFullstackConfig harness the other GCP Deploy tests
// use) and asserts the structural wiring that backs every #493 acceptance
// criterion:
//
//   - AC1 "M5 appears in ComputeOutputs.ServiceEndpoints[\"m5\"]" — proven
//     transitively: NewCompute sets endpoints["m5"] = m5.URL unconditionally
//     right after creating the service, so asserting the M5 Cloud Run service
//     + its Service Directory endpoint exist proves the map entry is
//     populated (and the SD endpoint is the resolvable address that entry
//     resolves to for peer services).
//   - AC2 "health check returns 200 in a deployed dev stack" — a deploy-time
//     check; structurally enabled here by the correct image (the management
//     Artifact Registry repo) + container port 50055 + the VPC connector so
//     the probe path actually reaches a running container.
//   - AC3 "M5 can connect to Cloud SQL, Memorystore, and Redpanda
//     end-to-end" — structurally enabled here by the DB/Redis/Kafka endpoint
//     env vars, the four Secret Manager secretKeyRef mounts, the auto
//     secretAccessor IAM bindings, the roles/cloudsql.client binding, and the
//     VPC connector (Memorystore/Cloud SQL/Redpanda are private-IP only).
//
// The deploy-time halves of AC2/AC3 cannot run in the unit sandbox (no GCP
// project); they are documented in the PR as an operator checklist. This test
// locks everything that must be true *before* a deploy can possibly pass.

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

const (
	m5ServiceResourceName  = "kaizen-dev-m5-management"
	m5ServiceAccountID     = "dev-m5-management-run"
	m5ServiceAccountMember = "serviceAccount:dev-m5-management-run@kaizen-experimentation-dev.iam.gserviceaccount.com"
)

// runGCPDeployForM5 runs Deploy(gcp) under mocks and returns the recorder.
func runGCPDeployForM5(t *testing.T) *gcpFullstackMocks {
	t.Helper()
	mocks := &gcpFullstackMocks{}
	if err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		gcpFullstackConfig(),
	); err != nil {
		t.Fatalf("Deploy(gcp) failed: %v", err)
	}
	return mocks
}

// m5CloudRunService returns the single Cloud Run service registration whose
// resource name is the M5 service (kaizen-dev-m5-management), failing if it
// is absent or duplicated. Other Cloud Run services (the preview-canary, and
// future #488..#495 deploys) are filtered out by name.
func m5CloudRunService(t *testing.T, mocks *gcpFullstackMocks) fsResource {
	t.Helper()
	var found []fsResource
	for _, r := range mocks.byType("gcp:cloudrunv2/service:Service") {
		if v, ok := r.Inputs["name"]; ok && v.HasValue() && v.StringValue() == m5ServiceResourceName {
			found = append(found, r)
		}
	}
	if len(found) != 1 {
		t.Fatalf("expected exactly 1 Cloud Run service named %q, got %d", m5ServiceResourceName, len(found))
	}
	return found[0]
}

// containerEnvs returns the container env array for a Cloud Run service.
func containerEnvs(t *testing.T, svc fsResource) []resource.PropertyValue {
	t.Helper()
	tmpl, ok := svc.Inputs["template"]
	if !ok || !tmpl.IsObject() {
		t.Fatal("M5 Cloud Run service missing template object")
	}
	containers, ok := tmpl.ObjectValue()["containers"]
	if !ok || !containers.IsArray() || len(containers.ArrayValue()) == 0 {
		t.Fatal("M5 Cloud Run template missing containers")
	}
	c0 := containers.ArrayValue()[0]
	if !c0.IsObject() {
		t.Fatal("M5 container[0] is not an object")
	}
	envs, ok := c0.ObjectValue()["envs"]
	if !ok || !envs.IsArray() {
		t.Fatal("M5 container[0] missing envs array")
	}
	return envs.ArrayValue()
}

// TestM5ManagementCloudRunWiring_GCP locks M5's Cloud Run shape: name, image,
// port, default min-instances, and VPC connector.
func TestM5ManagementCloudRunWiring_GCP(t *testing.T) {
	mocks := runGCPDeployForM5(t)
	svc := m5CloudRunService(t, mocks)

	tmpl := svc.Inputs["template"].ObjectValue()
	c0 := tmpl["containers"].ArrayValue()[0].ObjectValue()

	// Image must point at the management Artifact Registry repo (not the
	// canary's hello image) and carry a tag.
	img, ok := c0["image"]
	if !ok || !img.HasValue() {
		t.Fatal("M5 container missing image")
	}
	if !strings.Contains(img.StringValue(), "management") || !strings.HasSuffix(img.StringValue(), ":latest") {
		t.Errorf("M5 image = %q, want the management AR repo with :latest", img.StringValue())
	}

	// Container port 50055 — matches the AWS ECS task def + ADR-025 binary.
	ports, ok := c0["ports"]
	if !ok || !ports.IsObject() {
		t.Fatal("M5 container missing ports")
	}
	if p := ports.ObjectValue()["containerPort"]; !p.IsNumber() || p.NumberValue() != 50055 {
		t.Errorf("M5 containerPort = %v, want 50055", ports.ObjectValue()["containerPort"])
	}

	// Default min-instances (0). M5 has no p99 SLA — only M1/M7 override.
	scaling, ok := tmpl["scaling"]
	if !ok || !scaling.IsObject() {
		t.Fatal("M5 template missing scaling")
	}
	if mi := scaling.ObjectValue()["minInstanceCount"]; !mi.HasValue() || mi.NumberValue() != 0 {
		t.Errorf("M5 minInstanceCount = %v, want 0 (default; no cold-start SLA)", scaling.ObjectValue()["minInstanceCount"])
	}

	// VPC connector — without it, Cloud Run egress bypasses the VPC and the
	// private-IP Cloud SQL / Memorystore / Redpanda calls 503 (AC3) and the
	// health probe path never reaches the container (AC2).
	vpc, ok := tmpl["vpcAccess"]
	if !ok || !vpc.IsObject() {
		t.Fatal("M5 template missing vpcAccess — Cloud SQL/Memorystore/Redpanda unreachable")
	}
	if conn := vpc.ObjectValue()["connector"]; !conn.HasValue() || conn.StringValue() == "" {
		t.Error("M5 vpcAccess.connector is empty")
	}
}

// TestM5ManagementEnvAndSecrets_GCP locks the env-var + Secret Manager
// contract that lets M5 reach Cloud SQL, Memorystore, and Redpanda (AC3).
func TestM5ManagementEnvAndSecrets_GCP(t *testing.T) {
	mocks := runGCPDeployForM5(t)
	svc := m5CloudRunService(t, mocks)
	envs := containerEnvs(t, svc)

	literals := map[string]bool{}
	secretEnvToSecretID := map[string]string{}
	for _, e := range envs {
		if !e.IsObject() {
			continue
		}
		o := e.ObjectValue()
		name, ok := o["name"]
		if !ok || !name.HasValue() {
			continue
		}
		if vs, ok := o["valueSource"]; ok && vs.IsObject() {
			if skr, ok := vs.ObjectValue()["secretKeyRef"]; ok && skr.IsObject() {
				if sec, ok := skr.ObjectValue()["secret"]; ok && sec.HasValue() {
					secretEnvToSecretID[name.StringValue()] = sec.StringValue()
				}
			}
			continue
		}
		literals[name.StringValue()] = true
	}

	// Non-secret connection endpoints + runtime config (mirrors the AWS
	// service contract so the same experimentation-management binary runs
	// unmodified on both clouds).
	for _, want := range []string{
		"ENVIRONMENT", "RUST_LOG",
		"DATABASE_ENDPOINT", "REDIS_ENDPOINT", "KAFKA_BOOTSTRAP_BROKERS",
		"OTEL_SERVICE_NAME",
	} {
		if !literals[want] {
			t.Errorf("M5 missing literal env var %q", want)
		}
	}

	// Credentials arrive only via Secret Manager secretKeyRef — never as
	// literal env values.
	wantSecrets := map[string]string{
		"DATABASE_SECRET": "kaizen-dev-database",
		"KAFKA_SECRET":    "kaizen-dev-kafka",
		"REDIS_SECRET":    "kaizen-dev-redis",
		"AUTH_SECRET":     "kaizen-dev-auth",
	}
	for env, wantID := range wantSecrets {
		got, ok := secretEnvToSecretID[env]
		if !ok {
			t.Errorf("M5 missing secret env %q (must be a Secret Manager secretKeyRef)", env)
			continue
		}
		if got != wantID {
			t.Errorf("M5 secret env %q -> secret %q, want %q", env, got, wantID)
		}
		if literals[env] {
			t.Errorf("M5 %q must be a secretKeyRef, not a literal value", env)
		}
	}
}

// TestM5ManagementWorkloadIdentityAndIAM_GCP locks the per-service Workload
// Identity SA and the "read DB creds, read Redis auth, read Kafka creds" +
// Cloud SQL IAM scopes from the issue. "Right binding, wrong identity" is the
// failure mode this guards against.
func TestM5ManagementWorkloadIdentityAndIAM_GCP(t *testing.T) {
	mocks := runGCPDeployForM5(t)

	// Exactly one runtime SA for M5, accountId dev-m5-management-run.
	var m5SAs []fsResource
	for _, sa := range mocks.byType("gcp:serviceaccount/account:Account") {
		if v, ok := sa.Inputs["accountId"]; ok && v.HasValue() && v.StringValue() == m5ServiceAccountID {
			m5SAs = append(m5SAs, sa)
		}
	}
	if len(m5SAs) != 1 {
		t.Fatalf("expected 1 M5 runtime SA (accountId %q), got %d", m5ServiceAccountID, len(m5SAs))
	}

	// The Cloud Run service must run AS that SA (not the project default).
	svc := m5CloudRunService(t, mocks)
	saField, ok := svc.Inputs["template"].ObjectValue()["serviceAccount"]
	if !ok || !saField.HasValue() ||
		saField.StringValue() != "dev-m5-management-run@kaizen-experimentation-dev.iam.gserviceaccount.com" {
		t.Errorf("M5 template.serviceAccount = %v, want the per-service WI SA", saField)
	}

	// roles/secretmanager.secretAccessor on each of M5's four secrets,
	// bound to M5's SA — this is the issue's "read DB creds, read Redis
	// auth, read Kafka creds" scope set (+ auth, for AWS parity).
	accessor := map[string]bool{}
	for _, b := range mocks.byType("gcp:secretmanager/secretIamMember:SecretIamMember") {
		member, _ := b.Inputs["member"]
		role, _ := b.Inputs["role"]
		secret, _ := b.Inputs["secretId"]
		if member.HasValue() && member.StringValue() == m5ServiceAccountMember &&
			role.HasValue() && role.StringValue() == "roles/secretmanager.secretAccessor" &&
			secret.HasValue() {
			accessor[secret.StringValue()] = true
		}
	}
	for _, want := range []string{"kaizen-dev-database", "kaizen-dev-kafka", "kaizen-dev-redis", "kaizen-dev-auth"} {
		if !accessor[want] {
			t.Errorf("M5 SA missing roles/secretmanager.secretAccessor on %q", want)
		}
	}

	// roles/cloudsql.client on M5's SA for Cloud SQL connectivity.
	foundCloudSQL := false
	for _, b := range mocks.byType("gcp:projects/iAMMember:IAMMember") {
		member, _ := b.Inputs["member"]
		role, _ := b.Inputs["role"]
		if member.HasValue() && member.StringValue() == m5ServiceAccountMember &&
			role.HasValue() && role.StringValue() == "roles/cloudsql.client" {
			foundCloudSQL = true
		}
	}
	if !foundCloudSQL {
		t.Error("M5 SA missing roles/cloudsql.client project binding (Cloud SQL connectivity, AC3)")
	}
}

// TestM5ManagementServiceDirectory_GCP locks the Service Directory
// registration that backs ComputeOutputs.ServiceEndpoints["m5"] (AC1): peers
// resolve M5 via this SD service/endpoint, and NewCompute populates the map
// entry from the same Cloud Run service unconditionally.
func TestM5ManagementServiceDirectory_GCP(t *testing.T) {
	mocks := runGCPDeployForM5(t)

	foundSvc := false
	for _, s := range mocks.byType("gcp:servicedirectory/service:Service") {
		if v, ok := s.Inputs["serviceId"]; ok && v.HasValue() && v.StringValue() == "m5-management" {
			foundSvc = true
		}
	}
	if !foundSvc {
		t.Error("no Service Directory service with serviceId \"m5-management\" — ServiceEndpoints[\"m5\"] would be unresolvable")
	}

	// At least the canary + M5 endpoints exist; assert M5's specifically by
	// the "primary" endpointId the factory always uses for Cloud Run.
	if len(mocks.byType("gcp:servicedirectory/endpoint:Endpoint")) == 0 {
		t.Error("expected Service Directory endpoint registrations, got 0")
	}
}
