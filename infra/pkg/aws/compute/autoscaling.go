// Package compute — autoscaling policies for Kaizen Fargate services.
//
// Sprint I.1.7: Target-tracking autoscaling for all Fargate services.
// M1 Assignment and M7 Flags scale on ALBRequestCountPerTarget (latency-sensitive gRPC).
// All other services scale on ECSServiceAverageCPUUtilization.
package compute

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/appautoscaling"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ServiceScalingConfig defines the autoscaling parameters for a single ECS service.
type ServiceScalingConfig struct {
	// MinCapacity is the minimum number of tasks.
	MinCapacity int
	// MaxCapacity is the maximum number of tasks.
	MaxCapacity int
}

// AutoscalingArgs configures autoscaling for all Fargate services.
type AutoscalingArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string
	// ClusterName is the ECS cluster name (used to build resource IDs).
	ClusterName pulumi.StringInput

	// Per-service scaling config. All values are overridable per environment.
	M1Assignment ServiceScalingConfig
	M2Pipeline   ServiceScalingConfig
	M2Orch       ServiceScalingConfig
	M3Metrics    ServiceScalingConfig
	M4aAnalysis  ServiceScalingConfig
	M5Management ServiceScalingConfig
	M6UI         ServiceScalingConfig
	M7Flags      ServiceScalingConfig

	// ALBFullName is the ALB's full name suffix (e.g. "app/kaizen-dev-alb/50dc6c495c0c9188").
	// Required for ALBRequestCountPerTarget metrics on M1 and M7.
	ALBFullName pulumi.StringInput

	// M1TargetGroupFullName is the target group full name for M1 Assignment
	// (e.g. "targetgroup/kaizen-dev-m1/abcdef123456").
	M1TargetGroupFullName pulumi.StringInput

	// M7TargetGroupFullName is the target group full name for M7 Flags.
	M7TargetGroupFullName pulumi.StringInput
}

// AutoscalingOutputs holds references to the created scaling targets and policies.
type AutoscalingOutputs struct {
	// ScalingTargetArns maps service key → scalable target ARN.
	ScalingTargetArns map[string]pulumi.StringOutput
	// PolicyArns maps service key → scaling policy ARN.
	PolicyArns map[string]pulumi.StringOutput
}

// DefaultAutoscalingArgs returns environment-appropriate defaults per the task spec.
// Callers may override individual ServiceScalingConfig fields after calling this.
func DefaultAutoscalingArgs(env string) AutoscalingArgs {
	args := AutoscalingArgs{
		Environment:  env,
		M1Assignment: ServiceScalingConfig{MinCapacity: 2, MaxCapacity: 20},
		M2Pipeline:   ServiceScalingConfig{MinCapacity: 2, MaxCapacity: 10},
		M2Orch:       ServiceScalingConfig{MinCapacity: 1, MaxCapacity: 3},
		M3Metrics:    ServiceScalingConfig{MinCapacity: 1, MaxCapacity: 5},
		M4aAnalysis:  ServiceScalingConfig{MinCapacity: 1, MaxCapacity: 5},
		M5Management: ServiceScalingConfig{MinCapacity: 1, MaxCapacity: 5},
		M6UI:         ServiceScalingConfig{MinCapacity: 1, MaxCapacity: 3},
		M7Flags:      ServiceScalingConfig{MinCapacity: 2, MaxCapacity: 10},
	}

	// Dev environment: reduce minimums to save cost.
	if env == "dev" {
		args.M1Assignment.MinCapacity = 1
		args.M1Assignment.MaxCapacity = 5
		args.M2Pipeline.MinCapacity = 1
		args.M2Pipeline.MaxCapacity = 3
		args.M7Flags.MinCapacity = 1
		args.M7Flags.MaxCapacity = 3
	}

	return args
}

// NewAutoscaling creates target-tracking autoscaling policies for all Fargate services.
func NewAutoscaling(ctx *pulumi.Context, args *AutoscalingArgs) (*AutoscalingOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	outputs := &AutoscalingOutputs{
		ScalingTargetArns: make(map[string]pulumi.StringOutput),
		PolicyArns:        make(map[string]pulumi.StringOutput),
	}

	// CPU-tracked services: M2, M2-Orch, M3, M4a, M5, M6.
	cpuServices := []struct {
		key     string
		service string
		config  ServiceScalingConfig
	}{
		{"m2", "m2-pipeline", args.M2Pipeline},
		{"m2-orch", "m2-orch", args.M2Orch},
		{"m3", "m3-metrics", args.M3Metrics},
		{"m4a", "m4a-analysis", args.M4aAnalysis},
		{"m5", "m5-management", args.M5Management},
		{"m6", "m6-ui", args.M6UI},
	}

	for _, svc := range cpuServices {
		target, policy, err := newCPUScalingPolicy(ctx, prefix, svc.key, svc.service, args.ClusterName, svc.config, args.Environment)
		if err != nil {
			return nil, fmt.Errorf("creating CPU autoscaling for %s: %w", svc.key, err)
		}
		outputs.ScalingTargetArns[svc.key] = target.Arn
		outputs.PolicyArns[svc.key] = policy.Arn
	}

	// ALB request count services: M1 Assignment, M7 Flags.
	albServices := []struct {
		key              string
		service          string
		config           ServiceScalingConfig
		targetGroupFull  pulumi.StringInput
	}{
		{"m1", "m1-assignment", args.M1Assignment, args.M1TargetGroupFullName},
		{"m7", "m7-flags", args.M7Flags, args.M7TargetGroupFullName},
	}

	for _, svc := range albServices {
		target, policy, err := newALBScalingPolicy(
			ctx, prefix, svc.key, svc.service,
			args.ClusterName, svc.config,
			args.ALBFullName, svc.targetGroupFull,
			args.Environment,
		)
		if err != nil {
			return nil, fmt.Errorf("creating ALB autoscaling for %s: %w", svc.key, err)
		}
		outputs.ScalingTargetArns[svc.key] = target.Arn
		outputs.PolicyArns[svc.key] = policy.Arn
	}

	return outputs, nil
}

// newCPUScalingPolicy creates a target-tracking policy on ECSServiceAverageCPUUtilization
// at 70% target for a single Fargate service.
func newCPUScalingPolicy(
	ctx *pulumi.Context,
	prefix, key, serviceName string,
	clusterName pulumi.StringInput,
	cfg ServiceScalingConfig,
	env string,
) (*appautoscaling.Target, *appautoscaling.Policy, error) {
	resourceId := pulumi.Sprintf("service/%s/%s-%s", clusterName, prefix, serviceName)

	target, err := appautoscaling.NewTarget(ctx, fmt.Sprintf("%s-scaling-target", key), &appautoscaling.TargetArgs{
		MaxCapacity:       pulumi.Int(cfg.MaxCapacity),
		MinCapacity:       pulumi.Int(cfg.MinCapacity),
		ResourceId:        resourceId,
		ScalableDimension: pulumi.String("ecs:service:DesiredCount"),
		ServiceNamespace:  pulumi.String("ecs"),
		Tags: pulumi.StringMap{
			"Environment": pulumi.String(env),
			"Project":     pulumi.String("kaizen"),
			"Service":     pulumi.String(key),
		},
	})
	if err != nil {
		return nil, nil, fmt.Errorf("creating scaling target: %w", err)
	}

	policy, err := appautoscaling.NewPolicy(ctx, fmt.Sprintf("%s-cpu-scaling", key), &appautoscaling.PolicyArgs{
		Name:              pulumi.Sprintf("%s-%s-cpu-tracking", prefix, key),
		PolicyType:        pulumi.String("TargetTrackingScaling"),
		ResourceId:        target.ResourceId,
		ScalableDimension: target.ScalableDimension,
		ServiceNamespace:  target.ServiceNamespace,
		TargetTrackingScalingPolicyConfiguration: &appautoscaling.PolicyTargetTrackingScalingPolicyConfigurationArgs{
			PredefinedMetricSpecification: &appautoscaling.PolicyTargetTrackingScalingPolicyConfigurationPredefinedMetricSpecificationArgs{
				PredefinedMetricType: pulumi.String("ECSServiceAverageCPUUtilization"),
			},
			TargetValue:      pulumi.Float64(70.0),
			ScaleInCooldown:  pulumi.Int(300),
			ScaleOutCooldown: pulumi.Int(60),
		},
	})
	if err != nil {
		return nil, nil, fmt.Errorf("creating scaling policy: %w", err)
	}

	return target, policy, nil
}

// newALBScalingPolicy creates a target-tracking policy on ALBRequestCountPerTarget
// at 1000 requests/target for a single Fargate service.
func newALBScalingPolicy(
	ctx *pulumi.Context,
	prefix, key, serviceName string,
	clusterName pulumi.StringInput,
	cfg ServiceScalingConfig,
	albFullName pulumi.StringInput,
	targetGroupFullName pulumi.StringInput,
	env string,
) (*appautoscaling.Target, *appautoscaling.Policy, error) {
	resourceId := pulumi.Sprintf("service/%s/%s-%s", clusterName, prefix, serviceName)

	target, err := appautoscaling.NewTarget(ctx, fmt.Sprintf("%s-scaling-target", key), &appautoscaling.TargetArgs{
		MaxCapacity:       pulumi.Int(cfg.MaxCapacity),
		MinCapacity:       pulumi.Int(cfg.MinCapacity),
		ResourceId:        resourceId,
		ScalableDimension: pulumi.String("ecs:service:DesiredCount"),
		ServiceNamespace:  pulumi.String("ecs"),
		Tags: pulumi.StringMap{
			"Environment": pulumi.String(env),
			"Project":     pulumi.String("kaizen"),
			"Service":     pulumi.String(key),
		},
	})
	if err != nil {
		return nil, nil, fmt.Errorf("creating scaling target: %w", err)
	}

	// The resource label format for ALBRequestCountPerTarget is:
	// app/<alb-name>/<alb-id>/targetgroup/<tg-name>/<tg-id>
	resourceLabel := pulumi.Sprintf("%s/%s", albFullName, targetGroupFullName)

	policy, err := appautoscaling.NewPolicy(ctx, fmt.Sprintf("%s-alb-scaling", key), &appautoscaling.PolicyArgs{
		Name:              pulumi.Sprintf("%s-%s-alb-tracking", prefix, key),
		PolicyType:        pulumi.String("TargetTrackingScaling"),
		ResourceId:        target.ResourceId,
		ScalableDimension: target.ScalableDimension,
		ServiceNamespace:  target.ServiceNamespace,
		TargetTrackingScalingPolicyConfiguration: &appautoscaling.PolicyTargetTrackingScalingPolicyConfigurationArgs{
			PredefinedMetricSpecification: &appautoscaling.PolicyTargetTrackingScalingPolicyConfigurationPredefinedMetricSpecificationArgs{
				PredefinedMetricType: pulumi.String("ALBRequestCountPerTarget"),
				ResourceLabel:        resourceLabel,
			},
			TargetValue:      pulumi.Float64(1000.0),
			ScaleInCooldown:  pulumi.Int(300),
			ScaleOutCooldown: pulumi.Int(60),
		},
	})
	if err != nil {
		return nil, nil, fmt.Errorf("creating scaling policy: %w", err)
	}

	return target, policy, nil
}
