package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m2OrchStageOutputs returns the minimal StageOutputs M2-Orch reads — the
// "orchestration" repo, Cloud SQL endpoint, Redpanda brokers, and DB+Kafka
// secret refs.
func m2OrchStageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"orchestration": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/orchestration").ToStringOutput(),
			},
		},
		DB: types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
		},
		Stream: types.StreamingOutputs{
			BootstrapBrokers: pulumi.String("seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef:    pulumi.String("kaizen-dev-kafka").ToStringOutput(),
		},
	}
}

// TestM2Orchestration_Wiring asserts M2-Orch's Cloud Run shape — Cloud Run
// service name is "kaizen-dev-m2-orchestration" (matches the AWS Cloud Map
// name + dev-m2-orchestration-run SA convention), port 50058, image substring.
func TestM2Orchestration_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM2Orchestration(ctx, scopedCfg(), scopedInputs(), m2OrchStageOutputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM2Orchestration failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m2-orchestration" {
		t.Errorf("service name = %q, want kaizen-dev-m2-orchestration", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != 50058 {
		t.Errorf("containerPort = %v, want 50058", port)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "orchestration") {
		t.Errorf("image = %q, want substring \"orchestration\"", img)
	}

	// M2-Orch is a stateless orchestrator (not p99-sensitive) — default min.
	scaling := tmpl["scaling"].ObjectValue()
	if min := scaling["minInstanceCount"]; !min.HasValue() || min.NumberValue() != 0 {
		t.Errorf("M2-Orch minInstanceCount = %v, want 0 (stateless orchestrator)", min)
	}

	// Service Directory registers under the "m2-orchestration" service name
	// (matches the AWS Cloud Map registration).
	sdSvcs := mocks.byType("gcp:servicedirectory/service:Service")
	if len(sdSvcs) != 1 {
		t.Fatalf("expected 1 Service Directory service, got %d", len(sdSvcs))
	}
	if sid := sdSvcs[0].Inputs["serviceId"]; !sid.HasValue() || sid.StringValue() != "m2-orchestration" {
		t.Errorf("SD serviceId = %v, want \"m2-orchestration\"", sid)
	}
}

// TestM2Orchestration_MissingOrchestrationRepoFails asserts the missing-repo
// error path.
func TestM2Orchestration_MissingOrchestrationRepoFails(t *testing.T) {
	bad := m2OrchStageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM2Orchestration(ctx, scopedCfg(), scopedInputs(), bad)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "orchestration") {
		t.Errorf("expected missing-orchestration error, got %v", err)
	}
}
