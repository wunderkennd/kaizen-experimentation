// Package storage provisions Cloud Storage buckets for the Kaizen
// experimentation platform: Delta Lake data, MLflow artifacts, and load
// balancer access logs. It mirrors the AWS storage module (pkg/aws/storage)
// in bucket inventory, lifecycle policy, and versioning intent — translated
// into GCS-native primitives:
//
//   - SSE-S3 (AES256)        → Google-managed CSEK at rest (default).
//   - BucketOwnershipControls + PublicAccessBlock
//                            → UniformBucketLevelAccess + PublicAccessPrevention.
//   - VPC-endpoint deny policy on data/mlflow buckets
//                            → deferred to a follow-up PR using VPC Service
//                              Controls (org-level resource), not bucket IAM.
//
// IAM bindings to compute service accounts (Workload Identity → bucket
// access) are intentionally NOT created here. The compute module will add
// `storage.BucketIAMBinding` resources once it provisions the runtime
// service accounts. See issue tracker for the compute Phase 1 task.
package storage

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/storage"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// StorageOutputs is the GCP-side equivalent of the AWS storage module's
// output struct. Bucket fields hold the bare bucket name (matching AWS
// `Bucket`) and the `gs://`-prefixed URI used as the cross-provider Ref.
type StorageOutputs struct {
	DataBucketName   pulumi.StringOutput
	DataBucketURI    pulumi.StringOutput
	MlflowBucketName pulumi.StringOutput
	MlflowBucketURI  pulumi.StringOutput
	LogsBucketName   pulumi.StringOutput
	LogsBucketURI    pulumi.StringOutput
}

// StorageInputs holds optional inputs from sibling modules. Currently
// unused; reserved so the signature stays parallel to AWS's StorageInputs
// and so future inputs (e.g., a CMEK key from a KMS module) don't force a
// signature change downstream.
type StorageInputs struct{}

// NewStorage provisions the three Cloud Storage buckets and returns their
// outputs. Bucket names are deterministic and globally unique within the
// project: `kaizen-{env}-{role}`. Region is inherited from the gcp provider
// configuration (typically pinned per-stack in Pulumi.{env}.yaml).
func NewStorage(ctx *pulumi.Context, env string, _ *StorageInputs) (*StorageOutputs, error) {
	labels := defaultLabels(env)

	data, err := newDataBucket(ctx, env, labels)
	if err != nil {
		return nil, fmt.Errorf("data bucket: %w", err)
	}

	mlflow, err := newMlflowBucket(ctx, env, labels)
	if err != nil {
		return nil, fmt.Errorf("mlflow bucket: %w", err)
	}

	logs, err := newLogsBucket(ctx, env, labels)
	if err != nil {
		return nil, fmt.Errorf("logs bucket: %w", err)
	}

	return &StorageOutputs{
		DataBucketName:   data.Name,
		DataBucketURI:    data.Url,
		MlflowBucketName: mlflow.Name,
		MlflowBucketURI:  mlflow.Url,
		LogsBucketName:   logs.Name,
		LogsBucketURI:    logs.Url,
	}, nil
}

// ---------------------------------------------------------------------------
// kaizen-{env}-data — Delta Lake storage
// ---------------------------------------------------------------------------
//
// AWS parity:
//   - versioning enabled (Delta Lake ACID guarantees)
//   - lifecycle: STANDARD → STANDARD_IA @ 90d → GLACIER @ 365d
//   - abort incomplete multipart upload @ 7d
//   - noncurrent version expiration @ 90d
//
// GCS translation:
//   - storage.BucketVersioning enabled
//   - lifecycle SetStorageClass NEARLINE @ 90d, COLDLINE @ 365d
//   - lifecycle Delete on AbortIncompleteMultipartUpload age=7
//   - lifecycle Delete on noncurrent versions age=90

func newDataBucket(ctx *pulumi.Context, env string, labels pulumi.StringMap) (*storage.Bucket, error) {
	name := fmt.Sprintf("kaizen-%s-data", env)
	forceDestroy := env == "dev"

	return storage.NewBucket(ctx, "data-bucket", &storage.BucketArgs{
		Name:                     pulumi.String(name),
		Location:                 pulumi.String("US"),
		ForceDestroy:             pulumi.Bool(forceDestroy),
		UniformBucketLevelAccess: pulumi.Bool(true),
		PublicAccessPrevention:   pulumi.String("enforced"),
		Labels: mergeLabels(labels, pulumi.StringMap{
			"component": pulumi.String("delta-lake"),
		}),
		Versioning: &storage.BucketVersioningArgs{
			Enabled: pulumi.Bool(true),
		},
		LifecycleRules: storage.BucketLifecycleRuleArray{
			&storage.BucketLifecycleRuleArgs{
				Action: &storage.BucketLifecycleRuleActionArgs{
					Type:         pulumi.String("SetStorageClass"),
					StorageClass: pulumi.String("NEARLINE"),
				},
				Condition: &storage.BucketLifecycleRuleConditionArgs{
					Age: pulumi.Int(90),
				},
			},
			&storage.BucketLifecycleRuleArgs{
				Action: &storage.BucketLifecycleRuleActionArgs{
					Type:         pulumi.String("SetStorageClass"),
					StorageClass: pulumi.String("COLDLINE"),
				},
				Condition: &storage.BucketLifecycleRuleConditionArgs{
					Age: pulumi.Int(365),
				},
			},
			// Mirrors AWS AbortIncompleteMultipartUpload@7d.
			&storage.BucketLifecycleRuleArgs{
				Action: &storage.BucketLifecycleRuleActionArgs{
					Type: pulumi.String("AbortIncompleteMultipartUpload"),
				},
				Condition: &storage.BucketLifecycleRuleConditionArgs{
					Age: pulumi.Int(7),
				},
			},
			// Mirrors AWS NoncurrentVersionExpiration@90d.
			&storage.BucketLifecycleRuleArgs{
				Action: &storage.BucketLifecycleRuleActionArgs{
					Type: pulumi.String("Delete"),
				},
				Condition: &storage.BucketLifecycleRuleConditionArgs{
					DaysSinceNoncurrentTime: pulumi.Int(90),
					WithState:               pulumi.String("ARCHIVED"),
				},
			},
		},
	})
}

// ---------------------------------------------------------------------------
// kaizen-{env}-mlflow — MLflow artifact storage
// ---------------------------------------------------------------------------
//
// AWS parity: versioning only, no tiering. MLflow rewrites are infrequent
// and per-experiment; cold-archiving artifacts harms restore latency.

func newMlflowBucket(ctx *pulumi.Context, env string, labels pulumi.StringMap) (*storage.Bucket, error) {
	name := fmt.Sprintf("kaizen-%s-mlflow", env)
	forceDestroy := env == "dev"

	return storage.NewBucket(ctx, "mlflow-bucket", &storage.BucketArgs{
		Name:                     pulumi.String(name),
		Location:                 pulumi.String("US"),
		ForceDestroy:             pulumi.Bool(forceDestroy),
		UniformBucketLevelAccess: pulumi.Bool(true),
		PublicAccessPrevention:   pulumi.String("enforced"),
		Labels: mergeLabels(labels, pulumi.StringMap{
			"component": pulumi.String("mlflow"),
		}),
		Versioning: &storage.BucketVersioningArgs{
			Enabled: pulumi.Bool(true),
		},
	})
}

// ---------------------------------------------------------------------------
// kaizen-{env}-logs — load balancer access log bucket
// ---------------------------------------------------------------------------
//
// AWS parity: no versioning; lifecycle delete @ 90d; abort multipart @ 7d.
// On AWS the bucket also receives an `s3:PutObject` policy granting the
// regional ELB service account write access. On GCP the equivalent is an
// IAM binding granting `roles/storage.objectCreator` to the Cloud Load
// Balancing service account, but that account is not yet known at this
// stage (edge module hasn't provisioned the load balancer). The binding
// will be added by the edge module — same pattern as how the AWS edge
// module already references `storageOut` to build its bucket policy.

func newLogsBucket(ctx *pulumi.Context, env string, labels pulumi.StringMap) (*storage.Bucket, error) {
	name := fmt.Sprintf("kaizen-%s-logs", env)
	forceDestroy := env == "dev"

	return storage.NewBucket(ctx, "logs-bucket", &storage.BucketArgs{
		Name:                     pulumi.String(name),
		Location:                 pulumi.String("US"),
		ForceDestroy:             pulumi.Bool(forceDestroy),
		UniformBucketLevelAccess: pulumi.Bool(true),
		PublicAccessPrevention:   pulumi.String("enforced"),
		Labels: mergeLabels(labels, pulumi.StringMap{
			"component": pulumi.String("lb-logs"),
		}),
		LifecycleRules: storage.BucketLifecycleRuleArray{
			&storage.BucketLifecycleRuleArgs{
				Action: &storage.BucketLifecycleRuleActionArgs{
					Type: pulumi.String("Delete"),
				},
				Condition: &storage.BucketLifecycleRuleConditionArgs{
					Age: pulumi.Int(90),
				},
			},
			&storage.BucketLifecycleRuleArgs{
				Action: &storage.BucketLifecycleRuleActionArgs{
					Type: pulumi.String("AbortIncompleteMultipartUpload"),
				},
				Condition: &storage.BucketLifecycleRuleConditionArgs{
					Age: pulumi.Int(7),
				},
			},
		},
	})
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

// defaultLabels returns the base label set applied to every bucket. Mirrors
// pkg/config.DefaultTags but lower-cased to satisfy GCP label key/value
// constraints (lowercase letters, numbers, hyphens, underscores; ≤ 63 chars).
func defaultLabels(env string) pulumi.StringMap {
	return pulumi.StringMap{
		"project":     pulumi.String("kaizen"),
		"environment": pulumi.String(env),
		"managed-by":  pulumi.String("pulumi"),
	}
}

// mergeLabels combines a base label map with overrides. Extra keys win.
func mergeLabels(base, extra pulumi.StringMap) pulumi.StringMap {
	merged := pulumi.StringMap{}
	for k, v := range base {
		merged[k] = v
	}
	for k, v := range extra {
		merged[k] = v
	}
	return merged
}
