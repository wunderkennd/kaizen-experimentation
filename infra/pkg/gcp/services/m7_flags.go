package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM7Flags wires M7 Flags (issue #495) onto Cloud Run.
//
// Rust feature-flag service (ADR-024) on its gRPC port 50057. Same SLA
// profile as M1: min-instances=1 keeps a warm instance so requests never
// pay a Cloud Run cold start (spec Compute Model → Cold starts;
// p99 < 5ms). Image pulled from the "flags" Artifact Registry repo.
//
// SecretRef values from gcp.NewSecrets are already the bare
// `projects/<P>/secrets/<S>` path that Cloud Run's secretKeyRef and
// Secret Manager IAM bindings expect (see gcp.NewSecrets contract note);
// they're passed through directly without trimming, matching the M1/M2-
// Orch/M3 convention above.
func NewM7Flags(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	flagsRepo, ok := stages.CICD.RepositoryURLs["flags"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM7Flags: CICDOutputs.RepositoryURLs missing \"flags\" repo for M7 — " +
				"the CICD stage must run before compute")
	}

	return compute.NewCloudRunService(ctx, cfg, inputs, "m7-flags",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", flagsRepo),
			ContainerPort: 50057,
			MinInstances:  1, // p99 < 5ms SLA (parity with M1).
			MaxInstances:  10,
			EnvVars: []compute.EnvVar{
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "DATABASE_ENDPOINT", Value: stages.DB.Endpoint},
				{Name: "REDIS_ENDPOINT", Value: stages.Cache.Endpoint},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
				{EnvName: "REDIS_SECRET", SecretID: stages.Secrets.RedisSecretRef, Version: "latest"},
			},
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
