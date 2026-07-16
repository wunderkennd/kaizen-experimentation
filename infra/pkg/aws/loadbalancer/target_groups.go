// Package loadbalancer — target_groups.go provisions ALB target groups
// and path-based / host-based listener rules for public-facing services.
//
// Sprint I.1 task I.1.8 — depends on I.0.13 (ALB) and I.1.5 (ECS services).
//
// Routing topology:
//
//	assign.kaizen.{domain}          →  M1 Assignment   (gRPC, port 50051)
//	/experimentation.management.*   →  M5 Management   (bare ConnectRPC paths, port 50055)
//	/experimentation.flags.*        →  M7 Flags        (gRPC, port 50057)
//	/* (default)                    →  M6 UI           (HTTP, port 3000)
//
// gRPC/Connect rules MUST match on bare /package.Service/Method paths —
// gRPC clients cannot mount a service under a path prefix, and a
// human-facing prefix like the old /flags/* shadows the UI's page routes
// (browsers got ALB 464 Incompatible-protocol on /flags/new because the
// GET landed on the GRPC-protocol-version target group). The dot in
// /experimentation.<module>.* keeps these disjoint from UI page routes.
//
// Browser API traffic (/api/rpc/<module>/...) intentionally falls through
// to the M6 catch-all: the Next.js BFF route handler proxies it to the
// backends over Cloud Map, since ALB cannot rewrite paths and the
// backends serve bare /package.Service/Method ConnectRPC routes.
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
	// TlsEnabled mirrors the ALB mode. ALB rejects GRPC/HTTP2
	// protocol-version target groups behind a plaintext listener
	// ("InvalidLoadBalancerAction: Listener protocol 'HTTP' is not
	// supported…"), so when false the gRPC rules (M1, M7) are skipped and
	// M5 falls back to HTTP/1.1 (ConnectRPC unary + /healthz both work).
	TlsEnabled bool
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
	// M1AssignmentTgArnSuffix is the target group ARN suffix for ALBRequestCountPerTarget.
	M1AssignmentTgArnSuffix pulumi.StringOutput
	// M7FlagsTgArnSuffix is the target group ARN suffix for ALBRequestCountPerTarget.
	M7FlagsTgArnSuffix pulumi.StringOutput
	// Rules are the listener-rule resources. ECS services referencing the
	// target groups must depend on these: a target group is only
	// "attached to a load balancer" (an ECS create/update precondition)
	// once a rule forwards to it.
	Rules []pulumi.Resource
}

// targetGroupSpec defines a service target group configuration.
type targetGroupSpec struct {
	name            string
	port            int
	protocolVersion string // "GRPC", "HTTP2", or "HTTP1"
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

	m5ProtocolVersion := "HTTP2"
	if !inputs.TlsEnabled {
		m5ProtocolVersion = "HTTP1" // HTTP2 TGs are invalid behind the plaintext :80 rules listener
	}

	specs := []targetGroupSpec{
		{
			name:            "m1-assignment",
			port:            50051,
			protocolVersion: "GRPC",
			healthCheckPath: "/grpc.health.v1.Health/Check",
			// M1/M7 don't register grpc.health.v1 (no tonic-health), so a
			// strict "0" matcher would mark every target unhealthy
			// (UNIMPLEMENTED=12) and the LB-integrated scheduler would kill
			// the tasks. Any gRPC status proves the server answers HTTP/2;
			// tighten back to "0" when tonic-health ships.
			healthCheckMatcher: "0-99",
		},
		{
			name:               "m5-management",
			port:               50055,
			protocolVersion:    m5ProtocolVersion,
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
			name:            "m7-flags",
			port:            50057,
			protocolVersion: "GRPC",
			healthCheckPath: "/grpc.health.v1.Health/Check",
			// See m1-assignment: no grpc.health.v1 service registered yet.
			healthCheckMatcher: "0-99",
		},
	}

	tgArns := make(map[string]pulumi.StringOutput, len(specs))
	tgArnSuffixes := make(map[string]pulumi.StringOutput, len(specs))

	for _, spec := range specs {
		tg, err := newTargetGroup(ctx, prefix, spec, inputs.VpcId, tags)
		if err != nil {
			return nil, err
		}
		tgArns[spec.name] = tg.Arn
		tgArnSuffixes[spec.name] = tg.ArnSuffix
	}

	// --- Listener Rules ---
	// Priority ordering: lower number = evaluated first.
	//   10: host = assign.kaizen.{domain}       → M1
	//   20: path = /experimentation.management.* → M5
	//   30: path = /experimentation.flags.*      → M7
	//  100: path = /* (catch-all, incl. UI pages and /api/rpc BFF) → M6

	assignHost := fmt.Sprintf("assign.kaizen.%s", inputs.Domain)

	rules := []struct {
		name        string
		priority    int
		target      string
		requiresTls bool // gRPC target groups only route behind an HTTPS listener
		conditions  func() lb.ListenerRuleConditionArray
	}{
		{
			name:        "m1-host",
			priority:    10,
			target:      "m1-assignment",
			requiresTls: true,
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
			// Direct ConnectRPC access to M5 (grpcurl, server SDKs). The
			// UI does NOT use this path — its /api/rpc/* calls go to the
			// M6 BFF via the catch-all. This rule also keeps the M5 target
			// group attached to the ALB (an ECS service precondition).
			name:     "m5-api",
			priority: 20,
			target:   "m5-management",
			conditions: func() lb.ListenerRuleConditionArray {
				return lb.ListenerRuleConditionArray{
					&lb.ListenerRuleConditionArgs{
						PathPattern: &lb.ListenerRuleConditionPathPatternArgs{
							Values: pulumi.StringArray{pulumi.String("/experimentation.management.*")},
						},
					},
				}
			},
		},
		{
			name:        "m7-flags",
			priority:    30,
			target:      "m7-flags",
			requiresTls: true,
			conditions: func() lb.ListenerRuleConditionArray {
				return lb.ListenerRuleConditionArray{
					&lb.ListenerRuleConditionArgs{
						PathPattern: &lb.ListenerRuleConditionPathPatternArgs{
							Values: pulumi.StringArray{pulumi.String("/experimentation.flags.*")},
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

	var ruleResources []pulumi.Resource
	for _, rule := range rules {
		if rule.requiresTls && !inputs.TlsEnabled {
			continue
		}
		lr, err := lb.NewListenerRule(ctx, fmt.Sprintf("%s-rule-%s", prefix, rule.name), &lb.ListenerRuleArgs{
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
		ruleResources = append(ruleResources, lr)
	}

	return &TargetGroupOutputs{
		Rules:                   ruleResources,
		M1AssignmentTgArn:       tgArns["m1-assignment"],
		M5ManagementTgArn:       tgArns["m5-management"],
		M6UITgArn:               tgArns["m6-ui"],
		M7FlagsTgArn:            tgArns["m7-flags"],
		M1AssignmentTgArnSuffix: tgArnSuffixes["m1-assignment"],
		M7FlagsTgArnSuffix:      tgArnSuffixes["m7-flags"],
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
		// Explicit Name + ForceNew fields (e.g. protocolVersion flipping with
		// TlsEnabled) would otherwise collide: create-replacement-first hits
		// "duplicate target group name".
	}, pulumi.DeleteBeforeReplace(true))
	if err != nil {
		return nil, fmt.Errorf("creating target group %q: %w", spec.name, err)
	}

	return tg, nil
}
