// Topology test for the M1 Assignment Cloud Run deploy (issue #488).
//
// Runs the gcp.NewCompute facade under Pulumi mocks (no GCP credentials /
// network) and asserts the four #488 acceptance criteria that are checkable
// at the unit level — the rest ("health check returns 200 in a deployed dev
// stack") is a deploy-time smoke check whose unit proxy is the VPC-connector
// wiring asserted here:
//
//	AC1  M1 appears in ComputeOutputs.ServiceEndpoints["m1"].
//	AC2  M1 Cloud Run service has min-instances = 1 (the p99 < 5ms SLA knob).
//	AC3  M1 egress goes through the Serverless VPC Access connector — the
//	     path Cloud SQL / Memorystore / Redpanda + the /health surface use.
//	AC4  M1 can read DB / Redis / Kafka creds from Secret Manager: exactly
//	     three roles/secretmanager.secretAccessor bindings on M1's runtime
//	     SA (the canary and M4b create none).
//
// This is the unit-level proxy for `pulumi preview --stack gcp-dev` for the
// M1 slice — if it fails, preview will fail.
package main

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	gcpfacade "github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m1TopologyMocks records every resource gcp.NewCompute registers (M4b
// stateful slice + the canary + M1 Cloud Run factory output) and enriches
// the outputs the M1 apply chains depend on (SA email, Cloud Run URI,
// reserved internal IP, SD endpoint echo).
type m1TopologyMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *m1TopologyMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
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
	case "gcp:serviceaccount/account:Account":
		// The factory chains saMember off sa.Email; without an email the
		// secret/IAM members would bind "serviceAccount:" (no local part)
		// and AC4's member assertion could not distinguish identities.
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
		// Cloud Run synthesizes the runtime URL post-deploy; echo a
		// deterministic stand-in so ServiceEndpoints["m1"] resolves.
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

	case "gcp:compute/address:Address":
		outputs["address"] = resource.NewStringProperty("10.0.16.42")
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/addresses/" + args.Name)

	case "gcp:compute/disk:Disk":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/zones/us-central1-a/disks/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)

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

	case "gcp:servicedirectory/service:Service":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/services/" + args.Name)
		if v, ok := args.Inputs["serviceId"]; ok {
			outputs["serviceId"] = v
		}
		if v, ok := args.Inputs["namespace"]; ok {
			outputs["namespace"] = v
		}

	case "gcp:servicedirectory/endpoint:Endpoint":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/endpoints/" + args.Name)
		if v, ok := args.Inputs["address"]; ok {
			outputs["address"] = v
		}
		if v, ok := args.Inputs["port"]; ok {
			outputs["port"] = v
		}
	}

	return id, outputs, nil
}

func (m *m1TopologyMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *m1TopologyMocks) byType(t string) []fsResource {
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

// nestedObject walks an object property path, returning the leaf value and
// whether the whole path resolved to an object/value at each hop.
func nestedObject(pv resource.PropertyValue, path ...string) (resource.PropertyValue, bool) {
	cur := pv
	for _, key := range path {
		if !cur.IsObject() {
			return resource.PropertyValue{}, false
		}
		next, ok := cur.ObjectValue()[resource.PropertyKey(key)]
		if !ok {
			return resource.PropertyValue{}, false
		}
		cur = next
	}
	return cur, true
}

// runM1Compute drives gcp.NewCompute with synthesized cross-stage inputs so
// the M1 slice is exercised in isolation from the Redpanda TF bridge and the
// upstream data-store modules (their outputs are stubbed here; their own
// topology tests cover them).
func runM1Compute(t *testing.T) (*m1TopologyMocks, types.ComputeOutputs, string) {
	t.Helper()
	mocks := &m1TopologyMocks{}
	var out types.ComputeOutputs
	var m1URL string

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
				pulumi.String("projects/kaizen-experimentation-dev/regions/us-central1/subnetworks/kaizen-private"),
			}.ToStringArrayOutput(),
			ServiceDiscoveryId: pulumi.ID(
				"projects/kaizen-experimentation-dev/locations/us-central1/namespaces/kaizen-local",
			).ToIDOutput(),
			VpcConnectorSelfLink: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector",
			).ToStringOutput(),
		}
		cicdOut := types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"assignment": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-assignment",
				).ToStringOutput(),
				// NewCompute provisions every wired per-service Cloud Run service
				// in one call, so this fixture must satisfy each service's image
				// lookup even when the test is scoped to M1.
				"orchestration": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-orchestration",
				).ToStringOutput(),
				"ui": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-ui",
				).ToStringOutput(),
				"analysis": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-analysis",
				).ToStringOutput(),
				"metrics": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-metrics",
				).ToStringOutput(),
				"flags": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-flags",
				).ToStringOutput(),
				"pipeline": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-pipeline",
				).ToStringOutput(),
				"management": pulumi.String(
					"us-docker.pkg.dev/kaizen-experimentation-dev/kaizen-management",
				).ToStringOutput(),
			},
		}
		dbOut := types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
			Port:     pulumi.Int(5432).ToIntOutput(),
		}
		streamOut := types.StreamingOutputs{
			BootstrapBrokers: pulumi.String("seed-abc.any.us-central1.gcp.redpanda.com:9092").ToStringOutput(),
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

		var err error
		out, err = gcpfacade.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut)
		if err != nil {
			return err
		}
		// Resolve the M1 URL inside RunErr so the apply completes.
		if ep, ok := out.ServiceEndpoints["m1"]; ok {
			ep.ApplyT(func(s string) string {
				m1URL = s
				return s
			})
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("gcp.NewCompute (M1 slice) failed: %v", err)
	}
	return mocks, out, m1URL
}

// m1Service returns the single M1 Cloud Run service resource (Pulumi
// resource name "kaizen-dev-m1-assignment-run"), failing if absent or if
// the canary's resource is accidentally matched instead.
func m1Service(t *testing.T, mocks *m1TopologyMocks) fsResource {
	t.Helper()
	for _, r := range mocks.byType("gcp:cloudrunv2/service:Service") {
		if r.Name == "kaizen-dev-m1-assignment-run" {
			return r
		}
	}
	t.Fatalf("no Cloud Run service named kaizen-dev-m1-assignment-run registered")
	return fsResource{}
}

// AC1: M1 is exported through the cross-cloud ServiceEndpoints map.
func TestM1AppearsInServiceEndpoints(t *testing.T) {
	_, out, m1URL := runM1Compute(t)

	ep, ok := out.ServiceEndpoints["m1"]
	if !ok {
		t.Fatalf("ComputeOutputs.ServiceEndpoints missing key \"m1\" (got keys: %v)", keysOf(out.ServiceEndpoints))
	}
	if ep == (pulumi.StringOutput{}) {
		t.Error("ServiceEndpoints[\"m1\"] is the zero StringOutput")
	}
	if !strings.HasPrefix(m1URL, "https://") || !strings.Contains(m1URL, "m1-assignment") {
		t.Errorf("ServiceEndpoints[\"m1\"] resolved to %q, want an https Cloud Run URL containing m1-assignment", m1URL)
	}
}

// AC2: min-instances = 1 — the single knob #488 exists to set.
func TestM1MinInstancesIsOne(t *testing.T) {
	mocks, _, _ := runM1Compute(t)
	svc := m1Service(t, mocks)

	template := findInput(svc, "template")
	min, ok := nestedObject(template, "scaling", "minInstanceCount")
	if !ok || !min.IsNumber() {
		t.Fatalf("M1 template.scaling.minInstanceCount missing/not a number: %v", min)
	}
	if min.NumberValue() != 1 {
		t.Errorf("M1 minInstanceCount = %v, want 1 (p99 < 5ms SLA — no cold starts)", min.NumberValue())
	}
}

// AC3 (unit proxy): M1 egress is routed through the Serverless VPC Access
// connector. The live "/health returns 200 via the VPC connector path"
// check is deploy-time; this asserts the wiring that makes it reachable.
func TestM1RoutesThroughVpcConnector(t *testing.T) {
	mocks, _, _ := runM1Compute(t)
	svc := m1Service(t, mocks)

	template := findInput(svc, "template")
	connector, ok := nestedObject(template, "vpcAccess", "connector")
	if !ok || !connector.HasValue() {
		t.Fatal("M1 template.vpcAccess.connector missing — Cloud SQL / Memorystore / Redpanda + /health unreachable")
	}
	want := "projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector"
	if connector.StringValue() != want {
		t.Errorf("M1 vpcAccess.connector = %q, want %q", connector.StringValue(), want)
	}

	// ContainerPort 8080 is the HTTP/JSON surface health + SDK traffic use.
	ports, ok := nestedObject(template, "containers")
	if !ok || !ports.IsArray() || len(ports.ArrayValue()) == 0 {
		t.Fatal("M1 template.containers missing")
	}
	cp, ok := nestedObject(ports.ArrayValue()[0], "ports", "containerPort")
	if !ok || !cp.IsNumber() || cp.NumberValue() != 8080 {
		t.Errorf("M1 containerPort = %v, want 8080", cp)
	}
}

// AC4: M1 can read DB / Redis / Kafka creds from Secret Manager — exactly
// three secretAccessor bindings, all on M1's per-service runtime SA.
func TestM1ReadsThreeSecretsFromSecretManager(t *testing.T) {
	mocks, _, _ := runM1Compute(t)

	wantMember := "serviceAccount:dev-m1-assignment-run@kaizen-experimentation-dev.iam.gserviceaccount.com"
	allMembers := mocks.byType("gcp:secretmanager/secretIamMember:SecretIamMember")
	// Other Cloud Run services in NewCompute (M2-Orch, etc.) also create
	// secretAccessor bindings, so filter to M1's runtime SA before asserting
	// the count.
	var members []fsResource
	for _, m := range allMembers {
		if v := findInput(m, "member"); v.HasValue() && v.StringValue() == wantMember {
			members = append(members, m)
		}
	}
	if len(members) != 3 {
		t.Fatalf("M1 secretmanager.secretIamMember bindings = %d, want 3 (DB + Redis + Kafka)", len(members))
	}
	for _, m := range members {
		role := findInput(m, "role")
		if !role.HasValue() || role.StringValue() != "roles/secretmanager.secretAccessor" {
			t.Errorf("%s: role = %v, want roles/secretmanager.secretAccessor", m.Name, role)
		}
	}

	// And the runtime SA is the dedicated Workload Identity SA, not the
	// project default — locks the WI invariant for M1 specifically.
	var m1SA bool
	for _, sa := range mocks.byType("gcp:serviceaccount/account:Account") {
		if v := findInput(sa, "accountId"); v.HasValue() && v.StringValue() == "dev-m1-assignment-run" {
			m1SA = true
		}
	}
	if !m1SA {
		t.Error("no dedicated runtime service account dev-m1-assignment-run for M1")
	}

	// roles/cloudsql.client — the data-plane companion to the DB creds.
	var sawCloudSQL bool
	for _, pm := range mocks.byType("gcp:projects/iAMMember:IAMMember") {
		if v := findInput(pm, "role"); v.HasValue() && v.StringValue() == "roles/cloudsql.client" {
			if mv := findInput(pm, "member"); mv.HasValue() && mv.StringValue() == wantMember {
				sawCloudSQL = true
			}
		}
	}
	if !sawCloudSQL {
		t.Error("M1 missing roles/cloudsql.client project IAM binding on its runtime SA")
	}
}
