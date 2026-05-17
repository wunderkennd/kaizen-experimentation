package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m1StageOutputs returns the minimal StageOutputs M1 reads: the "assignment"
// Artifact Registry repo, plus DB/Redis/Kafka secret refs and the Redpanda
// bootstrap brokers. No other service's repo keys.
func m1StageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"assignment": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/assignment").ToStringOutput(),
			},
		},
		Stream: types.StreamingOutputs{
			BootstrapBrokers: pulumi.String("seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef:    pulumi.String("kaizen-dev-kafka").ToStringOutput(),
			RedisSecretRef:    pulumi.String("kaizen-dev-redis").ToStringOutput(),
		},
	}
}

// TestM1Assignment_Wiring asserts M1's Cloud Run shape — name, port, image,
// and the p99 < 5ms MinInstances=1 pin (parity with the M1 spec).
func TestM1Assignment_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		m4bEndpoint := pulumi.String("10.0.16.42:50054").ToStringOutput()
		_, err := NewM1Assignment(ctx, scopedCfg(), scopedInputs(), m1StageOutputs(), m4bEndpoint)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM1Assignment failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m1-assignment" {
		t.Errorf("service name = %q, want kaizen-dev-m1-assignment", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != 8080 {
		t.Errorf("containerPort = %v, want 8080", port)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "assignment") {
		t.Errorf("image = %q, want substring \"assignment\"", img)
	}

	// p99 < 5ms SLA: MinInstances must be pinned to 1 (no cold starts).
	scaling := tmpl["scaling"].ObjectValue()
	if min := scaling["minInstanceCount"]; !min.HasValue() || min.NumberValue() != 1 {
		t.Errorf("M1 minInstanceCount = %v, want 1 (p99 < 5ms SLA — no cold starts)", min)
	}
}

// TestM1Assignment_MissingAssignmentRepoFails asserts the missing-repo error path.
func TestM1Assignment_MissingAssignmentRepoFails(t *testing.T) {
	bad := m1StageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		m4bEndpoint := pulumi.String("10.0.16.42:50054").ToStringOutput()
		_, err := NewM1Assignment(ctx, scopedCfg(), scopedInputs(), bad, m4bEndpoint)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "assignment") {
		t.Errorf("expected missing-assignment error, got %v", err)
	}
}
