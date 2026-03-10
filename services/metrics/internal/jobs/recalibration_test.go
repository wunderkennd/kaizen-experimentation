package jobs

import (
	"context"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

func setupRecalibrationJob(
	t *testing.T,
	inputs surrogate.InputMetrics,
	projections []surrogate.ProjectionRecord,
) (*RecalibrationJob, *testInputProvider, *querylog.MemWriter, *surrogate.MemProjectionWriter, *surrogate.MemCalibrationUpdater) {
	t.Helper()

	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	provider := &testInputProvider{MockInputMetricsProvider: MockInputMetricsProvider{Inputs: inputs}}
	qlWriter := querylog.NewMemWriter()
	projWriter := surrogate.NewMemProjectionWriter()
	calibUpdater := surrogate.NewMemCalibrationUpdater()

	// Pre-load projections into the MemProjectionWriter.
	ctx := context.Background()
	for _, p := range projections {
		require.NoError(t, projWriter.Write(ctx, p))
	}

	job := NewRecalibrationJob(cfgStore, renderer, provider, qlWriter, projWriter, calibUpdater)
	return job, provider, qlWriter, projWriter, calibUpdater
}

func TestRecalibrationJob_Run(t *testing.T) {
	// Setup: past projection predicted effect of -0.1175 for treatment variant.
	projections := []surrogate.ProjectionRecord{
		{
			ExperimentID:    "e0000000-0000-0000-0000-000000000001",
			VariantID:       "f0000000-0000-0000-0000-000000000002",
			ModelID:         "sm-churn-predictor-001",
			ProjectedEffect: -0.1175,
			ComputedAt:      time.Now().Add(-30 * 24 * time.Hour),
		},
	}

	// Actual target metric values (churn_7d) after prediction horizon.
	actuals := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"churn_7d": 0.15},  // control
		"f0000000-0000-0000-0000-000000000002": {"churn_7d": 0.035}, // treatment
	}
	// actual_effect = 0.035 - 0.15 = -0.115

	job, _, _, _, _ := setupRecalibrationJob(t, actuals, projections)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	require.NotNil(t, result)

	assert.Equal(t, "sm-churn-predictor-001", result.ModelID)
	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", result.ExperimentID)
	assert.Equal(t, 0.72, result.PreviousRSquared) // from seed config
	assert.Equal(t, 1, result.DataPoints)
	assert.False(t, result.CompletedAt.IsZero())

	// With only 1 data point, R² can't be computed → keeps previous.
	assert.Equal(t, 0.72, result.NewRSquared)
}

func TestRecalibrationJob_Run_NoSurrogateModel(t *testing.T) {
	job, _, _, _, _ := setupRecalibrationJob(t, nil, nil)
	ctx := context.Background()

	// search_ranking_interleave has no surrogate model.
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)
	assert.Nil(t, result, "should return nil result for experiment without surrogate model")
}

func TestRecalibrationJob_Run_NoProjections(t *testing.T) {
	actuals := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"churn_7d": 0.15},
		"f0000000-0000-0000-0000-000000000002": {"churn_7d": 0.035},
	}

	job, _, _, _, calibUpdater := setupRecalibrationJob(t, actuals, nil)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	require.NotNil(t, result)

	assert.Equal(t, 0.72, result.PreviousRSquared)
	assert.Equal(t, 0.72, result.NewRSquared, "should keep previous R² when no projections")
	assert.Equal(t, 0, result.DataPoints)
	assert.Empty(t, calibUpdater.Updates, "should not update calibration with no projections")
}

func TestRecalibrationJob_Run_RSquaredComputation(t *testing.T) {
	// 3 variants with known projected and actual effects → verifiable R².
	projections := []surrogate.ProjectionRecord{
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-000000000002", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.10},
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-extra-variant1", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.20},
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-extra-variant2", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.05},
	}

	actuals := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001":  {"churn_7d": 0.30}, // control
		"f0000000-0000-0000-0000-000000000002":  {"churn_7d": 0.20}, // actual effect: -0.10
		"f0000000-0000-0000-0000-extra-variant1": {"churn_7d": 0.12}, // actual effect: -0.18
		"f0000000-0000-0000-0000-extra-variant2": {"churn_7d": 0.24}, // actual effect: -0.06
	}
	// actual effects: -0.10, -0.18, -0.06
	// projected:      -0.10, -0.20, -0.05
	// mean(actual) = (-0.10 + -0.18 + -0.06) / 3 = -0.34/3 ≈ -0.11333
	// SS_res = ((-0.10 - -0.10)² + (-0.18 - -0.20)² + (-0.06 - -0.05)²) = 0 + 0.0004 + 0.0001 = 0.0005
	// SS_tot = ((-0.10 - -0.11333)² + (-0.18 - -0.11333)² + (-0.06 - -0.11333)²)
	//        = 0.01333² + 0.06667² + 0.05333²
	//        = 0.0001778 + 0.0044449 + 0.0028444 = 0.0074671
	// R² = 1 - 0.0005/0.0074671 ≈ 0.9330

	job, _, _, _, calibUpdater := setupRecalibrationJob(t, actuals, projections)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	require.NotNil(t, result)

	assert.Equal(t, 3, result.DataPoints)
	assert.InDelta(t, 0.933, result.NewRSquared, 0.005)
	assert.Equal(t, 0.72, result.PreviousRSquared)

	// Calibration should be updated.
	r2, ok := calibUpdater.Updates["sm-churn-predictor-001"]
	assert.True(t, ok, "calibration should be updated")
	assert.InDelta(t, 0.933, r2, 0.005)
}

func TestRecalibrationJob_Run_PerfectPrediction(t *testing.T) {
	projections := []surrogate.ProjectionRecord{
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-000000000002", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.10},
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-extra-variant1", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.20},
	}

	actuals := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001":  {"churn_7d": 0.30}, // control
		"f0000000-0000-0000-0000-000000000002":  {"churn_7d": 0.20}, // effect = -0.10 (matches)
		"f0000000-0000-0000-0000-extra-variant1": {"churn_7d": 0.10}, // effect = -0.20 (matches)
	}

	job, _, _, _, calibUpdater := setupRecalibrationJob(t, actuals, projections)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	require.NotNil(t, result)

	assert.Equal(t, 1.0, result.NewRSquared, "perfect predictions should yield R² = 1.0")

	r2 := calibUpdater.Updates["sm-churn-predictor-001"]
	assert.Equal(t, 1.0, r2)
}

func TestRecalibrationJob_Run_QueryLogEntries(t *testing.T) {
	projections := []surrogate.ProjectionRecord{
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-000000000002", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.10},
	}

	actuals := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"churn_7d": 0.15},
		"f0000000-0000-0000-0000-000000000002": {"churn_7d": 0.05},
	}

	job, _, qlWriter, _, _ := setupRecalibrationJob(t, actuals, projections)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	entries := qlWriter.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "surrogate_recalibration_actual", entries[0].JobType)
	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", entries[0].ExperimentID)
	assert.Equal(t, "churn_7d", entries[0].MetricID)
	assert.NotEmpty(t, entries[0].SQLText)
}

func TestRecalibrationJob_Run_MultipleVariants(t *testing.T) {
	projections := []surrogate.ProjectionRecord{
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-000000000002", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.10},
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-extra-variant1", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.15},
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-extra-variant2", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.20},
	}

	actuals := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001":  {"churn_7d": 0.30}, // control
		"f0000000-0000-0000-0000-000000000002":  {"churn_7d": 0.22}, // effect: -0.08
		"f0000000-0000-0000-0000-extra-variant1": {"churn_7d": 0.18}, // effect: -0.12
		"f0000000-0000-0000-0000-extra-variant2": {"churn_7d": 0.10}, // effect: -0.20
	}

	job, _, _, _, calibUpdater := setupRecalibrationJob(t, actuals, projections)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	require.NotNil(t, result)

	assert.Equal(t, 3, result.DataPoints, "should produce pairs for all 3 treatment variants")
	_, updated := calibUpdater.Updates["sm-churn-predictor-001"]
	assert.True(t, updated, "calibration should be updated with 3 data points")
}

func TestComputeRSquared(t *testing.T) {
	tests := []struct {
		name     string
		pairs    []effectPair
		expected float64
	}{
		{
			name:     "empty",
			pairs:    nil,
			expected: 0,
		},
		{
			name:     "single point",
			pairs:    []effectPair{{projected: 1.0, actual: 1.0}},
			expected: 0,
		},
		{
			name: "perfect prediction",
			pairs: []effectPair{
				{projected: -0.10, actual: -0.10},
				{projected: -0.20, actual: -0.20},
			},
			expected: 1.0,
		},
		{
			name: "all actual same value, predictions differ",
			pairs: []effectPair{
				{projected: -0.10, actual: -0.15},
				{projected: -0.20, actual: -0.15},
			},
			expected: 0, // SS_tot = 0, SS_res > 0
		},
		{
			name: "all actual same value, predictions match",
			pairs: []effectPair{
				{projected: -0.15, actual: -0.15},
				{projected: -0.15, actual: -0.15},
			},
			expected: 1.0, // SS_tot = 0, SS_res = 0
		},
		{
			name: "poor prediction yields negative R²",
			pairs: []effectPair{
				{projected: 1.0, actual: -1.0},
				{projected: -1.0, actual: 1.0},
			},
			// mean(actual) = 0
			// SS_res = ((-1-1)² + (1-(-1))²) = 4 + 4 = 8
			// SS_tot = ((-1-0)² + (1-0)²) = 1 + 1 = 2
			// R² = 1 - 8/2 = -3
			expected: -3.0,
		},
		{
			name: "moderate prediction",
			pairs: []effectPair{
				{projected: -0.10, actual: -0.12},
				{projected: -0.20, actual: -0.18},
				{projected: -0.05, actual: -0.06},
			},
			// Close but not exact → R² between 0 and 1.
			expected: 0.95, // approximate
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := computeRSquared(tt.pairs)
			if tt.name == "moderate prediction" {
				assert.InDelta(t, tt.expected, got, 0.1, "R² should be near expected for moderate prediction")
			} else {
				assert.InDelta(t, tt.expected, got, 1e-10, "R² mismatch for %s", tt.name)
			}
		})
	}
}
