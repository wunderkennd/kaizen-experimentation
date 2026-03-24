package handlers

import (
	"context"

	"connectrpc.com/connect"

	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/portfolio"
)

// GetPortfolioAllocation implements ADR-019 portfolio-level optimization.
// It returns per-experiment traffic allocation recommendations, conflict detection,
// and priority-weighted variance budget shares for all RUNNING experiments.
func (s *ExperimentService) GetPortfolioAllocation(
	ctx context.Context,
	req *connect.Request[mgmtv1.GetPortfolioAllocationRequest],
) (*connect.Response[mgmtv1.GetPortfolioAllocationResponse], error) {

	layerID := req.Msg.GetLayerId()

	// Fetch RUNNING experiments (optionally filtered by layer).
	experiments, _, guardrailRows, err := s.store.ListRunningByLayer(ctx, layerID)
	if err != nil {
		return nil, internalError("list running experiments", err)
	}

	// Collect layer capacities (lazily, one DB call per distinct layer).
	layerCapacity := make(map[string]int32)

	// Build portfolio.ExperimentInfo for each experiment.
	infos := make([]portfolio.ExperimentInfo, 0, len(experiments))
	for i, exp := range experiments {
		// Fetch layer capacity if not already cached.
		if _, ok := layerCapacity[exp.LayerID]; !ok {
			layer, err := s.layers.GetLayerByID(ctx, exp.LayerID)
			if err != nil {
				return nil, internalError("get layer", err)
			}
			layerCapacity[exp.LayerID] = layer.TotalBuckets
		}
		totalBuckets := layerCapacity[exp.LayerID]

		// Fetch active bucket allocation for this experiment.
		alloc, err := s.layers.GetAllocationByExperiment(ctx, nil, exp.ExperimentID)
		if err != nil {
			return nil, internalError("get allocation", err)
		}

		startBucket := int32(0)
		endBucket := int32(-1) // no allocation → 0 buckets
		if alloc != nil {
			startBucket = alloc.StartBucket
			endBucket = alloc.EndBucket
		}

		// Collect guardrail metric IDs.
		guardrailMetricIDs := make([]string, 0, len(guardrailRows[i]))
		for _, g := range guardrailRows[i] {
			guardrailMetricIDs = append(guardrailMetricIDs, g.MetricID)
		}

		targetingRuleID := ""
		if exp.TargetingRuleID != nil {
			targetingRuleID = *exp.TargetingRuleID
		}

		infos = append(infos, portfolio.ExperimentInfo{
			ExperimentID:      exp.ExperimentID,
			ExperimentName:    exp.Name,
			LayerID:           exp.LayerID,
			PrimaryMetricID:   exp.PrimaryMetricID,
			GuardrailIDs:      guardrailMetricIDs,
			TargetingRuleID:   targetingRuleID,
			StartBucket:       startBucket,
			EndBucket:         endBucket,
			LayerTotalBuckets: totalBuckets,
		})
	}

	// Run the optimizer.
	result := portfolio.Optimize(infos, req.Msg.GetPriorityOverrides())

	// Convert to proto response.
	resp := &mgmtv1.GetPortfolioAllocationResponse{
		Stats: &mgmtv1.PortfolioStats{
			RunningCount:             result.Stats.RunningCount,
			TrafficUtilization:       result.Stats.TrafficUtilization,
			ExpectedFalseDiscoveries: result.Stats.ExpectedFalseDiscoveries,
			UnderpoweredCount:        result.Stats.UnderpoweredCount,
			ConflictCount:            result.Stats.ConflictCount,
		},
	}

	for _, a := range result.Allocations {
		resp.Allocations = append(resp.Allocations, &mgmtv1.ExperimentAllocation{
			ExperimentId:               a.ExperimentID,
			ExperimentName:             a.ExperimentName,
			Priority:                   a.Priority,
			CurrentTrafficFraction:     a.CurrentTrafficFraction,
			RecommendedTrafficFraction: a.RecommendedTrafficFraction,
			Underpowered:               a.Underpowered,
			Rationale:                  a.Rationale,
			VarianceBudgetShare:        a.VarianceBudgetShare,
		})
	}

	for _, c := range result.Conflicts {
		resp.Conflicts = append(resp.Conflicts, &mgmtv1.ExperimentConflict{
			ExperimentIdA: c.ExperimentIDA,
			ExperimentIdB: c.ExperimentIDB,
			ConflictType:  mgmtv1.ConflictType(c.Type),
			Rationale:     c.Rationale,
		})
	}

	return connect.NewResponse(resp), nil
}
