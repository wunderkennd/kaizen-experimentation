package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// RegistryEntry binds a service's short key (e.g. "m3") to its factory.
// The factory signature is uniform — services that need extra args (today
// only M1, which reads M4b's endpoint) accept them through closure capture
// in the registry slice below.
type RegistryEntry struct {
	Key     string
	Factory func(ctx *pulumi.Context, cfg *kconfig.Config, inputs *compute.Inputs, stages StageOutputs) (*compute.CloudRunService, error)
}

// Walk invokes every factory in declaration order, returning a map of service
// key → recorded service. Callers (gcp.NewCompute) compose .URL into
// types.ComputeOutputs.ServiceEndpoints and .Service.ID() into ServiceArns.
//
// Order matters only for the Cloud Run dependency graph (Pulumi handles
// dependencies via output edges, not call order) — but stable iteration
// keeps ctx.Export key emission deterministic.
//
// Duplicate keys fail fast: a copy-paste mistake in the registry would
// otherwise silently overwrite the first service's entry in the returned map
// while still creating both Cloud Run resources (since the factory runs
// before the dedup check). The error catches this at the first Walk call.
func Walk(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
	registry []RegistryEntry,
) (map[string]*compute.CloudRunService, error) {
	out := make(map[string]*compute.CloudRunService, len(registry))
	for _, entry := range registry {
		if _, exists := out[entry.Key]; exists {
			return nil, fmt.Errorf("services.Walk: duplicate registry key %q", entry.Key)
		}
		svc, err := entry.Factory(ctx, cfg, inputs, stages)
		if err != nil {
			return nil, fmt.Errorf("services.Walk: factory %q failed: %w", entry.Key, err)
		}
		out[entry.Key] = svc
	}
	return out, nil
}
