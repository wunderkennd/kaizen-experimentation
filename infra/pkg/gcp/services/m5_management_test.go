package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m5StageOutputs returns the minimal StageOutputs M5 reads — "management" repo,
// Cloud SQL + Redis endpoints, Redpanda brokers, and DB/Kafka/Redis/Auth refs.
//
// The SecretRef values are the *Stage-4 outputs* (full
// projects/<P>/secrets/<S> path on the AWS-shaped contract). M5's
// secretIDForRef closure routes them through an ApplyT that returns the bare
// local secret ID (e.g. "kaizen-dev-database") — that's what TestM5
// asserts on, NOT the full projects/<P>/secrets/<S> path.
func m5StageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"management": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/management").ToStringOutput(),
			},
		},
		DB: types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
		},
		Cache: types.CacheOutputs{
			Endpoint: pulumi.String("redis://10.99.1.1:6379").ToStringOutput(),
		},
		Stream: types.StreamingOutputs{
			BootstrapBrokers: pulumi.String("seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef:    pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-kafka").ToStringOutput(),
			RedisSecretRef:    pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-redis").ToStringOutput(),
			AuthSecretRef:     pulumi.String("projects/kaizen-experimentation-dev/secrets/kaizen-dev-auth").ToStringOutput(),
		},
	}
}

// TestM5Management_Wiring asserts M5's Cloud Run shape — name, port, image,
// and (critically) that the secretIDForRef closure produces bare local secret
// IDs ("kaizen-dev-database" / "kaizen-dev-kafka" / "kaizen-dev-redis" /
// "kaizen-dev-auth") rather than the full projects/<P>/secrets/<S> path the
// stage outputs use upstream.
func TestM5Management_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM5Management(ctx, scopedCfg(), scopedInputs(), m5StageOutputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM5Management failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m5-management" {
		t.Errorf("service name = %q, want kaizen-dev-m5-management", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != M5ManagementPort {
		t.Errorf("containerPort = %v, want %d", port, M5ManagementPort)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "management") {
		t.Errorf("image = %q, want substring \"management\"", img)
	}

	// secretIDForRef must produce bare local secret IDs (not the full
	// projects/<P>/secrets/<S> path the Stage-4 *SecretRef outputs carry).
	envs := c["envs"].ArrayValue()
	wantSecretIDs := map[string]string{
		"DATABASE_SECRET": "kaizen-dev-database",
		"KAFKA_SECRET":    "kaizen-dev-kafka",
		"REDIS_SECRET":    "kaizen-dev-redis",
		"AUTH_SECRET":     "kaizen-dev-auth",
	}
	gotSecretIDs := map[string]string{}
	for _, e := range envs {
		eo := e.ObjectValue()
		name := eo["name"].StringValue()
		if vs, ok := eo["valueSource"]; ok && vs.IsObject() {
			if skr, ok := vs.ObjectValue()["secretKeyRef"]; ok && skr.IsObject() {
				if s := skr.ObjectValue()["secret"]; s.HasValue() {
					gotSecretIDs[name] = s.StringValue()
				}
			}
		}
	}
	for env, want := range wantSecretIDs {
		got, ok := gotSecretIDs[env]
		if !ok {
			t.Errorf("env %q missing secretKeyRef.secret", env)
			continue
		}
		if got != want {
			t.Errorf("env %q secret = %q, want bare local ID %q (NOT projects/<P>/secrets/<S>)", env, got, want)
		}
		if strings.HasPrefix(got, "projects/") {
			t.Errorf("env %q secret = %q must NOT be the full projects/<P>/secrets/<S> path", env, got)
		}
	}
}

// TestM5Management_MissingManagementRepoFails asserts the missing-repo error path.
func TestM5Management_MissingManagementRepoFails(t *testing.T) {
	bad := m5StageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM5Management(ctx, scopedCfg(), scopedInputs(), bad)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "management") {
		t.Errorf("expected missing-management error, got %v", err)
	}
}
