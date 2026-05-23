package observability

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/logging"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/monitoring"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// NewObservability configures GCP logging sinks, Cloud Monitoring alerts, and Prometheus metrics.
func NewObservability(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	dbOut types.DatabaseOutputs,
	streamOut types.StreamingOutputs,
	computeOut types.ComputeOutputs,
) error {
	env := cfg.Environment
	project := cfg.GCPProjectID

	// 1. Centralized Log Sink
	logSink, err := logging.NewProjectSink(ctx, fmt.Sprintf("kaizen-%s-log-sink", env), &logging.ProjectSinkArgs{
		Project:     pulumi.String(project),
		Destination: pulumi.String(fmt.Sprintf("storage.googleapis.com/kaizen-%s-logs-bucket", env)),
		Filter:      pulumi.String("resource.type=\"cloud_run_revision\" OR resource.type=\"gce_instance\""),
	})
	if err != nil {
		return err
	}

	// 2. Alert Policy: Cloud Run CPU/Memory exceeds 85%
	_, err = monitoring.NewAlertPolicy(ctx, fmt.Sprintf("kaizen-%s-run-resource-alert", env), &monitoring.AlertPolicyArgs{
		Project:     pulumi.String(project),
		DisplayName: pulumi.String(fmt.Sprintf("Kaizen %s Cloud Run Resource Alert (>85%%)", env)),
		Combiner:    pulumi.String("OR"),
		Conditions: monitoring.AlertPolicyConditionArray{
			&monitoring.AlertPolicyConditionArgs{
				DisplayName: pulumi.String("Cloud Run CPU Utilization"),
				ConditionThreshold: &monitoring.AlertPolicyConditionConditionThresholdArgs{
					Filter:     pulumi.String("resource.type = \"cloud_run_revision\" AND metric.type = \"run.googleapis.com/container/cpu/utilizations\""),
					Duration:   pulumi.String("60s"),
					Comparison: pulumi.String("COMPARISON_GT"),
					ThresholdValue: pulumi.Float64(0.85),
					Aggregations: monitoring.AlertPolicyConditionConditionThresholdAggregationArray{
						&monitoring.AlertPolicyConditionConditionThresholdAggregationArgs{
							AlignmentPeriod:  pulumi.String("60s"),
							PerSeriesAligner: pulumi.String("ALIGN_MEAN"),
						},
					},
				},
			},
		},
	})
	if err != nil {
		return err
	}

	// 3. Alert Policy: M1/M7 5xx Error Rate > 0.1% over 1 min
	_, err = monitoring.NewAlertPolicy(ctx, fmt.Sprintf("kaizen-%s-run-errors-alert", env), &monitoring.AlertPolicyArgs{
		Project:     pulumi.String(project),
		DisplayName: pulumi.String(fmt.Sprintf("Kaizen %s Service 5xx Error Rate Alert (>0.1%%)", env)),
		Combiner:    pulumi.String("OR"),
		Conditions: monitoring.AlertPolicyConditionArray{
			&monitoring.AlertPolicyConditionArgs{
				DisplayName: pulumi.String("Cloud Run 5xx Errors"),
				ConditionThreshold: &monitoring.AlertPolicyConditionConditionThresholdArgs{
					Filter:     pulumi.String("resource.type = \"cloud_run_revision\" AND metric.type = \"run.googleapis.com/request_count\" AND metric.labels.response_code_class = \"5xx\""),
					Duration:   pulumi.String("60s"),
					Comparison: pulumi.String("COMPARISON_GT"),
					ThresholdValue: pulumi.Float64(10), // Alert if there are more than 10 5xx errors per min
					Aggregations: monitoring.AlertPolicyConditionConditionThresholdAggregationArray{
						&monitoring.AlertPolicyConditionConditionThresholdAggregationArgs{
							AlignmentPeriod:  pulumi.String("60s"),
							PerSeriesAligner: pulumi.String("ALIGN_RATE"),
						},
					},
				},
			},
		},
	})
	if err != nil {
		return err
	}

	// Export observability configurations
	ctx.Export("gcpObservabilityLogSinkId", logSink.ID())

	return nil
}
