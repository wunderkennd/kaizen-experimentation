// Package waf provisions an AWS WAF v2 web ACL for the Kaizen ALB.
//
// Sprint I.2.7: WAF web ACL with rate limiting, AWS managed rule sets,
// optional geo-restriction, and S3 logging. Toggleable via kaizen:wafEnabled.
package waf

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/s3"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/wafv2"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// Inputs are the cross-module dependencies consumed by the WAF module.
type Inputs struct {
	// ALB ARN to associate the WAF web ACL with.
	AlbArn pulumi.StringOutput
	// Environment name used for resource naming and tagging.
	Environment string
	// RateLimitPerIP is the maximum requests per 5-minute window per IP.
	// AWS WAF evaluates the rate every 30 seconds within a rolling 5-min window.
	RateLimitPerIP int
	// BlockedCountries is an optional list of ISO 3166-1 alpha-2 country codes
	// to block. When empty, no geo-restriction rule is created.
	BlockedCountries []string
}

// Outputs are exported for downstream consumers (observability, dashboards).
type Outputs struct {
	// WebAclArn is the ARN of the WAF web ACL.
	WebAclArn pulumi.StringOutput
	// WebAclId is the resource ID of the WAF web ACL.
	WebAclId pulumi.IDOutput
	// LogBucketName is the S3 bucket name receiving WAF logs.
	LogBucketName pulumi.StringOutput
}

// New creates the WAF v2 web ACL and attaches it to the ALB.
//
// Rule evaluation order (by priority):
//  1. Rate limiting (priority 1) — blocks IPs exceeding the threshold
//  2. Geo-restriction (priority 2) — blocks traffic from listed countries (optional)
//  3. AWS Common Rule Set (priority 10) — OWASP top-10 protections
//  4. AWS SQLi Rule Set (priority 20) — SQL injection protections
func New(ctx *pulumi.Context, inputs *Inputs) (*Outputs, error) {
	namePrefix := fmt.Sprintf("kaizen-%s", inputs.Environment)
	tags := config.MergeTags(config.DefaultTags(inputs.Environment), pulumi.StringMap{
		"Module": pulumi.String("waf"),
	})

	// ── Build rules ────────────────────────────────────────────────────
	rules := wafv2.WebAclRuleArray{}
	priority := 0

	// Rule 1: Rate limiting per source IP.
	priority++
	rules = append(rules, rateLimit(priority, inputs.RateLimitPerIP))

	// Rule 2: Geo-restriction (only if countries are specified).
	if len(inputs.BlockedCountries) > 0 {
		priority++
		rules = append(rules, geoBlock(priority, inputs.BlockedCountries))
	}

	// Rule 3: AWS Managed Rules — Common Rule Set.
	rules = append(rules, managedRuleGroup(10, "AWSManagedRulesCommonRuleSet", "AWS"))

	// Rule 4: AWS Managed Rules — SQL Injection Rule Set.
	rules = append(rules, managedRuleGroup(20, "AWSManagedRulesSQLiRuleSet", "AWS"))

	// ── Web ACL ────────────────────────────────────────────────────────
	webAcl, err := wafv2.NewWebAcl(ctx, fmt.Sprintf("%s-waf", namePrefix), &wafv2.WebAclArgs{
		Name:        pulumi.Sprintf("%s-waf", namePrefix),
		Description: pulumi.String("WAF web ACL for Kaizen ALB — rate limiting, managed rules, geo-restriction"),
		Scope:       pulumi.String("REGIONAL"),

		DefaultAction: &wafv2.WebAclDefaultActionArgs{
			Allow: &wafv2.WebAclDefaultActionAllowArgs{},
		},

		VisibilityConfig: &wafv2.WebAclVisibilityConfigArgs{
			CloudwatchMetricsEnabled: pulumi.Bool(true),
			MetricName:               pulumi.Sprintf("%s-waf", namePrefix),
			SampledRequestsEnabled:   pulumi.Bool(true),
		},

		Rules: rules,
		Tags:  tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating WAF web ACL: %w", err)
	}

	// ── ALB association ────────────────────────────────────────────────
	_, err = wafv2.NewWebAclAssociation(ctx, fmt.Sprintf("%s-waf-alb", namePrefix), &wafv2.WebAclAssociationArgs{
		ResourceArn: inputs.AlbArn,
		WebAclArn:   webAcl.Arn,
	}, pulumi.DependsOn([]pulumi.Resource{webAcl}))
	if err != nil {
		return nil, fmt.Errorf("associating WAF with ALB: %w", err)
	}

	// ── WAF logging to S3 ──────────────────────────────────────────────
	// AWS requires the S3 bucket name to start with "aws-waf-logs-".
	logBucket, err := newWafLogBucket(ctx, namePrefix, inputs.Environment, tags)
	if err != nil {
		return nil, fmt.Errorf("creating WAF log bucket: %w", err)
	}

	_, err = wafv2.NewWebAclLoggingConfiguration(ctx, fmt.Sprintf("%s-waf-logging", namePrefix), &wafv2.WebAclLoggingConfigurationArgs{
		ResourceArn:            webAcl.Arn,
		LogDestinationConfigs: pulumi.StringArray{logBucket.Arn},
	}, pulumi.DependsOn([]pulumi.Resource{webAcl, logBucket}))
	if err != nil {
		return nil, fmt.Errorf("configuring WAF logging: %w", err)
	}

	// ── Exports ────────────────────────────────────────────────────────
	ctx.Export("wafWebAclArn", webAcl.Arn)
	ctx.Export("wafLogBucketName", logBucket.Bucket)

	return &Outputs{
		WebAclArn:     webAcl.Arn,
		WebAclId:      webAcl.ID(),
		LogBucketName: logBucket.Bucket,
	}, nil
}

// ---------------------------------------------------------------------------
// Rule builders
// ---------------------------------------------------------------------------

// rateLimit creates a rate-based rule that blocks source IPs exceeding the
// configured threshold within a 5-minute evaluation window.
func rateLimit(priority, limit int) wafv2.WebAclRuleArgs {
	return wafv2.WebAclRuleArgs{
		Name:     pulumi.String("rate-limit-per-ip"),
		Priority: pulumi.Int(priority),

		Action: &wafv2.WebAclRuleActionArgs{
			Block: &wafv2.WebAclRuleActionBlockArgs{},
		},

		Statement: &wafv2.WebAclRuleStatementArgs{
			RateBasedStatement: &wafv2.WebAclRuleStatementRateBasedStatementArgs{
				Limit:              pulumi.Int(limit),
				AggregateKeyType:   pulumi.String("IP"),
				EvaluationWindowSec: pulumi.Int(300),
			},
		},

		VisibilityConfig: &wafv2.WebAclRuleVisibilityConfigArgs{
			CloudwatchMetricsEnabled: pulumi.Bool(true),
			MetricName:               pulumi.String("rate-limit-per-ip"),
			SampledRequestsEnabled:   pulumi.Bool(true),
		},
	}
}

// geoBlock creates a rule that blocks requests originating from the specified countries.
func geoBlock(priority int, countryCodes []string) wafv2.WebAclRuleArgs {
	codes := make(pulumi.StringArray, len(countryCodes))
	for i, c := range countryCodes {
		codes[i] = pulumi.String(c)
	}

	return wafv2.WebAclRuleArgs{
		Name:     pulumi.String("geo-block"),
		Priority: pulumi.Int(priority),

		Action: &wafv2.WebAclRuleActionArgs{
			Block: &wafv2.WebAclRuleActionBlockArgs{},
		},

		Statement: &wafv2.WebAclRuleStatementArgs{
			GeoMatchStatement: &wafv2.WebAclRuleStatementGeoMatchStatementArgs{
				CountryCodes: codes,
			},
		},

		VisibilityConfig: &wafv2.WebAclRuleVisibilityConfigArgs{
			CloudwatchMetricsEnabled: pulumi.Bool(true),
			MetricName:               pulumi.String("geo-block"),
			SampledRequestsEnabled:   pulumi.Bool(true),
		},
	}
}

// managedRuleGroup creates a rule referencing an AWS-managed rule group.
// Override action is set to "none" so the managed rule's own actions apply.
func managedRuleGroup(priority int, name, vendorName string) wafv2.WebAclRuleArgs {
	return wafv2.WebAclRuleArgs{
		Name:     pulumi.String(name),
		Priority: pulumi.Int(priority),

		OverrideAction: &wafv2.WebAclRuleOverrideActionArgs{
			None: &wafv2.WebAclRuleOverrideActionNoneArgs{},
		},

		Statement: &wafv2.WebAclRuleStatementArgs{
			ManagedRuleGroupStatement: &wafv2.WebAclRuleStatementManagedRuleGroupStatementArgs{
				Name:       pulumi.String(name),
				VendorName: pulumi.String(vendorName),
			},
		},

		VisibilityConfig: &wafv2.WebAclRuleVisibilityConfigArgs{
			CloudwatchMetricsEnabled: pulumi.Bool(true),
			MetricName:               pulumi.String(name),
			SampledRequestsEnabled:   pulumi.Bool(true),
		},
	}
}

// ---------------------------------------------------------------------------
// WAF log bucket
// ---------------------------------------------------------------------------

// newWafLogBucket creates an S3 bucket for WAF log delivery.
// AWS requires the bucket name to begin with "aws-waf-logs-".
func newWafLogBucket(ctx *pulumi.Context, namePrefix, env string, tags pulumi.StringMap) (*s3.BucketV2, error) {
	bucketName := fmt.Sprintf("aws-waf-logs-%s", namePrefix)

	bucket, err := s3.NewBucketV2(ctx, "waf-log-bucket", &s3.BucketV2Args{
		Bucket:       pulumi.String(bucketName),
		ForceDestroy: pulumi.Bool(env == "dev"),
		Tags: config.MergeTags(tags, pulumi.StringMap{
			"Component": pulumi.String("waf-logs"),
		}),
	})
	if err != nil {
		return nil, err
	}

	// Ownership controls — bucket owner enforced (no ACLs).
	if _, err := s3.NewBucketOwnershipControls(ctx, "waf-log-bucket-ownership", &s3.BucketOwnershipControlsArgs{
		Bucket: bucket.ID(),
		Rule: &s3.BucketOwnershipControlsRuleArgs{
			ObjectOwnership: pulumi.String("BucketOwnerEnforced"),
		},
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	// Block all public access.
	if _, err := s3.NewBucketPublicAccessBlock(ctx, "waf-log-bucket-pab", &s3.BucketPublicAccessBlockArgs{
		Bucket:                bucket.ID(),
		BlockPublicAcls:       pulumi.Bool(true),
		BlockPublicPolicy:     pulumi.Bool(true),
		IgnorePublicAcls:      pulumi.Bool(true),
		RestrictPublicBuckets: pulumi.Bool(true),
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	// SSE-S3 encryption.
	if _, err := s3.NewBucketServerSideEncryptionConfigurationV2(ctx, "waf-log-bucket-sse", &s3.BucketServerSideEncryptionConfigurationV2Args{
		Bucket: bucket.ID(),
		Rules: s3.BucketServerSideEncryptionConfigurationV2RuleArray{
			&s3.BucketServerSideEncryptionConfigurationV2RuleArgs{
				ApplyServerSideEncryptionByDefault: &s3.BucketServerSideEncryptionConfigurationV2RuleApplyServerSideEncryptionByDefaultArgs{
					SseAlgorithm: pulumi.String("AES256"),
				},
			},
		},
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	// Lifecycle: expire WAF logs after 90 days.
	if _, err := s3.NewBucketLifecycleConfigurationV2(ctx, "waf-log-bucket-lifecycle", &s3.BucketLifecycleConfigurationV2Args{
		Bucket: bucket.ID(),
		Rules: s3.BucketLifecycleConfigurationV2RuleArray{
			&s3.BucketLifecycleConfigurationV2RuleArgs{
				Id:     pulumi.String("expire-waf-logs"),
				Status: pulumi.String("Enabled"),
				Filter: &s3.BucketLifecycleConfigurationV2RuleFilterArgs{},
				Expiration: &s3.BucketLifecycleConfigurationV2RuleExpirationArgs{
					Days: pulumi.Int(90),
				},
				AbortIncompleteMultipartUpload: &s3.BucketLifecycleConfigurationV2RuleAbortIncompleteMultipartUploadArgs{
					DaysAfterInitiation: pulumi.Int(7),
				},
			},
		},
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	return bucket, nil
}
