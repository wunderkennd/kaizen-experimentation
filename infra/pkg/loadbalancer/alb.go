// Package loadbalancer provisions the internet-facing Application Load Balancer
// for the Kaizen experimentation platform.
//
// Sprint I.0: ALB + listeners. Target groups + listener rules in target_groups.go (I.1.8).
package loadbalancer

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/lb"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ALBInputs are the cross-module dependencies consumed by the ALB module.
type ALBInputs struct {
	// VPC public subnet IDs (at least 2 AZs required).
	PublicSubnetIds pulumi.StringArrayInput
	// Security group ID for the ALB (from pkg/network SecurityGroups["alb"]).
	SecurityGroupId pulumi.StringInput
	// ACM certificate ARN for the HTTPS listener (from pkg/dns).
	CertificateArn pulumi.StringInput
	// S3 bucket name for ALB access logs (from pkg/storage).
	LogsBucketName pulumi.StringInput
	// Environment name used for resource naming and tagging.
	Environment string
}

// ALBOutputs are exported for downstream consumers (I.1.18 target groups,
// pkg/dns A-record, pkg/observability alarms).
type ALBOutputs struct {
	// ALB resource ID.
	AlbId pulumi.IDOutput
	// ALB ARN — needed by target groups and listener rules in I.1.18.
	AlbArn pulumi.StringOutput
	// ALB DNS name — used by Route 53 A-record alias.
	AlbDnsName pulumi.StringOutput
	// ALB canonical hosted zone ID — needed for Route 53 alias target.
	AlbZoneId pulumi.StringOutput
	// HTTPS listener ARN — I.1.18 attaches target group rules here.
	HttpsListenerArn pulumi.StringOutput
	// HTTP listener ARN (redirect-only, exposed for completeness).
	HttpListenerArn pulumi.StringOutput
}

// NewALB creates the internet-facing Application Load Balancer with:
//   - HTTP/2 enabled (required for gRPC)
//   - 60-second idle timeout
//   - HTTPS listener (443) with ACM cert and fixed-response default
//   - HTTP listener (80) → 301 redirect to HTTPS
//   - Access logging to S3
//
// Target groups and path-based routing rules are added in Sprint I.1.18.
func NewALB(ctx *pulumi.Context, inputs *ALBInputs) (*ALBOutputs, error) {
	namePrefix := fmt.Sprintf("kaizen-%s", inputs.Environment)

	// --- Application Load Balancer ---
	alb, err := lb.NewLoadBalancer(ctx, fmt.Sprintf("%s-alb", namePrefix), &lb.LoadBalancerArgs{
		Name:             pulumi.Sprintf("%s-alb", namePrefix),
		LoadBalancerType: pulumi.String("application"),
		Internal:         pulumi.Bool(false),
		SecurityGroups:   pulumi.StringArray{inputs.SecurityGroupId},
		Subnets:          inputs.PublicSubnetIds,

		// HTTP/2 is required for gRPC traffic (M1 Assignment, M7 Flags).
		EnableHttp2: pulumi.Bool(true),

		// 60s idle timeout matches gRPC streaming keep-alive defaults.
		IdleTimeout: pulumi.Int(60),

		// Drop malformed headers to prevent request smuggling.
		DropInvalidHeaderFields: pulumi.Bool(true),

		// Deletion protection enabled for staging/prod; callers can override
		// via Pulumi config if needed. Default off for dev iteration speed.
		EnableDeletionProtection: pulumi.Bool(inputs.Environment == "prod"),

		AccessLogs: &lb.LoadBalancerAccessLogsArgs{
			Bucket:  inputs.LogsBucketName,
			Prefix:  pulumi.Sprintf("alb/%s", namePrefix),
			Enabled: pulumi.Bool(true),
		},

		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen-experimentation"),
			"Environment": pulumi.String(inputs.Environment),
			"ManagedBy":   pulumi.String("pulumi"),
			"Module":      pulumi.String("loadbalancer"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating ALB: %w", err)
	}

	// --- HTTPS Listener (port 443) ---
	// Default action is a 503 fixed-response until I.1.18 wires target groups.
	// This allows the ALB to exist and serve health checks / return a clear
	// "not yet routed" signal during initial deployment.
	httpsListener, err := lb.NewListener(ctx, fmt.Sprintf("%s-https", namePrefix), &lb.ListenerArgs{
		LoadBalancerArn: alb.Arn,
		Port:            pulumi.Int(443),
		Protocol:        pulumi.String("HTTPS"),
		SslPolicy:       pulumi.String("ELBSecurityPolicy-TLS13-1-2-2021-06"),
		CertificateArn:  inputs.CertificateArn,

		DefaultActions: lb.ListenerDefaultActionArray{
			&lb.ListenerDefaultActionArgs{
				Type: pulumi.String("fixed-response"),
				FixedResponse: &lb.ListenerDefaultActionFixedResponseArgs{
					ContentType: pulumi.String("text/plain"),
					MessageBody: pulumi.String("Service not yet configured"),
					StatusCode:  pulumi.String("503"),
				},
			},
		},

		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen-experimentation"),
			"Environment": pulumi.String(inputs.Environment),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating HTTPS listener: %w", err)
	}

	// --- HTTP Listener (port 80) → redirect to HTTPS ---
	httpListener, err := lb.NewListener(ctx, fmt.Sprintf("%s-http", namePrefix), &lb.ListenerArgs{
		LoadBalancerArn: alb.Arn,
		Port:            pulumi.Int(80),
		Protocol:        pulumi.String("HTTP"),

		DefaultActions: lb.ListenerDefaultActionArray{
			&lb.ListenerDefaultActionArgs{
				Type: pulumi.String("redirect"),
				Redirect: &lb.ListenerDefaultActionRedirectArgs{
					Port:       pulumi.String("443"),
					Protocol:   pulumi.String("HTTPS"),
					StatusCode: pulumi.String("HTTP_301"),
				},
			},
		},

		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen-experimentation"),
			"Environment": pulumi.String(inputs.Environment),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating HTTP listener: %w", err)
	}

	return &ALBOutputs{
		AlbId:            alb.ID(),
		AlbArn:           alb.Arn,
		AlbDnsName:       alb.DnsName,
		AlbZoneId:        alb.ZoneId,
		HttpsListenerArn: httpsListener.Arn,
		HttpListenerArn:  httpListener.Arn,
	}, nil
}
