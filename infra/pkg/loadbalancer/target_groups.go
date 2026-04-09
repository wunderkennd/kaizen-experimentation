// Package loadbalancer — target_groups.go provisions ALB target groups
// and path-based / host-based listener rules for public-facing services.
//
// Sprint I.1 task I.1.8 — depends on I.0.13 (ALB) and I.1.5 (ECS services).
//
// Routing topology:
//
//	assign.kaizen.{domain}  →  M1 Assignment   (gRPC, port 50051)
//	/api/*                  →  M5 Management   (HTTP/2, port 50055)
//	/flags/*                →  M7 Flags        (gRPC, port 50057)
//	/* (default)            →  M6 UI           (HTTP, port 3000)
package loadbalancer

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/lb"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// TargetGroupInputs are the cross-module dependencies for target groups.
type TargetGroupInputs struct {
	// VpcId is required by ALB target groups for IP-mode registration.
	VpcId pulumi.StringInput
	// HttpsListenerArn from ALBOutputs — listener rules attach here.
	HttpsListenerArn pulumi.StringOutput
	// Domain is the base domain (e.g., "example.com"). M1 uses
	// assign.kaizen.{domain} for host-based routing.
	Domain string
	// Environment name used for resource naming and tagging.
	Environment string
}

// TargetGroupOutputs are exported for ECS service registration and monitoring.
type TargetGroupOutputs struct {
	// M1AssignmentTgArn — ECS service registers tasks here.
	M1AssignmentTgArn pulumi.StringOutput
	// M5ManagementTgArn — ECS service registers tasks here.
	M5ManagementTgArn pulumi.StringOutput
	// M6UITgArn — ECS service registers tasks here.
	M6UITgArn pulumi.StringOutput
	// M7FlagsTgArn — ECS service registers tasks here.
	M7FlagsTgArn pulumi.StringOutput
}

// targetGroupSpec defines a service target group configuration.
type targetGroupSpec struct {
	name            string
	port            int
	protocolVersion string // "gRPC", "HTTP2", or "HTTP1"
	healthCheckPath string
	healthCheckMatcher string // gRPC codes for gRPC TGs, HTTP codes otherwise
}

// NewTargetGroups creates the 4 ALB target groups and listener rules
// for public-facing Kaizen services.
func NewTargetGroups(ctx *pulumi.Context, inputs *TargetGroupInputs) (*TargetGroupOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", inputs.Environment)
	tags := pulumi.StringMap{
		"Project":     pulumi.String("kaizen-experimentation"),
		"Environment": pulumi.String(inputs.Environment),
		"ManagedBy":   pulumi.String("pulumi"),
		"Module":      pulumi.String("loadbalancer"),
	}

	specs := []targetGroupSpec{
		{
			name:               "m1-assignment",
			port:               50051,
			protocolVersion:    "gRPC",
			healthCheckPath:    "/grpc.health.v1.Health/Check",
			healthCheckMatcher: "0",  // gRPC OK
		},
		{
			name:               "m5-management",
			port:               50055,
			protocolVersion:    "HTTP2",
			healthCheckPath:    "/healthz",
			healthCheckMatcher: "200",
		},
		{
			name:            "m6-ui",
			port:            3000,
			protocolVersion: "HTTP1",
			healthCheckPath: "/",
			healthCheckMatcher: "200",
		},
		{
			name:               "m7-flags",
			port:               50057,
			protocolVersion:    "gRPC",
			healthCheckPath:    "/grpc.health.v1.Health/Check",
			healthCheckMatcher: "0",
		},
	}

	tgArns := make(map[string]pulumi.StringOutput, len(specs))

	for _, spec := range specs {
		tg, err := newTargetGroup(ctx, prefix, spec, inputs.VpcId, tags)
		if err != nil {
			return nil, err
		}
		tgArns[spec.name] = tg.Arn
	}

	// --- Listener Rules ---
	// Priority ordering: lower number = evaluated first.
	//   10: host = assign.kaizen.{domain} → M1
	//   20: path = /api/*                 → M5
	//   30: path = /flags/*               → M7
	//  100: path = /* (catch-all)         → M6

	assignHost := fmt.Sprintf("assign.kaizen.%s", inputs.Domain)

	rules := []struct {
		name     string
		priority int
		target   string
		conditions func() lb.ListenerRuleConditionArray
	}{
		{
			name:     "m1-host",
			priority: 10,
			target:   "m1-assignment",
			conditions: func() lb.ListenerRuleConditionArray {
				return lb.ListenerRuleConditionArray{
					&lb.ListenerRuleConditionArgs{
						HostHeader: &lb.ListenerRuleConditionHostHeaderArgs{
							Values: pulumi.StringArray{pulumi.String(assignHost)},
						},
					},
				}
			},
		},
		{
			name:     "m5-api",
			priority: 20,
			target:   "m5-management",
			conditions: func() lb.ListenerRuleConditionArray {
				return lb.ListenerRuleConditionArray{
					&lb.ListenerRuleConditionArgs{
						PathPattern: &lb.ListenerRuleConditionPathPatternArgs{
							Values: pulumi.StringArray{pulumi.String("/api/*")},
						},
					},
				}
			},
		},
		{
			name:     "m7-flags",
			priority: 30,
			target:   "m7-flags",
			conditions: func() lb.ListenerRuleConditionArray {
				return lb.ListenerRuleConditionArray{
					&lb.ListenerRuleConditionArgs{
						PathPattern: &lb.ListenerRuleConditionPathPatternArgs{
							Values: pulumi.StringArray{pulumi.String("/flags/*")},
						},
					},
				}
			},
		},
		{
			name:     "m6-default",
			priority: 100,
			target:   "m6-ui",
			conditions: func() lb.ListenerRuleConditionArray {
				return lb.ListenerRuleConditionArray{
					&lb.ListenerRuleConditionArgs{
						PathPattern: &lb.ListenerRuleConditionPathPatternArgs{
							Values: pulumi.StringArray{pulumi.String("/*")},
						},
					},
				}
			},
		},
	}

	for _, rule := range rules {
		_, err := lb.NewListenerRule(ctx, fmt.Sprintf("%s-rule-%s", prefix, rule.name), &lb.ListenerRuleArgs{
			ListenerArn: inputs.HttpsListenerArn,
			Priority:    pulumi.Int(rule.priority),
			Actions: lb.ListenerRuleActionArray{
				&lb.ListenerRuleActionArgs{
					Type:           pulumi.String("forward"),
					TargetGroupArn: tgArns[rule.target],
				},
			},
			Conditions: rule.conditions(),
			Tags:       tags,
		})
		if err != nil {
			return nil, fmt.Errorf("creating listener rule %q: %w", rule.name, err)
		}
	}

	return &TargetGroupOutputs{
		M1AssignmentTgArn: tgArns["m1-assignment"],
		M5ManagementTgArn: tgArns["m5-management"],
		M6UITgArn:         tgArns["m6-ui"],
		M7FlagsTgArn:      tgArns["m7-flags"],
	}, nil
}

// newTargetGroup creates a single ALB target group with health check
// configuration appropriate for its protocol version (gRPC, HTTP/2, or HTTP/1).
func newTargetGroup(
	ctx *pulumi.Context,
	prefix string,
	spec targetGroupSpec,
	vpcId pulumi.StringInput,
	tags pulumi.StringMap,
) (*lb.TargetGroup, error) {
	// ALB target groups for ECS Fargate use IP target type.
	// Protocol is always "HTTP" — ProtocolVersion distinguishes gRPC/HTTP2/HTTP1.
	// Health checks also use HTTP; the Matcher field differentiates gRPC codes
	// (e.g., "0" for OK) from HTTP status codes (e.g., "200").
	tg, err := lb.NewTargetGroup(ctx, fmt.Sprintf("%s-tg-%s", prefix, spec.name), &lb.TargetGroupArgs{
		Name:            pulumi.Sprintf("%s-%s", prefix, spec.name),
		Port:            pulumi.Int(spec.port),
		Protocol:        pulumi.String("HTTP"),
		ProtocolVersion: pulumi.String(spec.protocolVersion),
		VpcId:           vpcId.ToStringOutput(),
		TargetType:      pulumi.String("ip"),

		// Deregistration delay: 30s gives in-flight requests time to drain
		// without holding up deployments. Default 300s is too long for
		// rolling ECS deployments.
		DeregistrationDelay: pulumi.Int(30),

		HealthCheck: &lb.TargetGroupHealthCheckArgs{
			Enabled:            pulumi.Bool(true),
			Protocol:           pulumi.String("HTTP"),
			Path:               pulumi.String(spec.healthCheckPath),
			Port:               pulumi.String("traffic-port"),
			Interval:           pulumi.Int(15),
			Timeout:            pulumi.Int(5),
			HealthyThreshold:   pulumi.Int(2),
			UnhealthyThreshold: pulumi.Int(3),
			Matcher:            pulumi.String(spec.healthCheckMatcher),
		},

		Tags: tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating target group %q: %w", spec.name, err)
	}

	return tg, nil
}
