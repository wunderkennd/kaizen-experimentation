package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m6StageOutputs returns the minimal StageOutputs M6 reads — the "ui" repo
// and the Auth secret ref for SSR session encryption.
func m6StageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"ui": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/ui").ToStringOutput(),
			},
		},
		Secrets: types.SecretsOutputs{
			AuthSecretRef: pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-auth").ToStringOutput(),
		},
	}
}

// TestM6UI_Wiring asserts M6's Cloud Run shape — name, Next.js SSR port 3000,
// image substring, and that AUTH_SECRET is mounted from Secret Manager.
func TestM6UI_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		m4bEndpoint := pulumi.String("10.0.16.42:50054").ToStringOutput()
		_, err := NewM6UI(ctx, scopedCfg(), scopedInputs(), m6StageOutputs(), m4bEndpoint)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM6UI failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m6-ui" {
		t.Errorf("service name = %q, want kaizen-dev-m6-ui", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != 3000 {
		t.Errorf("containerPort = %v, want 3000 (Next.js SSR port)", port)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "ui") {
		t.Errorf("image = %q, want substring \"ui\"", img)
	}

	// AUTH_SECRET must be mounted from Secret Manager (secretKeyRef), not as a
	// literal env value.
	envs := c["envs"].ArrayValue()
	foundAuthSecret := false
	for _, e := range envs {
		eo := e.ObjectValue()
		if eo["name"].StringValue() != "AUTH_SECRET" {
			continue
		}
		vs, ok := eo["valueSource"]
		if !ok || !vs.IsObject() {
			t.Fatal("AUTH_SECRET missing valueSource (must be secretKeyRef)")
		}
		if _, ok := vs.ObjectValue()["secretKeyRef"]; !ok {
			t.Fatal("AUTH_SECRET valueSource missing secretKeyRef")
		}
		foundAuthSecret = true
	}
	if !foundAuthSecret {
		t.Errorf("AUTH_SECRET env var missing from M6 container")
	}
}

// TestM6UI_MissingUIRepoFails asserts the missing-repo error path.
func TestM6UI_MissingUIRepoFails(t *testing.T) {
	bad := m6StageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		m4bEndpoint := pulumi.String("10.0.16.42:50054").ToStringOutput()
		_, err := NewM6UI(ctx, scopedCfg(), scopedInputs(), bad, m4bEndpoint)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "ui") {
		t.Errorf("expected missing-ui error, got %v", err)
	}
}
