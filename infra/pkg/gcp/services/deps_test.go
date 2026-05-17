package services

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// TestStageOutputs_FieldsAccessible is a compile-time + runtime guard that
// StageOutputs exposes every stage output gcp.NewCompute consumes. If a new
// stage is added (e.g. observability), this test must be extended.
func TestStageOutputs_FieldsAccessible(t *testing.T) {
	s := StageOutputs{
		Net:     types.NetworkOutputs{},
		CICD:    types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}},
		DB:      types.DatabaseOutputs{},
		Cache:   types.CacheOutputs{},
		Stream:  types.StreamingOutputs{},
		Secrets: types.SecretsOutputs{},
		Storage: types.StorageOutputs{},
	}
	// Field access is the assertion — if any field is missing or misnamed
	// the package will not compile.
	_ = s.Net
	_ = s.CICD
	_ = s.DB
	_ = s.Cache
	_ = s.Stream
	_ = s.Secrets
	_ = s.Storage
}
