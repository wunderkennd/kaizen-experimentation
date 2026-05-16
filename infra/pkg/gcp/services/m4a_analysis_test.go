package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m4aStageOutputs returns the minimal StageOutputs M4a reads — "analysis" repo,
// Cloud SQL endpoint, data bucket, and DB secret ref.
func m4aStageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"analysis": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/analysis").ToStringOutput(),
			},
		},
		DB: types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
		},
		Storage: types.StorageOutputs{
			DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
			DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
		},
	}
}

// TestM4aAnalysis_Wiring asserts M4a's Cloud Run shape — name "m4a-analysis",
// port 50053, the elevated CPU=2/memory=4Gi limits, and the gRPC startup probe.
func TestM4aAnalysis_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4aAnalysis(ctx, scopedCfg(), scopedInputs(), m4aStageOutputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM4aAnalysis failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m4a-analysis" {
		t.Errorf("service name = %q, want kaizen-dev-m4a-analysis", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != 50053 {
		t.Errorf("containerPort = %v, want 50053", port)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "analysis") {
		t.Errorf("image = %q, want substring \"analysis\"", img)
	}

	// Elevated CPU/memory above the Cloud Run default sizing.
	res := c["resources"].ObjectValue()
	limits := res["limits"].ObjectValue()
	if cpu := limits["cpu"]; !cpu.HasValue() || cpu.StringValue() != "2" {
		t.Errorf("M4a CPU limit = %v, want \"2\"", cpu)
	}
	if mem := limits["memory"]; !mem.HasValue() || mem.StringValue() != "4Gi" {
		t.Errorf("M4a memory limit = %v, want \"4Gi\"", mem)
	}

	// gRPC startup probe on the container port — health check is verified
	// end-to-end via the standard gRPC Health Checking Protocol.
	probe := c["startupProbe"].ObjectValue()
	grpc := probe["grpc"].ObjectValue()
	if probePort := grpc["port"]; !probePort.HasValue() || probePort.NumberValue() != 50053 {
		t.Errorf("M4a startupProbe.grpc.port = %v, want 50053", probePort)
	}
}

// TestM4aAnalysis_MissingAnalysisRepoFails asserts the missing-repo error path.
func TestM4aAnalysis_MissingAnalysisRepoFails(t *testing.T) {
	bad := m4aStageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4aAnalysis(ctx, scopedCfg(), scopedInputs(), bad)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "analysis") {
		t.Errorf("expected missing-analysis error, got %v", err)
	}
}
