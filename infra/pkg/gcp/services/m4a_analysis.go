package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM4aAnalysis wires M4a Analysis (issue #492) onto Cloud Run.
// CPU-intensive batch (Rust gRPC). Elevated CPU/memory above the
// default Cloud Run sizing; gRPC startup probe verifies the standard
// gRPC Health Checking Protocol responds before traffic is routed.
// Not in the p99 < 5ms cold-start-sensitive set (only M1/M7 pin
// min-instances=1); batch analysis tolerates cold starts.
func NewM4aAnalysis(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	const m4aPort = 50053
	repoURL, ok := stages.CICD.RepositoryURLs["analysis"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM4aAnalysis: cicdOut.RepositoryURLs missing \"analysis\" repo for M4a (#492)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m4a-analysis",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: m4aPort,
			MinInstances:  0,
			// 2 vCPU / 4Gi mirrors the AWS Fargate M4a Tier-2 sizing intent.
			CPULimit:    "2",
			MemoryLimit: "4Gi",
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "DATABASE_ENDPOINT", Value: stages.DB.Endpoint},
				{Name: "DATA_BUCKET", Value: stages.Storage.DataBucketName},
				{Name: "DATA_BUCKET_URI", Value: stages.Storage.DataBucketRef},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
			},
			Buckets:      []pulumi.StringInput{stages.Storage.DataBucketName},
			ProjectRoles: []string{"roles/cloudsql.client"},
			HealthCheck: &compute.HealthProbe{
				Type:                "grpc",
				Port:                m4aPort,
				InitialDelaySeconds: 10,
				PeriodSeconds:       10,
				FailureThreshold:    6,
			},
		})
}
