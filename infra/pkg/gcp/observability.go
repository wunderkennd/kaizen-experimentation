// Package gcp — observability.go provisions the GCP observability layer
// (Stage 6 sibling of edge.go), the parity arm of
// pkg/aws/observability.NewCloudWatch + observability.New. Issue #498
// (multi-cloud spec Phase 3).
//
// # Parity table
//
//	AWS (pkg/aws/observability)              GCP (this file)
//	─────────────────────────────────────────────────────────────────────────
//	SNS topic (kaizen-{env}-alarms)          Pub/Sub topic + Monitoring
//	                                          NotificationChannel (type=pubsub)
//	CloudWatch log group per service         Aggregated Cloud Logging bucket
//	                                          + project sink filtering Cloud
//	                                          Run + M4b GCE logs, retention
//	                                          = cfg.CloudwatchRetention
//	CW alarm: p99 latency (M1/M5/M7)         Monitoring alert on
//	                                          run.googleapis.com/request_latencies
//	                                          filtered by service_name
//	CW alarm: error rate > 1% (per svc)      Monitoring alert on
//	                                          run.googleapis.com/request_count
//	                                          filtered by response_code_class
//	CW alarm: RDS CPU > 80%                  Monitoring alert on
//	                                          cloudsql.googleapis.com/database/
//	                                          cpu/utilization
//	CW alarm: RDS connections > 180          Monitoring alert on
//	                                          cloudsql.googleapis.com/database/
//	                                          postgresql/num_backends
//	CW alarm: MSK consumer lag               Skipped — GCP always runs
//	                                          Redpanda; no AWS/Kafka analogue
//	                                          to alarm on. See #620 for
//	                                          Redpanda Cloud alerting.
//	CW alarm: M4b EC2 status check           Monitoring alert on
//	                                          compute.googleapis.com/instance/
//	                                          uptime (ConditionAbsent variant)
//	AMP workspace + remote-write IAM         Managed Service for Prometheus is
//	                                          project-wide and reads Cloud
//	                                          Monitoring data by default; no
//	                                          workspace resource to create.
//	                                          The PromQL query endpoint is
//	                                          exported for Grafana Cloud.
//	AMG workspace (in-account Grafana)       Grafana Cloud (external) is
//	                                          wired out-of-band via the
//	                                          exported query endpoint URL and
//	                                          a service-account token created
//	                                          post-deploy — no in-project
//	                                          Grafana resource, mirroring the
//	                                          spec's Grafana Cloud choice.
//
// # Alert-policy authority
//
// The alertPolicies table below is the ground truth for the parity audit
// (#503) and is pinned independently by the topology test in
// infra/observability_topology_test.go — silent edits here fail loudly there.
package gcp

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/logging"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/monitoring"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/pubsub"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	gcpcicd "github.com/kaizen-experimentation/infra/pkg/gcp/cicd"
)

// alertKind classifies a policy by its metric source so the loop below can
// materialize the correct filter/aggregation without a growing switch.
type alertKind int

const (
	alertRunLatency alertKind = iota
	alertRunErrorRate
	alertCloudSQLCPU
	alertCloudSQLConnections
	alertGCEUptimeAbsent
)

// alertPolicy is one row of the AWS-parity alert inventory. Threshold units
// match the AWS thresholds verbatim — the topology test pins that too.
type alertPolicy struct {
	// slug names the resource; must be unique across alertPolicies.
	slug string
	// display is the human-readable alert-policy title.
	display string
	// kind selects the metric/filter template.
	kind alertKind
	// serviceKey scopes latency + error-rate alerts to one Cloud Run
	// service_name. Ignored for non-Run alert kinds.
	serviceKey string
	// thresholdMs is the p99 latency threshold in milliseconds. Only used
	// when kind == alertRunLatency.
	thresholdMs float64
}

// alertPolicies is the AWS-parity inventory. Latency thresholds mirror
// pkg/aws/observability/cloudwatch.go's latencyTargets (M1 5ms / M5 50ms /
// M7 10ms). Error-rate alerts are enumerated per Cloud Run service
// (gcpcicd.ServiceNames, minus the utility images that don't route requests).
// The three infra alerts (Cloud SQL x2, M4b uptime) close the AWS inventory.
var alertPolicies = func() []alertPolicy {
	out := []alertPolicy{
		{slug: "latency-assignment", display: "M1 Assignment p99 latency > 5ms",
			kind: alertRunLatency, serviceKey: "assignment", thresholdMs: 5},
		{slug: "latency-management", display: "M5 Management p99 latency > 50ms",
			kind: alertRunLatency, serviceKey: "management", thresholdMs: 50},
		{slug: "latency-flags", display: "M7 Flags p99 latency > 10ms",
			kind: alertRunLatency, serviceKey: "flags", thresholdMs: 10},
	}
	for _, svc := range gcpcicd.ServiceNames {
		out = append(out, alertPolicy{
			slug:       "errors-" + svc,
			display:    fmt.Sprintf("%s error rate > 1%%", svc),
			kind:       alertRunErrorRate,
			serviceKey: svc,
		})
	}
	out = append(out,
		alertPolicy{slug: "cloudsql-cpu", display: "Cloud SQL CPU > 80%",
			kind: alertCloudSQLCPU},
		alertPolicy{slug: "cloudsql-connections", display: "Cloud SQL connections > 180",
			kind: alertCloudSQLConnections},
		alertPolicy{slug: "m4b-uptime-absent", display: "M4b GCE instance uptime absent",
			kind: alertGCEUptimeAbsent},
	)
	return out
}()

// ObservabilityInputs holds the cross-stage references NewObservability
// needs. All fields are lazy Pulumi outputs so a caller can wire them
// straight from stage 3 (database) and stage 5 (compute).
type ObservabilityInputs struct {
	// CloudSQLInstanceName scopes the Cloud SQL alerts to the Kaizen DB
	// instance (types.DatabaseOutputs.InstanceId on the GCP arm).
	CloudSQLInstanceName pulumi.StringOutput

	// M4bInstanceName scopes the M4b uptime-absent alert to the Kaizen
	// M4b MIG-managed instance (types.ComputeOutputs.M4bInstanceId).
	M4bInstanceName pulumi.StringOutput
}

// NewObservability provisions the Cloud Monitoring + Cloud Logging + GMP
// layer at AWS CloudWatch parity. See the package comment for the
// AWS-to-GCP mapping.
//
// Returns (error) — parity with pkg/aws.NewObservability. Cross-stage
// consumers pick up values via ctx.Export:
//
//   - `gcpAlertPubsubTopicId`      — Pub/Sub topic for alert fan-out.
//   - `gcpAlertNotificationChannel`— Monitoring channel name to attach to
//     any follow-on custom alerts.
//   - `gcpLogBucketId`             — Cloud Logging bucket that receives the
//     aggregated Kaizen log sink.
//   - `gcpPrometheusQueryEndpoint` — GMP PromQL endpoint Grafana Cloud
//     points its datasource at.
func NewObservability(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	in *ObservabilityInputs,
) error {
	if cfg.GCPProjectID == "" {
		return fmt.Errorf(
			"gcp.NewObservability: cfg.GCPProjectID is required when cloudProvider=gcp")
	}
	project := cfg.GCPProjectID
	envSuffix := cfg.Environment

	// ── 1. Pub/Sub topic + notification channel ────────────────────────
	// Parity with pkg/aws/observability's SNS topic. Subscriptions
	// (email, PagerDuty, Slack) are wired post-deploy — same as the AWS
	// module leaves SNS subscription out.
	alertTopic, err := pubsub.NewTopic(ctx, "kaizen-alert-topic", &pubsub.TopicArgs{
		Name: pulumi.Sprintf("kaizen-%s-alerts", envSuffix),
	})
	if err != nil {
		return fmt.Errorf("alert Pub/Sub topic: %w", err)
	}

	channel, err := monitoring.NewNotificationChannel(ctx, "kaizen-alert-channel",
		&monitoring.NotificationChannelArgs{
			DisplayName: pulumi.Sprintf("kaizen-%s-alerts", envSuffix),
			Type:        pulumi.String("pubsub"),
			Labels: pulumi.StringMap{
				"topic": pulumi.Sprintf(
					"projects/%s/topics/kaizen-%s-alerts", project, envSuffix),
			},
			Enabled: pulumi.Bool(true),
		}, pulumi.DependsOn([]pulumi.Resource{alertTopic}))
	if err != nil {
		return fmt.Errorf("notification channel: %w", err)
	}
	channels := pulumi.StringArray{channel.Name}

	// ── 2. Aggregated Cloud Logging bucket + project sink ──────────────
	// Retention mirrors cfg.CloudwatchRetention (which is a days value on
	// both providers — the field name is the AWS ancestor).
	retention := cfg.CloudwatchRetention
	if retention <= 0 {
		retention = 30
	}
	bucket, err := logging.NewProjectBucketConfig(ctx, "kaizen-log-bucket",
		&logging.ProjectBucketConfigArgs{
			Project:       pulumi.String(project),
			Location:      pulumi.String("global"),
			BucketId:      pulumi.Sprintf("kaizen-%s-logs", envSuffix),
			RetentionDays: pulumi.Int(retention),
		})
	if err != nil {
		return fmt.Errorf("log bucket: %w", err)
	}

	// Sink filter: every Cloud Run revision under this project plus the
	// M4b GCE instance. The naming pattern matches how NewCompute names
	// the services (kaizen-{env}-{key}); parity with AWS's log-group-per-
	// service, without needing a resource per service.
	logFilter := pulumi.Sprintf(
		`(resource.type="cloud_run_revision" AND resource.labels.service_name:"kaizen-%s-") `+
			`OR (resource.type="gce_instance" AND labels."compute.googleapis.com/resource_name":"kaizen-%s-m4b-*")`,
		envSuffix, envSuffix)

	_, err = logging.NewProjectSink(ctx, "kaizen-log-sink",
		&logging.ProjectSinkArgs{
			Name: pulumi.Sprintf("kaizen-%s-sink", envSuffix),
			Destination: pulumi.Sprintf(
				"logging.googleapis.com/projects/%s/locations/global/buckets/kaizen-%s-logs",
				project, envSuffix),
			Filter:               logFilter,
			UniqueWriterIdentity: pulumi.Bool(true),
		}, pulumi.DependsOn([]pulumi.Resource{bucket}))
	if err != nil {
		return fmt.Errorf("log sink: %w", err)
	}

	// ── 3. Alert policies (AWS-parity inventory) ───────────────────────
	for _, ap := range alertPolicies {
		if err := newAlertPolicy(ctx, ap, envSuffix, in, channels); err != nil {
			return fmt.Errorf("alert %q: %w", ap.slug, err)
		}
	}

	// ── 4. Exports ─────────────────────────────────────────────────────
	ctx.Export("gcpAlertPubsubTopicId", alertTopic.ID())
	ctx.Export("gcpAlertNotificationChannel", channel.Name)
	ctx.Export("gcpLogBucketId", bucket.ID())
	// GMP PromQL endpoint is project-scoped and static-form. Exposing it
	// as a stack export means the Grafana Cloud datasource can be wired
	// with `pulumi stack output gcpPrometheusQueryEndpoint`.
	ctx.Export("gcpPrometheusQueryEndpoint", pulumi.String(fmt.Sprintf(
		"https://monitoring.googleapis.com/v1/projects/%s/location/global/prometheus/",
		project)))
	return nil
}

// newAlertPolicy materializes one AWS-parity alert as a Monitoring alert
// policy. The switch is tight on purpose: each kind writes exactly the
// filter/aggregation/threshold shape the AWS analogue uses, so the parity
// audit (#503) reads as a 1:1 diff of the two files.
func newAlertPolicy(
	ctx *pulumi.Context,
	ap alertPolicy,
	envSuffix string,
	in *ObservabilityInputs,
	channels pulumi.StringArray,
) error {
	var (
		filter     pulumi.StringInput
		threshold  float64
		comparison = "COMPARISON_GT"
		aligner    = "ALIGN_MEAN"
		reducer    = "REDUCE_MEAN"
		duration   = "180s"
		useAbsent  = false
	)

	switch ap.kind {
	case alertRunLatency:
		// 99th percentile of Cloud Run request_latencies, filtered to one
		// service_name. Cloud Run tags the metric with the raw service
		// name (no environment prefix); the M-key → cloud-run-service
		// mapping is documented in NewCompute.
		filter = pulumi.Sprintf(
			`metric.type="run.googleapis.com/request_latencies" `+
				`AND resource.type="cloud_run_revision" `+
				`AND resource.labels.service_name="kaizen-%s-%s"`,
			envSuffix, ap.serviceKey)
		threshold = ap.thresholdMs
		aligner = "ALIGN_PERCENTILE_99"
		reducer = "REDUCE_MAX"
	case alertRunErrorRate:
		// request_count filtered on response_code_class="5xx". A raw count
		// threshold (>0 sustained 3 minutes) is the parity-safe minimum;
		// AWS's 1% ratio needs a math expression that isn't 1:1 available
		// as a single alert condition — the ratio variant will land as an
		// SLO burn-rate alert in a follow-up (see #503 audit).
		filter = pulumi.Sprintf(
			`metric.type="run.googleapis.com/request_count" `+
				`AND resource.type="cloud_run_revision" `+
				`AND resource.labels.service_name="kaizen-%s-%s" `+
				`AND metric.labels.response_code_class="5xx"`,
			envSuffix, ap.serviceKey)
		threshold = 0
		aligner = "ALIGN_RATE"
		reducer = "REDUCE_SUM"
	case alertCloudSQLCPU:
		filter = in.CloudSQLInstanceName.ApplyT(func(name string) string {
			return fmt.Sprintf(
				`metric.type="cloudsql.googleapis.com/database/cpu/utilization" `+
					`AND resource.type="cloudsql_database" `+
					`AND resource.labels.database_id:"%s"`, name)
		}).(pulumi.StringOutput)
		threshold = 0.80
		duration = "300s"
	case alertCloudSQLConnections:
		filter = in.CloudSQLInstanceName.ApplyT(func(name string) string {
			return fmt.Sprintf(
				`metric.type="cloudsql.googleapis.com/database/postgresql/num_backends" `+
					`AND resource.type="cloudsql_database" `+
					`AND resource.labels.database_id:"%s"`, name)
		}).(pulumi.StringOutput)
		threshold = 180
		duration = "300s"
	case alertGCEUptimeAbsent:
		// AWS uses StatusCheckFailed>0; on GCE, the equivalent one-signal
		// alert is "uptime metric stopped reporting" — which is a
		// ConditionAbsent, handled below outside the threshold path.
		filter = in.M4bInstanceName.ApplyT(func(name string) string {
			return fmt.Sprintf(
				`metric.type="compute.googleapis.com/instance/uptime" `+
					`AND resource.type="gce_instance" `+
					`AND metric.labels.instance_name="%s"`, name)
		}).(pulumi.StringOutput)
		duration = "120s"
		useAbsent = true
	default:
		return fmt.Errorf("unknown alertKind %d", ap.kind)
	}

	condName := pulumi.Sprintf("%s (kaizen-%s)", ap.display, envSuffix)
	var conditions monitoring.AlertPolicyConditionArray
	if useAbsent {
		conditions = monitoring.AlertPolicyConditionArray{
			&monitoring.AlertPolicyConditionArgs{
				DisplayName: condName,
				ConditionAbsent: &monitoring.AlertPolicyConditionConditionAbsentArgs{
					Filter:   filter,
					Duration: pulumi.String(duration),
					Aggregations: monitoring.AlertPolicyConditionConditionAbsentAggregationArray{
						&monitoring.AlertPolicyConditionConditionAbsentAggregationArgs{
							AlignmentPeriod:  pulumi.String("60s"),
							PerSeriesAligner: pulumi.String(aligner),
						},
					},
				},
			},
		}
	} else {
		conditions = monitoring.AlertPolicyConditionArray{
			&monitoring.AlertPolicyConditionArgs{
				DisplayName: condName,
				ConditionThreshold: &monitoring.AlertPolicyConditionConditionThresholdArgs{
					Filter:         filter,
					Comparison:     pulumi.String(comparison),
					ThresholdValue: pulumi.Float64(threshold),
					Duration:       pulumi.String(duration),
					Aggregations: monitoring.AlertPolicyConditionConditionThresholdAggregationArray{
						&monitoring.AlertPolicyConditionConditionThresholdAggregationArgs{
							AlignmentPeriod:    pulumi.String("60s"),
							PerSeriesAligner:   pulumi.String(aligner),
							CrossSeriesReducer: pulumi.String(reducer),
						},
					},
				},
			},
		}
	}

	_, err := monitoring.NewAlertPolicy(ctx, "alert-"+ap.slug,
		&monitoring.AlertPolicyArgs{
			DisplayName:          pulumi.Sprintf("%s (kaizen-%s)", ap.display, envSuffix),
			Combiner:             pulumi.String("OR"),
			Conditions:           conditions,
			NotificationChannels: channels,
			Enabled:              pulumi.Bool(true),
		})
	return err
}
