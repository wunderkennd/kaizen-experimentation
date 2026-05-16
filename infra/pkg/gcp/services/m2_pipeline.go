package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// M2PipelinePort is the gRPC ingest port M2 Pipeline binds to. Matches the
// AWS Cloud Map registration (pkg/aws/compute/services.go: m2-pipeline →
// 50052) and the value the experimentation-pipeline container listens on.
const M2PipelinePort = 50052

// M2PipelineMaxInstances caps M2's Cloud Run autoscaling. M2 is a high-
// throughput Kafka producer, so its ceiling is raised well above the
// cost-control default; floor stays at 0 (M2 carries no p99 cold-start SLA).
const M2PipelineMaxInstances = 100

// NewM2Pipeline wires M2 Pipeline (Rust experimentation-ingest) onto
// Cloud Run via the shared factory. Issue #489. The env-var contract mirrors
// crates/experimentation-pipeline/src/main.rs (KAFKA_BROKERS) and the AWS
// service contract so the same image runs unmodified on both clouds.
func NewM2Pipeline(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["pipeline"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.NewCompute: cicdOut.RepositoryURLs is missing the \"pipeline\" Artifact Registry repo required by M2")
	}

	return compute.NewCloudRunService(ctx, cfg, inputs, "m2-pipeline",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: M2PipelinePort,
			MinInstances:  0,
			MaxInstances:  M2PipelineMaxInstances,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "KAFKA_BROKERS", Value: stages.Stream.BootstrapBrokers},
				{Name: "SCHEMA_REGISTRY_URL", Value: stages.Stream.SchemaRegistryUrl},
				{Name: "OTEL_SERVICE_NAME", Value: pulumi.String("m2-pipeline")},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "KAFKA_SECRET", SecretID: stages.Secrets.KafkaSecretRef, Version: "latest"},
			},
		})
}
