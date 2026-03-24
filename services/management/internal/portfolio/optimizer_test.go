package portfolio_test

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/management/internal/portfolio"
)

func makeExp(id, name, layer, primaryMetric string, start, end, total int32) portfolio.ExperimentInfo {
	return portfolio.ExperimentInfo{
		ExperimentID:      id,
		ExperimentName:    name,
		LayerID:           layer,
		PrimaryMetricID:   primaryMetric,
		LayerTotalBuckets: total,
		StartBucket:       start,
		EndBucket:         end,
	}
}

func TestOptimize_Empty(t *testing.T) {
	result := portfolio.Optimize(nil, nil)
	assert.Empty(t, result.Allocations)
	assert.Empty(t, result.Conflicts)
	assert.Equal(t, int32(0), result.Stats.RunningCount)
}

func TestOptimize_SingleExperiment(t *testing.T) {
	exp := makeExp("exp-1", "Test", "layer-1", "watch_time", 0, 99, 1000)
	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp}, nil)

	require.Len(t, result.Allocations, 1)
	a := result.Allocations[0]
	assert.Equal(t, "exp-1", a.ExperimentID)
	assert.Equal(t, int32(portfolio.DefaultPriority), a.Priority)
	assert.InDelta(t, 0.1, a.CurrentTrafficFraction, 0.001) // 100/1000
	assert.Empty(t, result.Conflicts)
	assert.Equal(t, int32(1), result.Stats.RunningCount)
}

func TestOptimize_PriorityOverride(t *testing.T) {
	exp1 := makeExp("exp-1", "High Prio", "layer-1", "watch_time", 0, 99, 1000)
	exp2 := makeExp("exp-2", "Low Prio", "layer-1", "clicks", 100, 199, 1000)

	overrides := map[string]int32{
		"exp-1": 5, // highest
		"exp-2": 1, // lowest
	}
	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, overrides)

	require.Len(t, result.Allocations, 2)
	// First allocation should be exp-1 (highest priority)
	assert.Equal(t, "exp-1", result.Allocations[0].ExperimentID)
	assert.Equal(t, int32(5), result.Allocations[0].Priority)
	assert.Equal(t, "exp-2", result.Allocations[1].ExperimentID)
	assert.Equal(t, int32(1), result.Allocations[1].Priority)

	// exp-1 gets more variance budget (5/6 ≈ 0.833)
	assert.Greater(t, result.Allocations[0].VarianceBudgetShare, result.Allocations[1].VarianceBudgetShare)
}

func TestOptimize_PriorityClamping(t *testing.T) {
	exp := makeExp("exp-1", "Test", "layer-1", "watch_time", 0, 99, 1000)
	overrides := map[string]int32{"exp-1": 99} // out of bounds
	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp}, overrides)
	assert.Equal(t, int32(portfolio.MaxPriority), result.Allocations[0].Priority)
}

func TestConflictDetection_PrimaryMetricOverlap(t *testing.T) {
	// Two experiments in the same layer with the same primary metric.
	exp1 := makeExp("exp-1", "Exp1", "layer-1", "watch_time", 0, 99, 1000)
	exp2 := makeExp("exp-2", "Exp2", "layer-1", "watch_time", 100, 199, 1000)

	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, nil)

	require.Len(t, result.Conflicts, 2) // primary metric + population overlap
	var found bool
	for _, c := range result.Conflicts {
		if c.Type == portfolio.ConflictTypePrimaryMetric {
			found = true
			assert.Equal(t, "exp-1", c.ExperimentIDA)
			assert.Equal(t, "exp-2", c.ExperimentIDB)
		}
	}
	assert.True(t, found, "expected primary metric conflict")
}

func TestConflictDetection_DifferentLayers_NoConflict(t *testing.T) {
	// Same primary metric but different layers — no conflict.
	exp1 := makeExp("exp-1", "Exp1", "layer-1", "watch_time", 0, 99, 1000)
	exp2 := makeExp("exp-2", "Exp2", "layer-2", "watch_time", 0, 99, 1000)

	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, nil)
	assert.Empty(t, result.Conflicts)
}

func TestConflictDetection_GuardrailMetricOverlap(t *testing.T) {
	exp1 := portfolio.ExperimentInfo{
		ExperimentID: "exp-1", ExperimentName: "Exp1",
		LayerID: "layer-1", PrimaryMetricID: "watch_time",
		GuardrailIDs: []string{"rebuffer_rate", "playback_failures"},
		LayerTotalBuckets: 1000, StartBucket: 0, EndBucket: 99,
	}
	exp2 := portfolio.ExperimentInfo{
		ExperimentID: "exp-2", ExperimentName: "Exp2",
		LayerID: "layer-1", PrimaryMetricID: "clicks",
		GuardrailIDs: []string{"rebuffer_rate"}, // shared guardrail
		LayerTotalBuckets: 1000, StartBucket: 100, EndBucket: 199,
	}

	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, nil)

	var found bool
	for _, c := range result.Conflicts {
		if c.Type == portfolio.ConflictTypeGuardrailMetric {
			found = true
		}
	}
	assert.True(t, found, "expected guardrail metric conflict")
}

func TestConflictDetection_PopulationOverlap(t *testing.T) {
	// Both experiments in the same layer with no targeting rule.
	exp1 := makeExp("exp-1", "Exp1", "layer-1", "watch_time", 0, 99, 1000)
	exp2 := makeExp("exp-2", "Exp2", "layer-1", "clicks", 100, 199, 1000)
	// No TargetingRuleID set (empty string) → population overlap.

	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, nil)

	var found bool
	for _, c := range result.Conflicts {
		if c.Type == portfolio.ConflictTypePopulationOverlap {
			found = true
		}
	}
	assert.True(t, found, "expected population overlap conflict")
}

func TestConflictDetection_TargetingRuleSeparation_NoPopulationConflict(t *testing.T) {
	// Both experiments have a targeting rule → populations are separate.
	exp1 := portfolio.ExperimentInfo{
		ExperimentID: "exp-1", ExperimentName: "Exp1",
		LayerID: "layer-1", PrimaryMetricID: "watch_time",
		TargetingRuleID:   "rule-mobile",
		LayerTotalBuckets: 1000, StartBucket: 0, EndBucket: 99,
	}
	exp2 := portfolio.ExperimentInfo{
		ExperimentID: "exp-2", ExperimentName: "Exp2",
		LayerID: "layer-1", PrimaryMetricID: "clicks",
		TargetingRuleID:   "rule-desktop",
		LayerTotalBuckets: 1000, StartBucket: 100, EndBucket: 199,
	}

	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, nil)

	for _, c := range result.Conflicts {
		assert.NotEqual(t, portfolio.ConflictTypePopulationOverlap, c.Type,
			"experiments with targeting rules should not have population overlap conflict")
	}
}

func TestVarianceBudgetShares_SumToOne(t *testing.T) {
	experiments := []portfolio.ExperimentInfo{
		makeExp("exp-1", "Exp1", "layer-1", "m1", 0, 99, 1000),
		makeExp("exp-2", "Exp2", "layer-1", "m2", 100, 199, 1000),
		makeExp("exp-3", "Exp3", "layer-1", "m3", 200, 299, 1000),
	}
	result := portfolio.Optimize(experiments, nil)

	total := 0.0
	for _, a := range result.Allocations {
		total += a.VarianceBudgetShare
	}
	assert.InDelta(t, 1.0, total, 1e-9)
}

func TestPortfolioStats_FalseDiscoveryEstimate(t *testing.T) {
	experiments := make([]portfolio.ExperimentInfo, 10)
	for i := range experiments {
		experiments[i] = makeExp(
			"exp-"+string(rune('a'+i)), "Exp",
			"layer-1", "metric",
			int32(i*100), int32(i*100+99), 10000,
		)
	}
	result := portfolio.Optimize(experiments, nil)
	// Expected false discoveries = N × 0.05 = 10 × 0.05 = 0.5
	assert.InDelta(t, 0.5, result.Stats.ExpectedFalseDiscoveries, 1e-9)
}

func TestPortfolioStats_TrafficUtilization(t *testing.T) {
	// 2 experiments, each taking 10% of a 1000-bucket layer → 20% total
	exp1 := makeExp("exp-1", "Exp1", "layer-1", "m1", 0, 99, 1000)
	exp2 := makeExp("exp-2", "Exp2", "layer-1", "m2", 100, 199, 1000)
	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, nil)
	assert.InDelta(t, 0.2, result.Stats.TrafficUtilization, 0.001)
}

func TestUnderpoweredDetection(t *testing.T) {
	// exp-1 has a much higher priority (5) but only 1% traffic.
	// exp-2 has lower priority (1) but 40% traffic.
	// exp-1 should be flagged as underpowered since its recommended share >> current.
	exp1 := portfolio.ExperimentInfo{
		ExperimentID: "exp-1", ExperimentName: "High Priority",
		LayerID: "layer-1", PrimaryMetricID: "watch_time",
		LayerTotalBuckets: 1000, StartBucket: 0, EndBucket: 9, // 1%
	}
	exp2 := portfolio.ExperimentInfo{
		ExperimentID: "exp-2", ExperimentName: "Low Priority",
		LayerID: "layer-1", PrimaryMetricID: "clicks",
		LayerTotalBuckets: 1000, StartBucket: 10, EndBucket: 409, // 40%
	}
	overrides := map[string]int32{"exp-1": 5, "exp-2": 1}
	result := portfolio.Optimize([]portfolio.ExperimentInfo{exp1, exp2}, overrides)

	var exp1Alloc, exp2Alloc portfolio.Allocation
	for _, a := range result.Allocations {
		if a.ExperimentID == "exp-1" {
			exp1Alloc = a
		} else {
			exp2Alloc = a
		}
	}
	assert.True(t, exp1Alloc.Underpowered, "exp-1 should be underpowered")
	assert.False(t, exp2Alloc.Underpowered, "exp-2 should not be underpowered")
}
