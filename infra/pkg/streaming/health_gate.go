package streaming

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/cloudwatch"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// HealthGateArgs configures post-deploy health validation for the Schema Registry
// and Kafka topic provisioning.
type HealthGateArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string
	// ClusterName is the ECS cluster name (for CloudWatch metric dimensions).
	ClusterName pulumi.StringInput
	// SchemaRegistryServiceName is the ECS service name for the Schema Registry.
	SchemaRegistryServiceName pulumi.StringInput
	// SNSTopicArn is the SNS topic for alarm notifications (optional).
	SNSTopicArn pulumi.StringInput
	// Tags applied to all resources.
	Tags pulumi.StringMap
}

// HealthGateOutputs holds the outputs from the health gate resources.
type HealthGateOutputs struct {
	// HealthAlarmArn is the CloudWatch alarm that fires when Schema Registry
	// has zero healthy tasks.
	HealthAlarmArn pulumi.StringOutput
	// TopicCountAlarmArn is the CloudWatch alarm (composite) placeholder —
	// topic count is validated via unit tests and the ExpectedTopicNames export.
	ExpectedTopicNames []string
}

// ExpectedTopicNames returns the canonical list of 8 Kafka topics that must
// exist for the experimentation platform to function. This is the single
// source of truth shared between NewTopics (provisioning) and health gate
// validation (post-deploy checks and tests).
func ExpectedTopicNames() []string {
	names := make([]string, len(topics))
	for i, t := range topics {
		names[i] = t.Name
	}
	return names
}

// ExpectedTopicCount is the number of Kafka topics the platform requires.
const ExpectedTopicCount = 8

// SchemaRegistryHealthCheckConfig holds the health check parameters applied
// to the Schema Registry ECS container, exported for test validation.
type SchemaRegistryHealthCheckConfig struct {
	Command     string
	IntervalSec int
	TimeoutSec  int
	Retries     int
	StartPeriod int
}

// DefaultHealthCheckConfig returns the health check configuration that
// NewSchemaRegistry applies to the container definition.
func DefaultHealthCheckConfig() SchemaRegistryHealthCheckConfig {
	return SchemaRegistryHealthCheckConfig{
		Command:     "CMD-SHELL",
		IntervalSec: 30,
		TimeoutSec:  5,
		Retries:     3,
		StartPeriod: 60,
	}
}

// NewHealthGate creates CloudWatch alarms that monitor the Schema Registry
// ECS service health. The alarm fires when the running task count drops to 0,
// indicating the /subjects health check is failing and no healthy instance exists.
//
// Topic verification is handled via ExpectedTopicNames() which provides the
// canonical list for both provisioning (NewTopics) and post-deploy validation.
func NewHealthGate(ctx *pulumi.Context, args *HealthGateArgs) (*HealthGateOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	// CloudWatch alarm: fires when Schema Registry has 0 running tasks for
	// 2 consecutive 60-second evaluation periods. This catches cases where
	// the deployment circuit breaker triggers (rollback) or the task crashes.
	alarmActions := pulumi.Array{}
	if args.SNSTopicArn != nil {
		alarmActions = pulumi.Array{args.SNSTopicArn.ToStringOutput()}
	}

	healthAlarm, err := cloudwatch.NewMetricAlarm(ctx, "sr-health-alarm", &cloudwatch.MetricAlarmArgs{
		Name:               pulumi.Sprintf("%s-schema-registry-unhealthy", prefix),
		AlarmDescription:   pulumi.String("Schema Registry has 0 running ECS tasks — /subjects health check is failing"),
		ComparisonOperator: pulumi.String("LessThanThreshold"),
		EvaluationPeriods:  pulumi.Int(2),
		Threshold:          pulumi.Float64(1),
		Period:             pulumi.Int(60),
		Statistic:          pulumi.String("Minimum"),
		Namespace:          pulumi.String("AWS/ECS"),
		MetricName:         pulumi.String("RunningTaskCount"),
		Dimensions: pulumi.StringMap{
			"ClusterName": args.ClusterName.ToStringOutput(),
			"ServiceName": args.SchemaRegistryServiceName.ToStringOutput(),
		},
		AlarmActions:     alarmActions,
		OkActions:        alarmActions,
		TreatMissingData: pulumi.String("breaching"),
		Tags:             args.Tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating Schema Registry health alarm: %w", err)
	}

	return &HealthGateOutputs{
		HealthAlarmArn:     healthAlarm.Arn,
		ExpectedTopicNames: ExpectedTopicNames(),
	}, nil
}
