// Package test — acceptance topology test for the GCP M2 Pipeline Cloud Run
// deployment (issue #489).
//
// M2 Pipeline is the Rust ingest service: a high-throughput Kafka producer
// that must reach Redpanda via the Serverless VPC Access connector. This
// test runs the gcp.NewCompute facade under pulumi.WithMocks (no GCP
// credentials) and asserts the three issue-#489 acceptance criteria at the
// topology layer — the layer the spec's Testing Strategy assigns to "every
// PR, no credentials" (the literal "200 health check" and "publishes to
// Redpanda end-to-end" criteria are exercised by the nightly preview and
// weekly smoke layers against a real GCP project):
//
//  1. M2 appears in ComputeOutputs.ServiceEndpoints["m2-pipeline"].
//  2. M2's container/health port is 50052 and it registers a Service
//     Directory endpoint so the platform health check + peer discovery
//     resolve it.
//  3. M2 is wired to Redpanda end-to-end: KAFKA_BROKERS + SCHEMA_REGISTRY_URL
//     env vars, the Kafka SASL secret mounted from Secret Manager, and the
//     roles/secretmanager.secretAccessor binding on M2's runtime SA that lets
//     it read those credentials.
package test

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m2ComputeMocks records every resource gcp.NewCompute registers and
// enriches the outputs the M4b + Cloud Run ApplyT chains depend on. It is a
// superset of computeMocks (gcp_compute_topology_test.go) because NewCompute
// builds the stateful M4b slice alongside the Cloud Run services.
type m2ComputeMocks struct {
	mu        sync.Mutex
	resources []recordedComputeResource
}

func (m *m2ComputeMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
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
	case "gcp:compute/address:Address":
		outputs["address"] = resource.NewStringProperty("10.0.16.42")
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/addresses/" + args.Name)
	case "gcp:compute/disk:Disk", "gcp:compute/instanceTemplate:InstanceTemplate":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/healthCheck:HealthCheck":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/healthChecks/" + args.Name)
	case "gcp:compute/instanceGroupManager:InstanceGroupManager":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:servicedirectory/service:Service":
		if v, ok := args.Inputs["serviceId"]; ok {
			outputs["serviceId"] = v
		}
		if v, ok := args.Inputs["namespace"]; ok {
			outputs["namespace"] = v
		}
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/services/" + args.Name)
	case "gcp:servicedirectory/endpoint:Endpoint":
		if v, ok := args.Inputs["address"]; ok {
			outputs["address"] = v
		}
		if v, ok := args.Inputs["port"]; ok {
			outputs["port"] = v
		}
	}
	return id, outputs, nil
}

func (m *m2ComputeMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *m2ComputeMocks) byType(typeToken string) []recordedComputeResource {
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

// runM2Compute runs gcp.NewCompute with a representative Phase-1 GCP stack
// (CICD pipeline repo, Redpanda streaming outputs, Secret Manager refs) and
// returns the recorded resources plus the ComputeOutputs.
func runM2Compute(t *testing.T) (*m2ComputeMocks, types.ComputeOutputs) {
	t.Helper()
	mocks := &m2ComputeMocks{}
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
			PrivateSubnetIds: pulumi.ToStringArrayOutput([]pulumi.StringOutput{
				pulumi.String("projects/kaizen-experimentation-dev/regions/us-central1/subnetworks/kaizen-dev-private").ToStringOutput(),
			}),
			ServiceDiscoveryId: pulumi.ID(
				"projects/kaizen-experimentation-dev/locations/us-central1/namespaces/kaizen-local").ToIDOutput(),
			VpcConnectorSelfLink: pulumi.String(
				"projects/kaizen-experimentation-dev/locations/us-central1/connectors/kaizen-vpc-connector").ToStringOutput(),
		}
		cicdOut := types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"pipeline": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/pipeline").ToStringOutput(),
				// NewCompute provisions every wired per-service Cloud Run service
				// in one call, so this fixture must satisfy each service's image
				// lookup even when the test is scoped to M2 Pipeline.
				"orchestration": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/orchestration").ToStringOutput(),
				"ui":            pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/ui").ToStringOutput(),
				"analysis":      pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/analysis").ToStringOutput(),
				"assignment":    pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/assignment").ToStringOutput(),
				"metrics":       pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/metrics").ToStringOutput(),
				"flags":         pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/flags").ToStringOutput(),
			},
		}
		dbOut := types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
		}
		cacheOut := types.CacheOutputs{
			Endpoint: pulumi.String("10.99.1.1:6379").ToStringOutput(),
		}
		streamOut := types.StreamingOutputs{
			BootstrapBrokers:  pulumi.String("seed-abc.redpanda.cloud:9092").ToStringOutput(),
			SchemaRegistryUrl: pulumi.String("https://seed-abc.redpanda.cloud:30081").ToStringOutput(),
			ClusterName:       pulumi.String("kaizen-dev").ToStringOutput(),
		}
		storageOut := types.StorageOutputs{
			DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
			DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
		}
		// SecretsOutputs.*Ref is the bare local secret ID (e.g.
		// "kaizen-dev-kafka"), matching what NewSecrets returns in production
		// — Cloud Run's secretKeyRef.secret expects exactly this form, NOT
		// the projects/<P>/secrets/<S> path or version-qualified accessor.
		secretsOut := types.SecretsOutputs{
			KafkaSecretRef:    pulumi.String("kaizen-dev-kafka").ToStringOutput(),
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
			RedisSecretRef:    pulumi.String("kaizen-dev-redis").ToStringOutput(),
			AuthSecretRef:     pulumi.String("kaizen-dev-auth").ToStringOutput(),
		}

		var err error
		out, err = gcp.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, streamOut, secretsOut, storageOut, cacheOut)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("gcp.NewCompute failed: %v", err)
	}
	return mocks, out
}

// findM2Service returns the recorded Cloud Run service resource for
// m2-pipeline (Pulumi name kaizen-<env>-m2-pipeline-run).
func findM2Service(t *testing.T, mocks *m2ComputeMocks) recordedComputeResource {
	t.Helper()
	for _, r := range mocks.byType("gcp:cloudrunv2/service:Service") {
		if v, ok := r.Inputs["name"]; ok && v.HasValue() &&
			v.StringValue() == "kaizen-dev-m2-pipeline" {
			return r
		}
	}
	t.Fatal("no Cloud Run service named kaizen-dev-m2-pipeline registered")
	return recordedComputeResource{}
}

// ---------------------------------------------------------------------------
// Acceptance criterion 1: M2 in ComputeOutputs.ServiceEndpoints["m2-pipeline"]
// ---------------------------------------------------------------------------

func TestM2PipelineInServiceEndpoints(t *testing.T) {
	_, out := runM2Compute(t)
	if out.ServiceEndpoints == nil {
		t.Fatal("ComputeOutputs.ServiceEndpoints is nil")
	}
	if _, ok := out.ServiceEndpoints["m2-pipeline"]; !ok {
		t.Errorf("ServiceEndpoints missing key %q; keys present: %v",
			"m2-pipeline", keysOfOutputs(out.ServiceEndpoints))
	}
	if _, ok := out.ServiceArns["m2-pipeline"]; !ok {
		t.Errorf("ServiceArns missing key %q", "m2-pipeline")
	}
}

func keysOfOutputs(m map[string]pulumi.StringOutput) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}

// ---------------------------------------------------------------------------
// Acceptance criterion 2: health/container port + Service Directory endpoint
// ---------------------------------------------------------------------------

func TestM2PipelineContainerPortAndDiscovery(t *testing.T) {
	mocks, _ := runM2Compute(t)
	svc := findM2Service(t, mocks)

	tmpl, ok := svc.Inputs["template"]
	if !ok || !tmpl.IsObject() {
		t.Fatal("m2 Cloud Run service missing template")
	}
	containers := tmpl.ObjectValue()["containers"]
	if !containers.IsArray() || len(containers.ArrayValue()) == 0 {
		t.Fatal("m2 template has no containers")
	}
	ports := containers.ArrayValue()[0].ObjectValue()["ports"]
	if !ports.IsObject() {
		t.Fatal("m2 container missing ports")
	}
	cp := ports.ObjectValue()["containerPort"]
	if !cp.IsNumber() || cp.NumberValue() != 50052 {
		t.Errorf("m2 containerPort = %v, want 50052 (M2 gRPC ingest port)", cp)
	}

	// Service Directory service + endpoint so the platform health check and
	// peer services can resolve m2-pipeline.
	var sdFound bool
	for _, sd := range mocks.byType("gcp:servicedirectory/service:Service") {
		if v, ok := sd.Inputs["serviceId"]; ok && v.StringValue() == "m2-pipeline" {
			sdFound = true
		}
	}
	if !sdFound {
		t.Error("no Service Directory service registered with serviceId m2-pipeline")
	}
	if len(mocks.byType("gcp:servicedirectory/endpoint:Endpoint")) == 0 {
		t.Error("m2-pipeline registered no Service Directory endpoint")
	}
}

// ---------------------------------------------------------------------------
// Acceptance criterion 3: Redpanda end-to-end wiring (env + secret + IAM)
// ---------------------------------------------------------------------------

func TestM2PipelineRedpandaWiring(t *testing.T) {
	mocks, _ := runM2Compute(t)
	svc := findM2Service(t, mocks)

	envs := svc.Inputs["template"].ObjectValue()["containers"].
		ArrayValue()[0].ObjectValue()["envs"]
	if !envs.IsArray() {
		t.Fatal("m2 container has no envs array")
	}

	literals := map[string]string{}
	secretEnvs := map[string]string{} // env name -> secret id
	for _, e := range envs.ArrayValue() {
		eo := e.ObjectValue()
		name := eo["name"].StringValue()
		if val, ok := eo["value"]; ok && val.HasValue() {
			literals[name] = val.StringValue()
			continue
		}
		if vs, ok := eo["valueSource"]; ok && vs.IsObject() {
			skr := vs.ObjectValue()["secretKeyRef"]
			if skr.IsObject() {
				secretEnvs[name] = skr.ObjectValue()["secret"].StringValue()
			}
		}
	}

	// Redpanda bootstrap brokers + Schema Registry URL as literal env vars.
	if got := literals["KAFKA_BROKERS"]; got != "seed-abc.redpanda.cloud:9092" {
		t.Errorf("KAFKA_BROKERS = %q, want the Redpanda bootstrap brokers", got)
	}
	if got := literals["SCHEMA_REGISTRY_URL"]; got != "https://seed-abc.redpanda.cloud:30081" {
		t.Errorf("SCHEMA_REGISTRY_URL = %q, want the Redpanda schema registry URL", got)
	}

	// Kafka SASL credentials mounted from Secret Manager.
	kafkaSecret, ok := secretEnvs["KAFKA_SECRET"]
	if !ok {
		t.Fatalf("KAFKA_SECRET not mounted from Secret Manager; secret envs: %v", secretEnvs)
	}
	// The factory's secretKeyRef.secret takes the bare local secret ID
	// (resolved via secretIDForRef → secrets.SecretID(cfg, "kafka")).
	if kafkaSecret != "kaizen-dev-kafka" {
		t.Errorf("KAFKA_SECRET secret ref = %q, want the kaizen-dev-kafka Secret Manager secret", kafkaSecret)
	}

	// IAM: M2's runtime SA must be able to READ the Kafka secret.
	wantMember := "serviceAccount:dev-m2-pipeline-run@kaizen-experimentation-dev.iam.gserviceaccount.com"
	var bound bool
	for _, b := range mocks.byType("gcp:secretmanager/secretIamMember:SecretIamMember") {
		role := b.Inputs["role"]
		member := b.Inputs["member"]
		if role.HasValue() && role.StringValue() == "roles/secretmanager.secretAccessor" &&
			member.HasValue() && member.StringValue() == wantMember {
			bound = true
		}
	}
	if !bound {
		t.Errorf("no roles/secretmanager.secretAccessor binding for %s on the Kafka secret", wantMember)
	}
}

// ---------------------------------------------------------------------------
// M2 is a high-throughput producer: default min-instances, elevated max.
// ---------------------------------------------------------------------------

func TestM2PipelineScaling(t *testing.T) {
	mocks, _ := runM2Compute(t)
	svc := findM2Service(t, mocks)

	scaling := svc.Inputs["template"].ObjectValue()["scaling"]
	if !scaling.IsObject() {
		t.Fatal("m2 template missing scaling block")
	}
	min := scaling.ObjectValue()["minInstanceCount"]
	if !min.IsNumber() || min.NumberValue() != 0 {
		t.Errorf("m2 minInstanceCount = %v, want 0 (default — M2 has no cold-start SLA)", min)
	}
	max := scaling.ObjectValue()["maxInstanceCount"]
	if !max.IsNumber() || max.NumberValue() != 100 {
		t.Errorf("m2 maxInstanceCount = %v, want 100 (elevated for Kafka-producer throughput)", max)
	}
}
