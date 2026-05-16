package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
	"github.com/kaizen-experimentation/infra/pkg/gcp/secrets"
)

// M5ManagementPort is the HTTP/gRPC port M5 Management binds to. Matches the
// AWS service contract (pkg/aws/compute/services.go) and the value baked into
// the experimentation-management container per ADR-025.
const M5ManagementPort = 50055

// NewM5Management wires M5 Management (Rust experimentation-management,
// ADR-025) onto Cloud Run via the shared factory. CRUD/Postgres + Kafka
// publisher for lifecycle events. Env-var contract mirrors the AWS service
// contract so the same image runs unmodified on either cloud; credentials
// arrive via Secret Manager refs (factory mounts secretKeyRef + auto-creates
// secretAccessor IAM binding on the per-service SA).
func NewM5Management(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["management"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.NewCompute: cicdOut.RepositoryURLs is missing the \"management\" Artifact Registry repo required by M5")
	}

	// secretIDForRef returns the bare local secret ID Cloud Run's
	// secretKeyRef.secret + the secretAccessor IAM binding expect, but routes
	// the value through an ApplyT on the corresponding Stage-4 *SecretRef
	// output so M5's secret mounts + IAM bindings are ordered after the
	// Secret Manager secret + version actually exist. The string returned is
	// deterministic (secrets.SecretID is a pure function of cfg.Env +
	// component name); the ApplyT exists solely to thread the dependency
	// edge — see #542 refactor for unifying this across services.
	secretIDForRef := func(ref pulumi.StringOutput, component string) pulumi.StringInput {
		return ref.ApplyT(func(string) string {
			return secrets.SecretID(cfg, component)
		}).(pulumi.StringOutput)
	}

	return compute.NewCloudRunService(ctx, cfg, inputs, "m5-management",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: M5ManagementPort,
			MinInstances:  0, // CRUD/Postgres — no p99 < 5ms SLA.
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "DATABASE_ENDPOINT", Value: stages.DB.Endpoint},
				{Name: "REDIS_ENDPOINT", Value: stages.Cache.Endpoint},
				{Name: "KAFKA_BOOTSTRAP_BROKERS", Value: stages.Stream.BootstrapBrokers},
				{Name: "OTEL_SERVICE_NAME", Value: pulumi.String("m5-management")},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: secretIDForRef(stages.Secrets.DatabaseSecretRef, "database"), Version: "latest"},
				{EnvName: "KAFKA_SECRET", SecretID: secretIDForRef(stages.Secrets.KafkaSecretRef, "kafka"), Version: "latest"},
				{EnvName: "REDIS_SECRET", SecretID: secretIDForRef(stages.Secrets.RedisSecretRef, "redis"), Version: "latest"},
				{EnvName: "AUTH_SECRET", SecretID: secretIDForRef(stages.Secrets.AuthSecretRef, "auth"), Version: "latest"},
			},
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
