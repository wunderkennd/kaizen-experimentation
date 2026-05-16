package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m7StageOutputs returns the minimal StageOutputs M7 reads — "flags" repo,
// Cloud SQL + Redis endpoints, DB + Redis secret refs.
func m7StageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"flags": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/flags").ToStringOutput(),
			},
		},
		DB: types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
		},
		Cache: types.CacheOutputs{
			Endpoint: pulumi.String("redis://10.99.1.1:6379").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
			RedisSecretRef:    pulumi.String("kaizen-dev-redis").ToStringOutput(),
		},
	}
}

// TestM7Flags_Wiring asserts M7's Cloud Run shape — name, port 50057, image,
// and the p99 < 5ms MinInstances=1 pin (parity with M1).
func TestM7Flags_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM7Flags(ctx, scopedCfg(), scopedInputs(), m7StageOutputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM7Flags failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m7-flags" {
		t.Errorf("service name = %q, want kaizen-dev-m7-flags", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != 50057 {
		t.Errorf("containerPort = %v, want 50057", port)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "flags") {
		t.Errorf("image = %q, want substring \"flags\"", img)
	}

	// p99 < 5ms SLA: MinInstances pinned to 1 (parity with M1).
	scaling := tmpl["scaling"].ObjectValue()
	if min := scaling["minInstanceCount"]; !min.HasValue() || min.NumberValue() != 1 {
		t.Errorf("M7 minInstanceCount = %v, want 1 (p99 < 5ms SLA, parity with M1)", min)
	}
}

// TestM7Flags_MissingFlagsRepoFails asserts the missing-repo error path.
func TestM7Flags_MissingFlagsRepoFails(t *testing.T) {
	bad := m7StageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM7Flags(ctx, scopedCfg(), scopedInputs(), bad)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "flags") {
		t.Errorf("expected missing-flags error, got %v", err)
	}
}
