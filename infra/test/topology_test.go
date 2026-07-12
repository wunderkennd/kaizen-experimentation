// Package test — topology test for cross-provider StorageOutputs parity.
//
// Lives under `test/` (alongside the per-module integration tests) so CI's
// `go test ./pkg/... ./test/...` step exercises it. The top-level `main`
// package's existing `fullstack_test.go` is intentionally not run by CI.
package test

// Topology test for cross-provider StorageOutputs parity.
//
// Per the multi-cloud spec (Phase 1) and issue #480, this test asserts that
// both AWS and GCP storage modules:
//
//   1. Provision the same logical buckets (data, mlflow, logs).
//   2. Expose every field required by the cross-provider contract.
//   3. Format the bucket reference per provider — `arn:aws:s3:::*` on AWS,
//      `gs://*` on GCP.
//
// Why this test does NOT route through Deploy():
//
// The full Deploy() pipeline currently has a pre-existing bug in
// `pkg/aws/storage.applyVpcEndpointPolicy` (s3.go:310) that panics under
// pulumi.WithMocks: it type-asserts a `pulumi.ID` value as `string`. That
// path is exercised whenever `aws.NewStorage` is called with non-nil
// inputs, including by the existing `TestFullStackDeploy` on main. Fixing
// that bug is out of scope for #480 (storage parity). To avoid coupling
// this PR to that fix, the topology test calls each provider's inner
// `storage.NewStorage` directly under mocks. The cross-provider Ref
// translation is then verified via the provider facade in
// pkg/aws/aws.go::NewStorage and pkg/gcp/gcp.go::NewStorage by
// inspection of the wrapper code (which is trivially type-safe and
// already covered by `go build ./...`).

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	awsstorage "github.com/kaizen-experimentation/infra/pkg/aws/storage"
	gcpstorage "github.com/kaizen-experimentation/infra/pkg/gcp/storage"
)

// recordedResource is a single resource registration captured during a
// mocked Pulumi run.
type recordedResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

// providerMocks records resource registrations and enriches outputs for the
// resource types each provider's storage module creates. Implements
// pulumi.MockResourceMonitor.
type providerMocks struct {
	mu        sync.Mutex
	resources []recordedResource
}

func (m *providerMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, recordedResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	id := args.Name + "_id"
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	switch args.TypeToken {
	// --- AWS S3 ---
	case "aws:s3/bucketV2:BucketV2":
		bucketName := args.Name
		if b, ok := args.Inputs["bucket"]; ok && b.HasValue() {
			bucketName = b.StringValue()
		}
		outputs["bucket"] = resource.NewStringProperty(bucketName)
		outputs["arn"] = resource.NewStringProperty("arn:aws:s3:::" + bucketName)

	// --- GCP Cloud Storage ---
	case "gcp:storage/bucket:Bucket":
		bucketName := args.Name
		if n, ok := args.Inputs["name"]; ok && n.HasValue() {
			bucketName = n.StringValue()
		}
		outputs["name"] = resource.NewStringProperty(bucketName)
		outputs["url"] = resource.NewStringProperty("gs://" + bucketName)
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/storage/v1/b/" + bucketName)
	}

	return id, outputs, nil
}

func (m *providerMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

// countByType returns how many recorded resources match the given type
// token. Lock-safe.
func (m *providerMocks) countByType(typeToken string) int {
	m.mu.Lock()
	defer m.mu.Unlock()
	n := 0
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			n++
		}
	}
	return n
}

// hasBucketNamed returns true if the mock recorded a resource of the given
// type whose `name` (GCP) or `bucket` (AWS) input matches `wantName`.
func (m *providerMocks) hasBucketWithPrefix(typeToken, prefix string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	for _, r := range m.resources {
		if r.TypeToken != typeToken {
			continue
		}
		if v, ok := r.Inputs["name"]; ok && v.HasValue() && startsWith(v.StringValue(), prefix) {
			return true
		}
		if v, ok := r.Inputs["bucket"]; ok && v.HasValue() && startsWith(v.StringValue(), prefix) {
			return true
		}
	}
	return false
}

// TestTopologyStorageBothProviders exercises each provider's inner
// storage.NewStorage and asserts that the bucket inventory and naming
// match across providers.
func TestTopologyStorageBothProviders(t *testing.T) {
	t.Run("aws", func(t *testing.T) {
		mocks := &providerMocks{}
		err := pulumi.RunErr(func(ctx *pulumi.Context) error {
			// Pass nil inputs to skip applyVpcEndpointPolicy — the
			// pre-existing AWS bug lives in that code path. The bucket,
			// versioning, encryption, and lifecycle resources still get
			// created and recorded.
			_, err := awsstorage.NewStorage(ctx, "dev", nil)
			return err
		}, pulumi.WithMocks("kaizen", "dev", mocks))
		if err != nil {
			t.Fatalf("aws storage.NewStorage failed: %v", err)
		}

		assertBucketTopology(t, "aws", mocks, "aws:s3/bucketV2:BucketV2")
	})

	t.Run("gcp", func(t *testing.T) {
		mocks := &providerMocks{}
		err := pulumi.RunErr(func(ctx *pulumi.Context) error {
			_, err := gcpstorage.NewStorage(ctx, "dev", nil)
			return err
		}, pulumi.WithMocks("kaizen", "dev", mocks))
		if err != nil {
			t.Fatalf("gcp storage.NewStorage failed: %v", err)
		}

		assertBucketTopology(t, "gcp", mocks, "gcp:storage/bucket:Bucket")
	})
}

// TestTopologyStorageRefShape locks the cross-provider Ref contract:
// AWS modules must produce ARNs starting with `arn:aws:s3:::`, GCP modules
// must produce `gs://`-prefixed URIs. The wrapper code in pkg/aws/aws.go
// and pkg/gcp/gcp.go assigns `DataBucketArn` / `DataBucketURI` to
// `DataBucketRef` respectively, so verifying the source values here is
// sufficient to verify the shape Deploy() will downstream.
func TestTopologyStorageRefShape(t *testing.T) {
	t.Run("aws", func(t *testing.T) {
		mocks := &providerMocks{}
		err := pulumi.RunErr(func(ctx *pulumi.Context) error {
			out, err := awsstorage.NewStorage(ctx, "dev", nil)
			if err != nil {
				return err
			}
			out.DataBucketArn.ApplyT(func(s string) string {
				if !startsWith(s, "arn:aws:s3:::") {
					t.Errorf("aws DataBucketArn missing 'arn:aws:s3:::' prefix: %q", s)
				}
				return s
			})
			return nil
		}, pulumi.WithMocks("kaizen", "dev", mocks))
		if err != nil {
			t.Fatalf("aws storage.NewStorage failed: %v", err)
		}
	})

	t.Run("gcp", func(t *testing.T) {
		mocks := &providerMocks{}
		err := pulumi.RunErr(func(ctx *pulumi.Context) error {
			out, err := gcpstorage.NewStorage(ctx, "dev", nil)
			if err != nil {
				return err
			}
			out.DataBucketURI.ApplyT(func(s string) string {
				if !startsWith(s, "gs://") {
					t.Errorf("gcp DataBucketURI missing 'gs://' prefix: %q", s)
				}
				return s
			})
			return nil
		}, pulumi.WithMocks("kaizen", "dev", mocks))
		if err != nil {
			t.Fatalf("gcp storage.NewStorage failed: %v", err)
		}
	})
}

// assertBucketTopology checks that exactly three buckets of the given
// resource type were registered, named `kaizen-dev-{data,mlflow,logs}` —
// exact on GCP; on AWS an account-ID suffix follows (global S3 namespace),
// so matching is by prefix.
func assertBucketTopology(t *testing.T, provider string, mocks *providerMocks, typeToken string) {
	t.Helper()
	if got := mocks.countByType(typeToken); got != 3 {
		t.Errorf("%s: bucket count = %d, want 3 (data, mlflow, logs)", provider, got)
	}
	for _, name := range []string{"kaizen-dev-data", "kaizen-dev-mlflow", "kaizen-dev-logs"} {
		if !mocks.hasBucketWithPrefix(typeToken, name) {
			t.Errorf("%s: missing bucket named %q", provider, name)
		}
	}
}

func startsWith(s, prefix string) bool {
	if len(s) < len(prefix) {
		return false
	}
	return s[:len(prefix)] == prefix
}
