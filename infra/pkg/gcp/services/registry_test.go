package services

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// fullStageOutputs is the superset every factory consumes — used only by
// the registry-walk aggregator test below.
func fullStageOutputs() StageOutputs {
	repos := map[string]pulumi.StringOutput{}
	for _, k := range []string{"assignment", "orchestration", "ui", "analysis", "metrics", "flags", "pipeline", "management"} {
		repos[k] = pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/" + k).ToStringOutput()
	}
	return StageOutputs{
		CICD: types.CICDOutputs{RepositoryURLs: repos},
		DB:   types.DatabaseOutputs{Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput()},
		Cache: types.CacheOutputs{
			Endpoint: pulumi.String("redis://10.99.1.1:6379").ToStringOutput(),
		},
		Stream: types.StreamingOutputs{
			BootstrapBrokers:  pulumi.String("seed-0.redpanda.cloud:9092").ToStringOutput(),
			SchemaRegistryUrl: pulumi.String("https://schema-registry.redpanda.cloud:30081").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef:    pulumi.String("kaizen-dev-kafka").ToStringOutput(),
			RedisSecretRef:    pulumi.String("kaizen-dev-redis").ToStringOutput(),
			AuthSecretRef:     pulumi.String("kaizen-dev-auth").ToStringOutput(),
		},
		Storage: types.StorageOutputs{
			DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
			DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
		},
	}
}

// TestRegistry_WalkProducesEveryService asserts the walker produces exactly
// the 9 Cloud Run services every per-service issue is responsible for. If a
// future service lands, this count is the canonical place to bump.
func TestRegistry_WalkProducesEveryService(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		// Reproduce the registry from gcp.NewCompute — including the M1/M6
		// closure capture of m4bEndpoint (use a constant for testing).
		m4bEndpoint := pulumi.String("10.0.16.42:50054").ToStringOutput()
		registry := []RegistryEntry{
			{Key: "preview-canary", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, _ StageOutputs) (*compute.CloudRunService, error) {
				return NewCanary(ctx, cfg, in)
			}},
			{Key: "m2-orch", Factory: NewM2Orchestration},
			{Key: "m6", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, s StageOutputs) (*compute.CloudRunService, error) {
				return NewM6UI(ctx, cfg, in, s, m4bEndpoint)
			}},
			{Key: "m4a", Factory: NewM4aAnalysis},
			{Key: "m1", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, s StageOutputs) (*compute.CloudRunService, error) {
				return NewM1Assignment(ctx, cfg, in, s, m4bEndpoint)
			}},
			{Key: "m3", Factory: NewM3Metrics},
			{Key: "m7", Factory: NewM7Flags},
			{Key: "m2-pipeline", Factory: NewM2Pipeline},
			{Key: "m5", Factory: NewM5Management},
		}
		out, err := Walk(ctx, scopedCfg(), scopedInputs(), fullStageOutputs(), registry)
		if err != nil {
			return err
		}
		if got := len(out); got != 9 {
			t.Errorf("Walk produced %d services, want 9", got)
		}
		for _, key := range []string{"preview-canary", "m1", "m2-orch", "m2-pipeline", "m3", "m4a", "m5", "m6", "m7"} {
			if _, ok := out[key]; !ok {
				t.Errorf("Walk missing service %q", key)
			}
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("registry Walk failed: %v", err)
	}

	if got := len(mocks.byType("gcp:cloudrunv2/service:Service")); got != 9 {
		t.Errorf("expected 9 Cloud Run services registered, got %d", got)
	}
	if got := len(mocks.byType("gcp:servicedirectory/service:Service")); got != 9 {
		t.Errorf("expected 9 Service Directory services registered, got %d", got)
	}
}
