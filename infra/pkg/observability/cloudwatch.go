// Package observability provisions CloudWatch log groups, metric alarms,
// and SNS notification topics for the Kaizen experimentation platform.
//
// Sprint I.1.9: 9 ECS log groups, latency/error/infra alarms, SNS topic.
package observability

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/cloudwatch"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/sns"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/cicd"
	"github.com/kaizen-experimentation/infra/pkg/config"
)

// CloudWatchArgs configures the CloudWatch log groups and alarms module.
type CloudWatchArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string
	// CloudwatchRetention is the log retention in days (from stack config).
	CloudwatchRetention int
	// RdsInstanceId is the RDS DB instance identifier for metric alarms.
	RdsInstanceId pulumi.StringOutput
	// MskClusterName is the MSK cluster name for consumer lag alarms.
	MskClusterName pulumi.StringOutput
	// M4bAutoScalingGroupName is the ASG name for EC2 status check alarms.
	M4bAutoScalingGroupName pulumi.StringOutput
	// Tags applied to all resources.
	Tags pulumi.StringMap
}

// CloudWatchOutputs holds the resources created by the CloudWatch module.
type CloudWatchOutputs struct {
	// LogGroupArns maps service name to its CloudWatch log group ARN.
	LogGroupArns map[string]pulumi.StringOutput
	// AlarmTopicArn is the SNS topic ARN that receives all alarm notifications.
	AlarmTopicArn pulumi.StringOutput
}

// NewCloudWatch creates CloudWatch log groups for all 9 ECS services, metric alarms
// for latency, error rates, RDS, MSK, and M4b health, and an SNS topic
// for alarm notifications.
func NewCloudWatch(ctx *pulumi.Context, args *CloudWatchArgs) (*CloudWatchOutputs, error) {
	tags := args.Tags
	if tags == nil {
		tags = config.DefaultTags(args.Environment)
	}

	// ── SNS topic for alarm notifications ───────────────────────────────
	alarmTopic, err := sns.NewTopic(ctx, "kaizen-alarm-topic", &sns.TopicArgs{
		Name: pulumi.Sprintf("kaizen-%s-alarms", args.Environment),
		Tags: tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating alarm SNS topic: %w", err)
	}

	// ── Log groups: /ecs/kaizen/{service-name} ──────────────────────────
	logGroupArns := make(map[string]pulumi.StringOutput, len(cicd.ServiceNames))

	for _, svc := range cicd.ServiceNames {
		lg, err := cloudwatch.NewLogGroup(ctx, fmt.Sprintf("log-%s", svc), &cloudwatch.LogGroupArgs{
			Name:            pulumi.Sprintf("/ecs/kaizen/%s", svc),
			RetentionInDays: pulumi.Int(args.CloudwatchRetention),
			Tags: config.MergeTags(tags, pulumi.StringMap{
				"Service": pulumi.String(svc),
			}),
		})
		if err != nil {
			return nil, fmt.Errorf("creating log group for %s: %w", svc, err)
		}
		logGroupArns[svc] = lg.Arn
	}

	// ── Latency alarms (p99) ────────────────────────────────────────────
	// Only services with explicit SLO targets get latency alarms.
	latencyTargets := []struct {
		service     string
		thresholdMs float64
	}{
		{"assignment", 5},   // M1 < 5ms
		{"management", 50},  // M5 < 50ms
		{"flags", 10},       // M7 < 10ms
	}

	for _, t := range latencyTargets {
		_, err := cloudwatch.NewMetricAlarm(ctx, fmt.Sprintf("alarm-latency-%s", t.service), &cloudwatch.MetricAlarmArgs{
			Name:               pulumi.Sprintf("kaizen-%s-%s-p99-latency", args.Environment, t.service),
			ComparisonOperator: pulumi.String("GreaterThanThreshold"),
			EvaluationPeriods:  pulumi.Int(3),
			MetricName:         pulumi.String("p99_latency_ms"),
			Namespace:          pulumi.String("Kaizen/ECS"),
			Period:             pulumi.Int(60),
			Statistic:          pulumi.String("Maximum"),
			Threshold:          pulumi.Float64(t.thresholdMs),
			AlarmDescription:   pulumi.Sprintf("%s p99 latency exceeds %.0fms", t.service, t.thresholdMs),
			AlarmActions:       pulumi.Array{alarmTopic.Arn},
			OkActions:          pulumi.Array{alarmTopic.Arn},
			Dimensions: pulumi.StringMap{
				"ServiceName": pulumi.String(t.service),
			},
			TreatMissingData: pulumi.String("notBreaching"),
			Tags:             tags,
		})
		if err != nil {
			return nil, fmt.Errorf("creating latency alarm for %s: %w", t.service, err)
		}
	}

	// ── Error rate alarms (> 1% on every service) ───────────────────────
	for _, svc := range cicd.ServiceNames {
		_, err := cloudwatch.NewMetricAlarm(ctx, fmt.Sprintf("alarm-errors-%s", svc), &cloudwatch.MetricAlarmArgs{
			Name:               pulumi.Sprintf("kaizen-%s-%s-error-rate", args.Environment, svc),
			ComparisonOperator: pulumi.String("GreaterThanThreshold"),
			EvaluationPeriods:  pulumi.Int(3),
			Threshold:          pulumi.Float64(1.0),
			AlarmDescription:   pulumi.Sprintf("%s error rate exceeds 1%%", svc),
			AlarmActions:       pulumi.Array{alarmTopic.Arn},
			OkActions:          pulumi.Array{alarmTopic.Arn},
			TreatMissingData:   pulumi.String("notBreaching"),
			Tags:               tags,
			MetricQueries: cloudwatch.MetricAlarmMetricQueryArray{
				&cloudwatch.MetricAlarmMetricQueryArgs{
					Id:         pulumi.String("errors"),
					Label:      pulumi.String("Error Count"),
					ReturnData: pulumi.Bool(false),
					Metric: &cloudwatch.MetricAlarmMetricQueryMetricArgs{
						MetricName: pulumi.String("error_count"),
						Namespace:  pulumi.String("Kaizen/ECS"),
						Dimensions: pulumi.StringMap{
							"ServiceName": pulumi.String(svc),
						},
						Period: pulumi.Int(300),
						Stat:   pulumi.String("Sum"),
					},
				},
				&cloudwatch.MetricAlarmMetricQueryArgs{
					Id:         pulumi.String("requests"),
					Label:      pulumi.String("Request Count"),
					ReturnData: pulumi.Bool(false),
					Metric: &cloudwatch.MetricAlarmMetricQueryMetricArgs{
						MetricName: pulumi.String("request_count"),
						Namespace:  pulumi.String("Kaizen/ECS"),
						Dimensions: pulumi.StringMap{
							"ServiceName": pulumi.String(svc),
						},
						Period: pulumi.Int(300),
						Stat:   pulumi.String("Sum"),
					},
				},
				&cloudwatch.MetricAlarmMetricQueryArgs{
					Id:         pulumi.String("error_rate"),
					Label:      pulumi.String("Error Rate %"),
					ReturnData: pulumi.Bool(true),
					Expression: pulumi.String("IF(requests > 0, (errors / requests) * 100, 0)"),
				},
			},
		})
		if err != nil {
			return nil, fmt.Errorf("creating error rate alarm for %s: %w", svc, err)
		}
	}

	// ── RDS CPU utilization alarm (> 80%) ───────────────────────────────
	_, err = cloudwatch.NewMetricAlarm(ctx, "alarm-rds-cpu", &cloudwatch.MetricAlarmArgs{
		Name:               pulumi.Sprintf("kaizen-%s-rds-cpu-high", args.Environment),
		ComparisonOperator: pulumi.String("GreaterThanThreshold"),
		EvaluationPeriods:  pulumi.Int(3),
		MetricName:         pulumi.String("CPUUtilization"),
		Namespace:          pulumi.String("AWS/RDS"),
		Period:             pulumi.Int(300),
		Statistic:          pulumi.String("Average"),
		Threshold:          pulumi.Float64(80.0),
		AlarmDescription:   pulumi.String("RDS CPU utilization exceeds 80%"),
		AlarmActions:       pulumi.Array{alarmTopic.Arn},
		OkActions:          pulumi.Array{alarmTopic.Arn},
		Dimensions: pulumi.StringMap{
			"DBInstanceIdentifier": args.RdsInstanceId,
		},
		TreatMissingData: pulumi.String("missing"),
		Tags:             tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating RDS CPU alarm: %w", err)
	}

	// ── RDS connection count alarm (> 180) ──────────────────────────────
	_, err = cloudwatch.NewMetricAlarm(ctx, "alarm-rds-connections", &cloudwatch.MetricAlarmArgs{
		Name:               pulumi.Sprintf("kaizen-%s-rds-connections-high", args.Environment),
		ComparisonOperator: pulumi.String("GreaterThanThreshold"),
		EvaluationPeriods:  pulumi.Int(3),
		MetricName:         pulumi.String("DatabaseConnections"),
		Namespace:          pulumi.String("AWS/RDS"),
		Period:             pulumi.Int(300),
		Statistic:          pulumi.String("Average"),
		Threshold:          pulumi.Float64(180.0),
		AlarmDescription:   pulumi.String("RDS connections exceed 180 (max_connections=200)"),
		AlarmActions:       pulumi.Array{alarmTopic.Arn},
		OkActions:          pulumi.Array{alarmTopic.Arn},
		Dimensions: pulumi.StringMap{
			"DBInstanceIdentifier": args.RdsInstanceId,
		},
		TreatMissingData: pulumi.String("missing"),
		Tags:             tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating RDS connections alarm: %w", err)
	}

	// ── MSK consumer lag alarm (guardrail_alerts > 10000) ───────────────
	_, err = cloudwatch.NewMetricAlarm(ctx, "alarm-msk-consumer-lag", &cloudwatch.MetricAlarmArgs{
		Name:               pulumi.Sprintf("kaizen-%s-msk-consumer-lag", args.Environment),
		ComparisonOperator: pulumi.String("GreaterThanThreshold"),
		EvaluationPeriods:  pulumi.Int(3),
		MetricName:         pulumi.String("MaxOffsetLag"),
		Namespace:          pulumi.String("AWS/Kafka"),
		Period:             pulumi.Int(300),
		Statistic:          pulumi.String("Maximum"),
		Threshold:          pulumi.Float64(10000.0),
		AlarmDescription:   pulumi.String("MSK consumer lag on guardrail_alerts exceeds 10000"),
		AlarmActions:       pulumi.Array{alarmTopic.Arn},
		OkActions:          pulumi.Array{alarmTopic.Arn},
		Dimensions: pulumi.StringMap{
			"Cluster Name":   args.MskClusterName,
			"Consumer Group": pulumi.String("guardrail_alerts"),
			"Topic":          pulumi.String("guardrail_alerts"),
		},
		TreatMissingData: pulumi.String("notBreaching"),
		Tags:             tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating MSK consumer lag alarm: %w", err)
	}

	// ── M4b EC2 status check failure alarm ──────────────────────────────
	_, err = cloudwatch.NewMetricAlarm(ctx, "alarm-m4b-status-check", &cloudwatch.MetricAlarmArgs{
		Name:               pulumi.Sprintf("kaizen-%s-m4b-status-check", args.Environment),
		ComparisonOperator: pulumi.String("GreaterThanThreshold"),
		EvaluationPeriods:  pulumi.Int(2),
		MetricName:         pulumi.String("StatusCheckFailed"),
		Namespace:          pulumi.String("AWS/EC2"),
		Period:             pulumi.Int(60),
		Statistic:          pulumi.String("Maximum"),
		Threshold:          pulumi.Float64(0.0),
		AlarmDescription:   pulumi.String("M4b EC2 instance status check failure"),
		AlarmActions:       pulumi.Array{alarmTopic.Arn},
		OkActions:          pulumi.Array{alarmTopic.Arn},
		Dimensions: pulumi.StringMap{
			"AutoScalingGroupName": args.M4bAutoScalingGroupName,
		},
		TreatMissingData: pulumi.String("breaching"),
		Tags:             tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b status check alarm: %w", err)
	}

	// ── Exports ─────────────────────────────────────────────────────────
	ctx.Export("alarmTopicArn", alarmTopic.Arn)

	return &CloudWatchOutputs{
		LogGroupArns:  logGroupArns,
		AlarmTopicArn: alarmTopic.Arn,
	}, nil
}
