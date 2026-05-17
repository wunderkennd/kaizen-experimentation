package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m2PipeStageOutputs returns the minimal StageOutputs M2-Pipeline reads —
// "pipeline" repo, Redpanda brokers + schema-registry URL, Kafka secret ref.
func m2PipeStageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"pipeline": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/pipeline").ToStringOutput(),
			},
		},
		Stream: types.StreamingOutputs{
			BootstrapBrokers:  pulumi.String("seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092").ToStringOutput(),
			SchemaRegistryUrl: pulumi.String("https://schema-registry.kaizen-dev.fmc.prd.cloud.redpanda.com:30081").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			KafkaSecretRef: pulumi.String("kaizen-dev-kafka").ToStringOutput(),
		},
	}
}

// TestM2Pipeline_Wiring asserts M2-Pipeline's Cloud Run shape — name "m2-pipeline"
// in Service Directory, port 50052 (M2PipelinePort), the elevated
// MaxInstances=100 ceiling (high-throughput Kafka producer).
func TestM2Pipeline_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM2Pipeline(ctx, scopedCfg(), scopedInputs(), m2PipeStageOutputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM2Pipeline failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m2-pipeline" {
		t.Errorf("service name = %q, want kaizen-dev-m2-pipeline", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != M2PipelinePort {
		t.Errorf("containerPort = %v, want %d", port, M2PipelinePort)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "pipeline") {
		t.Errorf("image = %q, want substring \"pipeline\"", img)
	}

	// MaxInstances=100 — high-throughput Kafka producer, raised above the
	// Cloud Run default ceiling.
	scaling := tmpl["scaling"].ObjectValue()
	if max := scaling["maxInstanceCount"]; !max.HasValue() || max.NumberValue() != M2PipelineMaxInstances {
		t.Errorf("M2-Pipe maxInstanceCount = %v, want %d", max, M2PipelineMaxInstances)
	}

	// SD service ID is "m2-pipeline" (matches AWS Cloud Map name).
	sdSvcs := mocks.byType("gcp:servicedirectory/service:Service")
	if len(sdSvcs) != 1 {
		t.Fatalf("expected 1 Service Directory service, got %d", len(sdSvcs))
	}
	if sid := sdSvcs[0].Inputs["serviceId"]; !sid.HasValue() || sid.StringValue() != "m2-pipeline" {
		t.Errorf("SD serviceId = %v, want \"m2-pipeline\"", sid)
	}
}

// TestM2Pipeline_MissingPipelineRepoFails asserts the missing-repo error path.
func TestM2Pipeline_MissingPipelineRepoFails(t *testing.T) {
	bad := m2PipeStageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM2Pipeline(ctx, scopedCfg(), scopedInputs(), bad)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "pipeline") {
		t.Errorf("expected missing-pipeline error, got %v", err)
	}
}
