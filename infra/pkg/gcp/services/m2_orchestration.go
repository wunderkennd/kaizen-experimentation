package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM2Orchestration wires M2 Orchestration (issue #490) onto Cloud Run.
// Stateless coordinator. Needs Cloud SQL for orchestration state and
// Redpanda for event flow. Service name "m2-orchestration" (matches the
// AWS Cloud Map name + the dev-m2-orchestration-run SA convention); the
// ServiceEndpoints map key is "m2-orch" (matches the AWS service key and
// the #490 acceptance criterion). Default min-instances (0) — the spec's
// Compute Model marks M2-Orch a stateless orchestrator, NOT an M1/M7
// cold-start-sensitive service.
func NewM2Orchestration(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["orchestration"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM2Orchestration: cicdOut.RepositoryURLs missing \"orchestration\" key required for M2-Orch deploy (#490)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m2-orchestration",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: 50058,
			MinInstances:  0,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "LOG_LEVEL", Value: pulumi.String("info")},
				// Cloud SQL host:port — reachable from the container only
				// through the Serverless VPC Access connector wired by the
				// factory (acceptance criterion #3).
				{Name: "DATABASE_ENDPOINT", Value: stages.DB.Endpoint},
				// Redpanda Kafka-protocol bootstrap brokers for event flow.
				{Name: "KAFKA_BOOTSTRAP_BROKERS", Value: stages.Stream.BootstrapBrokers},
			},
			Secrets: []compute.SecretEnv{
				// secretsOut.*SecretRef is the bare projects/<P>/secrets/<S>
				// path on GCP (see gcp.NewSecrets contract note); the factory
				// grants roles/secretmanager.secretAccessor on each — i.e.
				// "read DB creds, read Kafka creds".
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
				{EnvName: "KAFKA_SECRET", SecretID: stages.Secrets.KafkaSecretRef, Version: "latest"},
			},
			// Cloud SQL connection scope. The secret-accessor bindings above
			// cover credential reads; this grants the IAM scope to open the
			// connection itself.
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
