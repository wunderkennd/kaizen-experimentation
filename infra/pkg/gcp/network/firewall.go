package network

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/compute"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// FirewallArgs holds the inputs for creating the GCP firewall rules.
type FirewallArgs struct {
	NetworkId pulumi.IDOutput
}

// FirewallResult holds the created firewall rule resource IDs keyed by role.
// Keys mirror the AWS module exactly: "alb", "ecs", "rds", "msk", "redis",
// "m4b". The values are GCP firewall rule IDs (the rule name) — the
// NetworkOutputs contract documents this provider-specific interpretation.
type FirewallResult struct {
	Rules map[string]pulumi.IDOutput
}

// Network tags that GCP resources opt into to receive a firewall rule's
// inbound traffic. Keys here match the AWS security-group keys exactly so
// downstream modules can use the same names regardless of provider.
const (
	tagAlb   = "kaizen-alb"
	tagEcs   = "kaizen-ecs"
	tagRds   = "kaizen-rds"
	tagMsk   = "kaizen-msk"
	tagRedis = "kaizen-redis"
	tagM4b   = "kaizen-m4b"
)

// NewFirewallRules creates 6 firewall rules whose target tags match the
// security-group key names exposed by the AWS module. GCP firewall rules
// are network-wide; resources opt into a rule by adding the matching network
// tag (or service account, for Cloud Run via the VPC connector).
//
// Traffic flow mirrors the AWS module:
//
//	Internet ─443→ [tag:kaizen-alb]
//	[tag:kaizen-alb] ─tcp→ [tag:kaizen-ecs / kaizen-m4b]
//	[tag:kaizen-ecs / kaizen-m4b] ─5432→ [tag:kaizen-rds]
//	[tag:kaizen-ecs / kaizen-m4b] ─9092/9094/9096→ [tag:kaizen-msk]
//	[tag:kaizen-ecs / kaizen-m4b] ─6379→ [tag:kaizen-redis]
//	[tag:kaizen-ecs] ↔ [tag:kaizen-m4b]   (cross-compute gRPC)
func NewFirewallRules(ctx *pulumi.Context, args *FirewallArgs) (*FirewallResult, error) {
	netRef := args.NetworkId.ToStringOutput()

	// ── ALB equivalent: ingress 443 from the internet ─────────────────
	albFw, err := compute.NewFirewall(ctx, "kaizen-fw-alb", &compute.FirewallArgs{
		Name:        pulumi.String("kaizen-fw-alb"),
		Network:     netRef,
		Direction:   pulumi.String("INGRESS"),
		Description: pulumi.String("ALB equivalent — public HTTPS ingress"),
		SourceRanges: pulumi.StringArray{
			pulumi.String("0.0.0.0/0"),
		},
		TargetTags: pulumi.StringArray{pulumi.String(tagAlb)},
		Allows: compute.FirewallAllowArray{
			&compute.FirewallAllowArgs{
				Protocol: pulumi.String("tcp"),
				Ports:    pulumi.StringArray{pulumi.String("443")},
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("alb firewall: %w", err)
	}

	// ── ECS equivalent: from ALB / self / M4b on all TCP ──────────────
	// Mirrors the AWS ecs-sg ingress (alb, self, m4b) — GCP firewall rules
	// support multiple sourceTags so all three sources collapse into one
	// rule rather than the three separate AWS rules.
	ecsFw, err := compute.NewFirewall(ctx, "kaizen-fw-ecs", &compute.FirewallArgs{
		Name:        pulumi.String("kaizen-fw-ecs"),
		Network:     netRef,
		Direction:   pulumi.String("INGRESS"),
		Description: pulumi.String("ECS/Cloud Run equivalent — inter-service gRPC"),
		SourceTags: pulumi.StringArray{
			pulumi.String(tagAlb),
			pulumi.String(tagEcs),
			pulumi.String(tagM4b),
		},
		TargetTags: pulumi.StringArray{pulumi.String(tagEcs)},
		Allows: compute.FirewallAllowArray{
			&compute.FirewallAllowArgs{
				Protocol: pulumi.String("tcp"),
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("ecs firewall: %w", err)
	}

	// ── RDS equivalent: PostgreSQL 5432 from ECS/M4b ──────────────────
	rdsFw, err := compute.NewFirewall(ctx, "kaizen-fw-rds", &compute.FirewallArgs{
		Name:        pulumi.String("kaizen-fw-rds"),
		Network:     netRef,
		Direction:   pulumi.String("INGRESS"),
		Description: pulumi.String("Cloud SQL equivalent — PostgreSQL from ECS/M4b only"),
		SourceTags: pulumi.StringArray{
			pulumi.String(tagEcs),
			pulumi.String(tagM4b),
		},
		TargetTags: pulumi.StringArray{pulumi.String(tagRds)},
		Allows: compute.FirewallAllowArray{
			&compute.FirewallAllowArgs{
				Protocol: pulumi.String("tcp"),
				Ports:    pulumi.StringArray{pulumi.String("5432")},
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("rds firewall: %w", err)
	}

	// ── MSK equivalent: Kafka 9092/9094/9096 from ECS/M4b ─────────────
	mskFw, err := compute.NewFirewall(ctx, "kaizen-fw-msk", &compute.FirewallArgs{
		Name:        pulumi.String("kaizen-fw-msk"),
		Network:     netRef,
		Direction:   pulumi.String("INGRESS"),
		Description: pulumi.String("Kafka equivalent — plaintext/TLS/SASL_SSL from ECS/M4b only"),
		SourceTags: pulumi.StringArray{
			pulumi.String(tagEcs),
			pulumi.String(tagM4b),
		},
		TargetTags: pulumi.StringArray{pulumi.String(tagMsk)},
		Allows: compute.FirewallAllowArray{
			&compute.FirewallAllowArgs{
				Protocol: pulumi.String("tcp"),
				Ports: pulumi.StringArray{
					pulumi.String("9092"),
					pulumi.String("9094"),
					pulumi.String("9096"),
				},
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("msk firewall: %w", err)
	}

	// ── Redis equivalent: 6379 from ECS/M4b ───────────────────────────
	redisFw, err := compute.NewFirewall(ctx, "kaizen-fw-redis", &compute.FirewallArgs{
		Name:        pulumi.String("kaizen-fw-redis"),
		Network:     netRef,
		Direction:   pulumi.String("INGRESS"),
		Description: pulumi.String("Memorystore Redis equivalent — 6379 from ECS/M4b only"),
		SourceTags: pulumi.StringArray{
			pulumi.String(tagEcs),
			pulumi.String(tagM4b),
		},
		TargetTags: pulumi.StringArray{pulumi.String(tagRedis)},
		Allows: compute.FirewallAllowArray{
			&compute.FirewallAllowArgs{
				Protocol: pulumi.String("tcp"),
				Ports:    pulumi.StringArray{pulumi.String("6379")},
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("redis firewall: %w", err)
	}

	// ── M4b: same surface as ECS (alb / self / ecs) ───────────────────
	m4bFw, err := compute.NewFirewall(ctx, "kaizen-fw-m4b", &compute.FirewallArgs{
		Name:        pulumi.String("kaizen-fw-m4b"),
		Network:     netRef,
		Direction:   pulumi.String("INGRESS"),
		Description: pulumi.String("M4b Policy service — mirrors ECS rule surface"),
		SourceTags: pulumi.StringArray{
			pulumi.String(tagAlb),
			pulumi.String(tagEcs),
			pulumi.String(tagM4b),
		},
		TargetTags: pulumi.StringArray{pulumi.String(tagM4b)},
		Allows: compute.FirewallAllowArray{
			&compute.FirewallAllowArgs{
				Protocol: pulumi.String("tcp"),
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("m4b firewall: %w", err)
	}

	return &FirewallResult{
		Rules: map[string]pulumi.IDOutput{
			"alb":   albFw.ID(),
			"ecs":   ecsFw.ID(),
			"rds":   rdsFw.ID(),
			"msk":   mskFw.ID(),
			"redis": redisFw.ID(),
			"m4b":   m4bFw.ID(),
		},
	}, nil
}
