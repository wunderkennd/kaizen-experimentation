// Package portfolio implements ADR-019 portfolio-level experiment optimization.
// It provides variance budget allocation, conflict detection, and priority scoring
// for the set of RUNNING experiments across layers.
package portfolio

import (
	"fmt"
	"sort"
)

// DefaultPriority is assigned to experiments not present in priority_overrides.
const DefaultPriority = 3

// MinPriority and MaxPriority are the allowed bounds for business priority.
const (
	MinPriority = 1
	MaxPriority = 5
)

// ExperimentInfo is the minimal per-experiment data the optimizer needs.
type ExperimentInfo struct {
	ExperimentID    string
	ExperimentName  string
	LayerID         string
	PrimaryMetricID string
	GuardrailIDs    []string // guardrail metric IDs
	TargetingRuleID string   // empty string = no targeting (full population)
	// Bucket allocation.
	StartBucket int32
	EndBucket   int32
	// Layer total buckets — needed to compute fraction.
	LayerTotalBuckets int32
}

// TrafficFraction returns the current fraction [0,1] of layer capacity occupied.
func (e ExperimentInfo) TrafficFraction() float64 {
	if e.LayerTotalBuckets <= 0 {
		return 0
	}
	used := float64(e.EndBucket-e.StartBucket+1) / float64(e.LayerTotalBuckets)
	if used < 0 {
		return 0
	}
	if used > 1 {
		return 1
	}
	return used
}

// ConflictType mirrors the proto enum values without importing proto.
type ConflictType int32

const (
	ConflictTypeUnspecified        ConflictType = 0
	ConflictTypePrimaryMetric      ConflictType = 1
	ConflictTypeGuardrailMetric    ConflictType = 2
	ConflictTypePopulationOverlap  ConflictType = 3
)

// Conflict records a detected conflict between two experiments.
type Conflict struct {
	ExperimentIDA string
	ExperimentIDB string
	Type          ConflictType
	Rationale     string
}

// Allocation is the optimizer's recommendation for a single experiment.
type Allocation struct {
	ExperimentID              string
	ExperimentName            string
	Priority                  int32
	CurrentTrafficFraction    float64
	RecommendedTrafficFraction float64
	Underpowered              bool
	Rationale                 string
	VarianceBudgetShare       float64
}

// Stats summarises portfolio health.
type Stats struct {
	RunningCount             int32
	TrafficUtilization       float64
	ExpectedFalseDiscoveries float64
	UnderpoweredCount        int32
	ConflictCount            int32
}

// Result is the full output of Optimize.
type Result struct {
	Allocations []Allocation
	Conflicts   []Conflict
	Stats       Stats
}

// Optimize computes portfolio-level allocation recommendations, conflict detection,
// and variance budget shares for the supplied experiments.
//
// priorityOverrides maps experiment_id → priority (1–5). Missing IDs get DefaultPriority.
func Optimize(experiments []ExperimentInfo, priorityOverrides map[string]int32) Result {
	if len(experiments) == 0 {
		return Result{}
	}

	// --- Priority resolution ---
	priorities := make(map[string]int32, len(experiments))
	for _, e := range experiments {
		p := int32(DefaultPriority)
		if ov, ok := priorityOverrides[e.ExperimentID]; ok {
			p = clampPriority(ov)
		}
		priorities[e.ExperimentID] = p
	}

	// --- Conflict detection ---
	conflicts := detectConflicts(experiments)

	// --- Variance budget shares (priority-weighted) ---
	totalPriority := int32(0)
	for _, e := range experiments {
		totalPriority += priorities[e.ExperimentID]
	}
	budgetShares := make(map[string]float64, len(experiments))
	for _, e := range experiments {
		if totalPriority > 0 {
			budgetShares[e.ExperimentID] = float64(priorities[e.ExperimentID]) / float64(totalPriority)
		} else {
			budgetShares[e.ExperimentID] = 1.0 / float64(len(experiments))
		}
	}

	// --- Recommended traffic allocation ---
	// Group experiments by layer to compute layer-level recommendations.
	byLayer := groupByLayer(experiments)
	recommendations := make(map[string]float64, len(experiments))
	underpowered := make(map[string]bool, len(experiments))

	for layerID, layerExps := range byLayer {
		_ = layerID
		layerRec, layerUnderpowered := recommendLayer(layerExps, priorities, budgetShares)
		for id, rec := range layerRec {
			recommendations[id] = rec
		}
		for id, up := range layerUnderpowered {
			underpowered[id] = up
		}
	}

	// --- Assemble allocations ---
	allocs := make([]Allocation, 0, len(experiments))
	underpoweredCount := int32(0)
	for _, e := range experiments {
		current := e.TrafficFraction()
		recommended := recommendations[e.ExperimentID]
		isUnderpowered := underpowered[e.ExperimentID]
		if isUnderpowered {
			underpoweredCount++
		}
		allocs = append(allocs, Allocation{
			ExperimentID:               e.ExperimentID,
			ExperimentName:             e.ExperimentName,
			Priority:                   priorities[e.ExperimentID],
			CurrentTrafficFraction:     current,
			RecommendedTrafficFraction: recommended,
			Underpowered:               isUnderpowered,
			Rationale:                  buildRationale(e, current, recommended, isUnderpowered, priorities[e.ExperimentID]),
			VarianceBudgetShare:        budgetShares[e.ExperimentID],
		})
	}

	// Sort by priority desc, then experiment_id for determinism.
	sort.Slice(allocs, func(i, j int) bool {
		if allocs[i].Priority != allocs[j].Priority {
			return allocs[i].Priority > allocs[j].Priority
		}
		return allocs[i].ExperimentID < allocs[j].ExperimentID
	})

	// --- Portfolio stats ---
	utilization := computeTrafficUtilization(experiments)
	stats := Stats{
		RunningCount:             int32(len(experiments)),
		TrafficUtilization:       utilization,
		ExpectedFalseDiscoveries: float64(len(experiments)) * 0.05,
		UnderpoweredCount:        underpoweredCount,
		ConflictCount:            int32(len(conflicts)),
	}

	return Result{
		Allocations: allocs,
		Conflicts:   conflicts,
		Stats:       stats,
	}
}

// detectConflicts inspects pairs of experiments for shared metrics or population overlap.
func detectConflicts(experiments []ExperimentInfo) []Conflict {
	var conflicts []Conflict
	for i := 0; i < len(experiments); i++ {
		for j := i + 1; j < len(experiments); j++ {
			a, b := experiments[i], experiments[j]

			// Primary metric overlap within the same layer.
			if a.LayerID == b.LayerID && a.PrimaryMetricID != "" && a.PrimaryMetricID == b.PrimaryMetricID {
				conflicts = append(conflicts, Conflict{
					ExperimentIDA: a.ExperimentID,
					ExperimentIDB: b.ExperimentID,
					Type:          ConflictTypePrimaryMetric,
					Rationale: fmt.Sprintf(
						"Both experiments use primary metric %q in the same layer — "+
							"concurrent significance tests inflate false discovery rate",
						a.PrimaryMetricID,
					),
				})
			}

			// Guardrail metric overlap within the same layer.
			if a.LayerID == b.LayerID {
				shared := sharedMetrics(a.GuardrailIDs, b.GuardrailIDs)
				for _, m := range shared {
					conflicts = append(conflicts, Conflict{
						ExperimentIDA: a.ExperimentID,
						ExperimentIDB: b.ExperimentID,
						Type:          ConflictTypeGuardrailMetric,
						Rationale: fmt.Sprintf(
							"Both experiments monitor guardrail metric %q — "+
								"correlated stopping rules may cause spurious pauses",
							m,
						),
					})
				}
			}

			// Population overlap: same layer, no targeting separation.
			if a.LayerID == b.LayerID && a.TargetingRuleID == "" && b.TargetingRuleID == "" {
				conflicts = append(conflicts, Conflict{
					ExperimentIDA: a.ExperimentID,
					ExperimentIDB: b.ExperimentID,
					Type:          ConflictTypePopulationOverlap,
					Rationale: "Both experiments target the full user population in the same layer — " +
						"users may see both treatments simultaneously, causing interference",
				})
			}
		}
	}
	return conflicts
}

// recommendLayer computes recommended traffic fractions for experiments in a single layer.
// The recommendation is proportional to priority-weighted variance budget shares,
// subject to the constraint that all fractions sum to ≤ 1.0.
func recommendLayer(
	experiments []ExperimentInfo,
	priorities map[string]int32,
	budgetShares map[string]float64,
) (recommendations map[string]float64, underpowered map[string]bool) {
	recommendations = make(map[string]float64, len(experiments))
	underpowered = make(map[string]bool, len(experiments))

	if len(experiments) == 0 {
		return
	}

	// Total weight for this layer.
	totalWeight := 0.0
	for _, e := range experiments {
		totalWeight += budgetShares[e.ExperimentID]
	}

	// Each experiment gets a recommended fraction proportional to its budget share,
	// capped so the total is ≤ 1.0.
	for _, e := range experiments {
		share := 0.0
		if totalWeight > 0 {
			share = budgetShares[e.ExperimentID] / totalWeight
		}
		// Cap at 0.5: no single experiment should take more than half the layer.
		if share > 0.5 {
			share = 0.5
		}
		recommendations[e.ExperimentID] = share

		// An experiment is underpowered if its recommended share would increase its
		// current allocation by more than 25% (i.e., it's significantly under-trafficked).
		current := e.TrafficFraction()
		if share > 0 && current < share*0.75 {
			underpowered[e.ExperimentID] = true
		}
	}
	return
}

// computeTrafficUtilization returns the fraction of all layer buckets that are in use.
// For simplicity we compute per-experiment bucket fractions and union them (sum, capped at 1).
func computeTrafficUtilization(experiments []ExperimentInfo) float64 {
	// Group by layer and sum bucket fractions per layer.
	byLayer := groupByLayer(experiments)
	if len(byLayer) == 0 {
		return 0
	}
	totalUtil := 0.0
	layerCount := 0
	for _, layerExps := range byLayer {
		layerUtil := 0.0
		for _, e := range layerExps {
			layerUtil += e.TrafficFraction()
		}
		if layerUtil > 1.0 {
			layerUtil = 1.0
		}
		totalUtil += layerUtil
		layerCount++
	}
	avg := totalUtil / float64(layerCount)
	if avg > 1.0 {
		avg = 1.0
	}
	return avg
}

func groupByLayer(experiments []ExperimentInfo) map[string][]ExperimentInfo {
	m := make(map[string][]ExperimentInfo)
	for _, e := range experiments {
		m[e.LayerID] = append(m[e.LayerID], e)
	}
	return m
}

func sharedMetrics(a, b []string) []string {
	set := make(map[string]struct{}, len(a))
	for _, m := range a {
		set[m] = struct{}{}
	}
	var shared []string
	for _, m := range b {
		if _, ok := set[m]; ok {
			shared = append(shared, m)
		}
	}
	return shared
}

func clampPriority(p int32) int32 {
	if p < MinPriority {
		return MinPriority
	}
	if p > MaxPriority {
		return MaxPriority
	}
	return p
}

func buildRationale(e ExperimentInfo, current, recommended float64, underpowered bool, priority int32) string {
	switch {
	case underpowered && recommended > current:
		return fmt.Sprintf(
			"Priority %d experiment is under-trafficked (%.1f%% vs recommended %.1f%%) — "+
				"increase allocation to reach significance faster",
			priority, current*100, recommended*100,
		)
	case recommended < current:
		return fmt.Sprintf(
			"Priority %d experiment has more traffic than its variance budget share warrants "+
				"(%.1f%% vs recommended %.1f%%) — consider reducing to free capacity for higher-priority work",
			priority, current*100, recommended*100,
		)
	default:
		return fmt.Sprintf(
			"Priority %d experiment allocation (%.1f%%) is consistent with its variance budget share (%.1f%%)",
			priority, current*100, recommended*100,
		)
	}
}
