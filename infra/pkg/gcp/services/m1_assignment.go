package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM1Assignment wires M1 Assignment (issue #488) onto Cloud Run. M1 carries
// the platform's strictest latency budget (p99 < 5ms) — MinInstances=1 keeps
// one warm instance so Cloud Run never cold-starts a request.
//
// Takes m4bEndpoint as an extra argument because M4b is not a Cloud Run service
// and therefore not part of StageOutputs (it's the stateful GCE/MIG slice
// constructed in NewCompute's preamble). M1 reads it as M4B_ADDR so the
// assignment service can delegate to the bandit policy at request time.
func NewM1Assignment(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
	m4bEndpoint pulumi.StringInput,
) (*compute.CloudRunService, error) {
	assignmentRepo, ok := stages.CICD.RepositoryURLs["assignment"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM1Assignment: CICDOutputs.RepositoryURLs is missing the \"assignment\" repo (required for the M1 image)")
	}
	m1Image := assignmentRepo.ApplyT(func(repo string) string {
		return repo + ":latest"
	}).(pulumi.StringOutput)

	return compute.NewCloudRunService(ctx, cfg, inputs, "m1-assignment",
		&compute.Options{
			Image:         m1Image,
			ContainerPort: 8080,
			MinInstances:  1, // p99 < 5ms SLA — no cold starts.
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "GRPC_ADDR", Value: pulumi.String("0.0.0.0:50051")},
				{Name: "HTTP_ADDR", Value: pulumi.String("0.0.0.0:8080")},
				{Name: "KAFKA_BOOTSTRAP_BROKERS", Value: stages.Stream.BootstrapBrokers},
				{Name: "M4B_ADDR", Value: m4bEndpoint},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
				{EnvName: "REDIS_SECRET", SecretID: stages.Secrets.RedisSecretRef, Version: "latest"},
				{EnvName: "KAFKA_SECRET", SecretID: stages.Secrets.KafkaSecretRef, Version: "latest"},
			},
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
