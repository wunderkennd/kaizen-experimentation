// Package gcp is the GCP-side facade for Deploy(). It mirrors pkg/aws (one
// stage-aggregating function per Deploy() switch arm) and is intentionally
// thin — actual resource creation happens in pkg/gcp/<module>/ sub-packages.
//
// Phase 1 only ships the cicd stage; subsequent phases will fill in network,
// storage, database, cache, secrets, compute, and edge. Until those land,
// Deploy() will hit the unsupportedCloud error in main.go for any stage
// other than cicd when cloudProvider=="gcp".
package gcp

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/gcp/cicd"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// NewCICD provisions Artifact Registry repositories for all Kaizen services.
// Returns the same shared types.CICDOutputs shape as pkg/aws.NewCICD so the
// compute layer (and CI dual-push job) consume an identical map regardless
// of cloud provider.
//
// Required cfg fields:
//   - GCPProjectID — GCP project hosting the registry.
//
// Optional cfg fields (all default to safe zero-values):
//   - GCPARLocation — registry location, defaults to "us" multi-region.
//   - GCPCIPushPrincipal — IAM principal granted writer per repo.
//   - GCPRunPullPrincipals — Cloud Run runtime SAs granted reader per repo.
func NewCICD(ctx *pulumi.Context, cfg *kconfig.Config) (types.CICDOutputs, error) {
	if cfg.GCPProjectID == "" {
		return types.CICDOutputs{}, fmt.Errorf(
			"gcp.NewCICD: cfg.GCPProjectID is required when cloudProvider=gcp " +
				"(set via `pulumi config set kaizen-experimentation:gcpProjectId <ID>`)")
	}

	out, err := cicd.NewArtifactRegistryRepositories(ctx, cicd.Config{
		Environment:    cfg.Environment,
		Project:        cfg.GCPProjectID,
		Location:       cfg.GCPARLocation,
		PushPrincipal:  cfg.GCPCIPushPrincipal,
		PullPrincipals: cfg.GCPRunPullPrincipals,
	})
	if err != nil {
		return types.CICDOutputs{}, err
	}

	// Export a single sentinel URL for smoke tests. The AWS facade exports
	// "ecrAssignmentUrl"; the GCP equivalent uses a clearly distinct key so
	// dashboards and stack-export consumers can detect provider drift.
	if url, ok := out.RepositoryURLs["assignment"]; ok {
		ctx.Export("artifactRegistryAssignmentUrl", url)
	}

	return types.CICDOutputs{
		RepositoryURLs: out.RepositoryURLs,
	}, nil
}
