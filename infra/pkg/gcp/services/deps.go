// Package services contains one Cloud Run factory per Kaizen per-service
// deploy (M1/M2-Orch/M2-Pipe/M3/M4a/M5/M6/M7 + the preview canary). Each
// factory is invoked from gcp.NewCompute via the registry in registry.go.
// See docs/superpowers/plans/2026-05-16-service-registry-refactor.md for
// the migration rationale (issue #542).
package services

import (
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// StageOutputs bundles every upstream stage output gcp.NewCompute threads
// into the per-service factories. Constructed once per Deploy() call in
// infra/main.go and passed by value (the fields are pulumi.*Output handles —
// cheap to copy, shared backing state).
type StageOutputs struct {
	Net     types.NetworkOutputs
	CICD    types.CICDOutputs
	DB      types.DatabaseOutputs
	Cache   types.CacheOutputs
	Stream  types.StreamingOutputs
	Secrets types.SecretsOutputs
	Storage types.StorageOutputs
}

// CommonInputs is the shared compute.Inputs every Cloud Run factory needs.
// Constructed once in NewCompute (from cfg + StageOutputs.Net) and passed to
// each factory verbatim.
type CommonInputs = compute.Inputs
