package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM3Metrics wires M3 Metrics (Go service, services/metrics) onto Cloud Run.
// Three runtime paths:
//  1. Spark SQL orchestration → Delta Lake on GCS (reads metric defs from
//     Cloud SQL).
//  2. Guardrail alerts published to Kafka topic "guardrail_alerts"
//     (services/metrics/cmd/main.go:62 — alerts.NewKafkaPublisher).
//  3. Surrogate recalibration consumer reading M5's requests from Kafka
//     (services/metrics/cmd/main.go:84 — recalconsumer.NewConsumer).
//
// Default min-instances; batch path, not p99-sensitive.
//
// NOTE: AWS M3 exposes a second port 50059 for the Prometheus scrape endpoint
// (services/metrics/cmd/main.go:88 — METRICS_PORT default). Cloud Run v2
// supports only one ingress port per container, so 50059 is not reachable
// from outside. Follow-up: either merge /metrics onto the main port (50056),
// add a sidecar that pushes to Cloud Managed Prometheus, or use Cloud Run's
// native metrics integration. Filed as a GCP-observability follow-up; not
// blocking #491.
func NewM3Metrics(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["metrics"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM3Metrics: cicdOut.RepositoryURLs missing \"metrics\" key required for M3 deploy (#491)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m3-metrics",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: 50056,
			MinInstances:  0,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "LOG_LEVEL", Value: pulumi.String("info")},
				{Name: "DATABASE_ENDPOINT", Value: stages.DB.Endpoint},
				{Name: "DATA_BUCKET", Value: stages.Storage.DataBucketName},
				{Name: "DATA_BUCKET_URI", Value: stages.Storage.DataBucketRef},
				// KAFKA_BROKERS (not KAFKA_BOOTSTRAP_BROKERS) is the name the
				// Go service code actually reads at services/metrics/cmd/main.go:57
				// and the convention shared across every Kafka-consuming
				// service in the repo (Rust crates experimentation-policy,
				// experimentation-management, experimentation-flags,
				// experimentation-pipeline, plus services/management Go).
				// The M1/M2-Orch GCP wiring above currently uses
				// KAFKA_BOOTSTRAP_BROKERS, which no service reads — that
				// inconsistency is pre-existing and tracked as a follow-up.
				{Name: "KAFKA_BROKERS", Value: stages.Stream.BootstrapBrokers},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
				// SASL credentials for Redpanda Cloud — required by both the
				// alerts publisher and the recalibration consumer above.
				{EnvName: "KAFKA_SECRET", SecretID: stages.Secrets.KafkaSecretRef, Version: "latest"},
			},
			// roles/storage.objectAdmin on the data bucket (#491 AC3).
			Buckets: []pulumi.StringInput{stages.Storage.DataBucketName},
			// roles/cloudsql.client — connect to Cloud SQL for metric defs.
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
