// Package storage provisions S3 buckets for the Kaizen experimentation
// platform: Delta Lake data, MLflow artifacts, and ALB access logs.
package storage

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/elb"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/s3"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// StorageOutputs is the cross-agent contract for S3 resources.
type StorageOutputs struct {
	DataBucketName   pulumi.StringOutput
	DataBucketArn    pulumi.StringOutput
	MlflowBucketName pulumi.StringOutput
	MlflowBucketArn  pulumi.StringOutput
	LogsBucketName   pulumi.StringOutput
	LogsBucketArn    pulumi.StringOutput
}

// StorageInputs contains VPC-level resources injected by the caller.
type StorageInputs struct {
	// S3VpcEndpointId is the Gateway VPC endpoint ID used to restrict
	// bucket access to VPC-internal traffic only.
	S3VpcEndpointId pulumi.IDOutput
}

// NewStorage provisions the three S3 buckets and returns their outputs.
func NewStorage(ctx *pulumi.Context, env string, inputs *StorageInputs) (*StorageOutputs, error) {
	tags := config.DefaultTags(env)

	data, err := newDataBucket(ctx, env, tags)
	if err != nil {
		return nil, fmt.Errorf("data bucket: %w", err)
	}

	mlflow, err := newMlflowBucket(ctx, env, tags)
	if err != nil {
		return nil, fmt.Errorf("mlflow bucket: %w", err)
	}

	logs, err := newLogsBucket(ctx, env, tags)
	if err != nil {
		return nil, fmt.Errorf("logs bucket: %w", err)
	}

	// VPC endpoint policies for data and mlflow buckets restrict access to
	// traffic originating from the VPC gateway endpoint. The logs bucket is
	// excluded — ALB writes logs via the AWS ELB service account, not the VPC.
	if inputs != nil {
		if err := applyVpcEndpointPolicy(ctx, "data", data, inputs.S3VpcEndpointId); err != nil {
			return nil, fmt.Errorf("data bucket vpc policy: %w", err)
		}
		if err := applyVpcEndpointPolicy(ctx, "mlflow", mlflow, inputs.S3VpcEndpointId); err != nil {
			return nil, fmt.Errorf("mlflow bucket vpc policy: %w", err)
		}
	}

	return &StorageOutputs{
		DataBucketName:   data.Bucket,
		DataBucketArn:    data.Arn,
		MlflowBucketName: mlflow.Bucket,
		MlflowBucketArn:  mlflow.Arn,
		LogsBucketName:   logs.Bucket,
		LogsBucketArn:    logs.Arn,
	}, nil
}

// ---------------------------------------------------------------------------
// kaizen-{env}-data — Delta Lake storage
// ---------------------------------------------------------------------------

func newDataBucket(ctx *pulumi.Context, env string, tags pulumi.StringMap) (*s3.BucketV2, error) {
	name := fmt.Sprintf("kaizen-%s-data", env)

	bucket, err := s3.NewBucketV2(ctx, "data-bucket", &s3.BucketV2Args{
		Bucket:       pulumi.String(name),
		ForceDestroy: pulumi.Bool(env == "dev"),
		Tags: config.MergeTags(tags, pulumi.StringMap{
			"Component": pulumi.String("delta-lake"),
		}),
	})
	if err != nil {
		return nil, err
	}

	if err := applyBucketDefaults(ctx, "data", bucket); err != nil {
		return nil, err
	}

	// Versioning — required for Delta Lake's ACID guarantees.
	if _, err := s3.NewBucketVersioningV2(ctx, "data-bucket-versioning", &s3.BucketVersioningV2Args{
		Bucket: bucket.ID(),
		VersioningConfiguration: &s3.BucketVersioningV2VersioningConfigurationArgs{
			Status: pulumi.String("Enabled"),
		},
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	// SSE-S3 (AES256) — simplest default; upgrade to KMS if needed later.
	if _, err := s3.NewBucketServerSideEncryptionConfigurationV2(ctx, "data-bucket-sse", &s3.BucketServerSideEncryptionConfigurationV2Args{
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

	// Lifecycle: Infrequent Access after 90 days, Glacier after 365 days.
	if _, err := s3.NewBucketLifecycleConfigurationV2(ctx, "data-bucket-lifecycle", &s3.BucketLifecycleConfigurationV2Args{
		Bucket: bucket.ID(),
		Rules: s3.BucketLifecycleConfigurationV2RuleArray{
			&s3.BucketLifecycleConfigurationV2RuleArgs{
				Id:     pulumi.String("tiered-storage"),
				Status: pulumi.String("Enabled"),
				Filter: &s3.BucketLifecycleConfigurationV2RuleFilterArgs{},
				Transitions: s3.BucketLifecycleConfigurationV2RuleTransitionArray{
					&s3.BucketLifecycleConfigurationV2RuleTransitionArgs{
						Days:         pulumi.Int(90),
						StorageClass: pulumi.String("STANDARD_IA"),
					},
					&s3.BucketLifecycleConfigurationV2RuleTransitionArgs{
						Days:         pulumi.Int(365),
						StorageClass: pulumi.String("GLACIER"),
					},
				},
				AbortIncompleteMultipartUpload: &s3.BucketLifecycleConfigurationV2RuleAbortIncompleteMultipartUploadArgs{
					DaysAfterInitiation: pulumi.Int(7),
				},
			},
			&s3.BucketLifecycleConfigurationV2RuleArgs{
				Id:     pulumi.String("noncurrent-cleanup"),
				Status: pulumi.String("Enabled"),
				Filter: &s3.BucketLifecycleConfigurationV2RuleFilterArgs{},
				NoncurrentVersionExpiration: &s3.BucketLifecycleConfigurationV2RuleNoncurrentVersionExpirationArgs{
					NoncurrentDays: pulumi.Int(90),
				},
			},
		},
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	return bucket, nil
}

// ---------------------------------------------------------------------------
// kaizen-{env}-mlflow — MLflow artifact storage
// ---------------------------------------------------------------------------

func newMlflowBucket(ctx *pulumi.Context, env string, tags pulumi.StringMap) (*s3.BucketV2, error) {
	name := fmt.Sprintf("kaizen-%s-mlflow", env)

	bucket, err := s3.NewBucketV2(ctx, "mlflow-bucket", &s3.BucketV2Args{
		Bucket:       pulumi.String(name),
		ForceDestroy: pulumi.Bool(env == "dev"),
		Tags: config.MergeTags(tags, pulumi.StringMap{
			"Component": pulumi.String("mlflow"),
		}),
	})
	if err != nil {
		return nil, err
	}

	if err := applyBucketDefaults(ctx, "mlflow", bucket); err != nil {
		return nil, err
	}

	// Versioning — protects model artifacts from accidental overwrites.
	if _, err := s3.NewBucketVersioningV2(ctx, "mlflow-bucket-versioning", &s3.BucketVersioningV2Args{
		Bucket: bucket.ID(),
		VersioningConfiguration: &s3.BucketVersioningV2VersioningConfigurationArgs{
			Status: pulumi.String("Enabled"),
		},
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	// SSE-S3.
	if _, err := s3.NewBucketServerSideEncryptionConfigurationV2(ctx, "mlflow-bucket-sse", &s3.BucketServerSideEncryptionConfigurationV2Args{
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

	return bucket, nil
}

// ---------------------------------------------------------------------------
// kaizen-{env}-logs — ALB access log bucket
// ---------------------------------------------------------------------------

func newLogsBucket(ctx *pulumi.Context, env string, tags pulumi.StringMap) (*s3.BucketV2, error) {
	name := fmt.Sprintf("kaizen-%s-logs", env)

	bucket, err := s3.NewBucketV2(ctx, "logs-bucket", &s3.BucketV2Args{
		Bucket:       pulumi.String(name),
		ForceDestroy: pulumi.Bool(env == "dev"),
		Tags: config.MergeTags(tags, pulumi.StringMap{
			"Component": pulumi.String("alb-logs"),
		}),
	})
	if err != nil {
		return nil, err
	}

	if err := applyBucketDefaults(ctx, "logs", bucket); err != nil {
		return nil, err
	}

	// SSE-S3 — ALB logs require AES256 (not KMS).
	if _, err := s3.NewBucketServerSideEncryptionConfigurationV2(ctx, "logs-bucket-sse", &s3.BucketServerSideEncryptionConfigurationV2Args{
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

	// Lifecycle: delete after 90 days — no archival for access logs.
	if _, err := s3.NewBucketLifecycleConfigurationV2(ctx, "logs-bucket-lifecycle", &s3.BucketLifecycleConfigurationV2Args{
		Bucket: bucket.ID(),
		Rules: s3.BucketLifecycleConfigurationV2RuleArray{
			&s3.BucketLifecycleConfigurationV2RuleArgs{
				Id:     pulumi.String("expire-logs"),
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

	// Bucket policy: grant the regional ELB service account write access.
	elbAccount, err := elb.GetServiceAccount(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("elb service account lookup: %w", err)
	}

	policy, err := iam.GetPolicyDocument(ctx, &iam.GetPolicyDocumentArgs{
		Statements: []iam.GetPolicyDocumentStatement{
			{
				Effect: pulumi.StringRef("Allow"),
				Principals: []iam.GetPolicyDocumentStatementPrincipal{
					{
						Type:        "AWS",
						Identifiers: []string{elbAccount.Arn},
					},
				},
				Actions: []string{"s3:PutObject"},
				Resources: []string{
					fmt.Sprintf("arn:aws:s3:::%s/AWSLogs/*", name),
				},
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("logs bucket policy document: %w", err)
	}

	if _, err := s3.NewBucketPolicy(ctx, "logs-bucket-policy", &s3.BucketPolicyArgs{
		Bucket: bucket.ID(),
		Policy: pulumi.String(policy.Json),
	}, pulumi.Parent(bucket)); err != nil {
		return nil, err
	}

	return bucket, nil
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

// applyVpcEndpointPolicy attaches a bucket policy that denies access unless
// the request originates from the specified S3 Gateway VPC endpoint. This
// ensures data/mlflow bucket traffic stays within the VPC.
func applyVpcEndpointPolicy(ctx *pulumi.Context, prefix string, bucket *s3.BucketV2, vpceId pulumi.IDOutput) error {
	policyJSON := pulumi.All(bucket.Arn, vpceId).ApplyT(func(args []interface{}) string {
		bucketArn := args[0].(string)
		endpointId := args[1].(string)
		return fmt.Sprintf(`{
  "Version": "2012-10-17",
  "Statement": [{
    "Sid": "VPCEndpointOnly",
    "Effect": "Deny",
    "Principal": "*",
    "Action": ["s3:GetObject", "s3:PutObject", "s3:ListBucket"],
    "Resource": ["%s", "%s/*"],
    "Condition": {
      "StringNotEquals": {
        "aws:sourceVpce": "%s"
      }
    }
  }]
}`, bucketArn, bucketArn, endpointId)
	}).(pulumi.StringOutput)

	_, err := s3.NewBucketPolicy(ctx, prefix+"-bucket-vpce-policy", &s3.BucketPolicyArgs{
		Bucket: bucket.ID(),
		Policy: policyJSON,
	}, pulumi.Parent(bucket))
	return err
}

// applyBucketDefaults configures ownership controls and public access block
// on every bucket. These are security baselines — all buckets are private.
func applyBucketDefaults(ctx *pulumi.Context, prefix string, bucket *s3.BucketV2) error {
	if _, err := s3.NewBucketOwnershipControls(ctx, prefix+"-bucket-ownership", &s3.BucketOwnershipControlsArgs{
		Bucket: bucket.ID(),
		Rule: &s3.BucketOwnershipControlsRuleArgs{
			ObjectOwnership: pulumi.String("BucketOwnerEnforced"),
		},
	}, pulumi.Parent(bucket)); err != nil {
		return err
	}

	if _, err := s3.NewBucketPublicAccessBlock(ctx, prefix+"-bucket-pab", &s3.BucketPublicAccessBlockArgs{
		Bucket:                bucket.ID(),
		BlockPublicAcls:       pulumi.Bool(true),
		BlockPublicPolicy:     pulumi.Bool(true),
		IgnorePublicAcls:      pulumi.Bool(true),
		RestrictPublicBuckets: pulumi.Bool(true),
	}, pulumi.Parent(bucket)); err != nil {
		return err
	}

	return nil
}
