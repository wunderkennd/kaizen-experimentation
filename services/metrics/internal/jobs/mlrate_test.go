package jobs

import (
	"context"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

func setupMLRATEJob(t *testing.T) (*MLRATEJob, *spark.MockExecutor, *querylog.MemWriter) {
	t.Helper()
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(1000)
	qlWriter := querylog.NewMemWriter()
	job := NewMLRATEJob(renderer, executor, qlWriter)
	return job, executor, qlWriter
}

func mlrateTestExperiment() *config.ExperimentConfig {
	return &config.ExperimentConfig{
		ExperimentID: "e0000000-0000-0000-0000-000000000008",
		Name:         "mlrate_crossfit_test",
		Type:         "AB",
		State:        "RUNNING",
		StartedAt:    "2024-02-01",
		MLRATEEnabled: true,
		MLRATEFolds:   3,
		Variants: []config.VariantConfig{
			{VariantID: "f0000000-0000-0000-0000-000000000015", Name: "control", TrafficFraction: 0.5, IsControl: true},
			{VariantID: "f0000000-0000-0000-0000-000000000016", Name: "treatment", TrafficFraction: 0.5, IsControl: false},
		},
	}
}

func mlrateTestMetric() *config.MetricConfig {
	return &config.MetricConfig{
		MetricID:                "watch_time_minutes_mlrate",
		Name:                    "Watch Time (MLRATE)",
		Type:                    "MEAN",
		SourceEventType:         "heartbeat",
		MLRATEFeatureEventTypes: []string{"heartbeat", "stream_start"},
		MLRATEModelURI:          "models:/mlrate-watch-time",
		MLRATELookbackDays:      14,
	}
}

func TestMLRATEJob_Run(t *testing.T) {
	job, executor, qlWriter := setupMLRATEJob(t)
	ctx := context.Background()
	exp := mlrateTestExperiment()
	metric := mlrateTestMetric()

	result, err := job.Run(ctx, exp, metric, "2024-02-15")
	require.NoError(t, err)

	assert.Equal(t, exp.ExperimentID, result.ExperimentID)
	assert.Equal(t, metric.MetricID, result.MetricID)
	assert.Equal(t, 3, result.Folds)
	assert.False(t, result.CompletedAt.IsZero())

	// 1 feature prep + 3 fold predictions = 4 calls
	calls := executor.GetCalls()
	assert.Len(t, calls, 4)

	// First call: feature preparation → delta.mlrate_features
	assert.Equal(t, "delta.mlrate_features", calls[0].TargetTable)
	assert.Contains(t, calls[0].SQL, "delta.exposures")
	assert.Contains(t, calls[0].SQL, "delta.metric_events")
	assert.Contains(t, calls[0].SQL, "'heartbeat', 'stream_start'")
	assert.Contains(t, calls[0].SQL, "fold_id")
	assert.Contains(t, calls[0].SQL, "% 3 + 1")

	// Next 3 calls: fold predictions → delta.metric_summaries
	for i := 1; i <= 3; i++ {
		assert.Equal(t, "delta.metric_summaries", calls[i].TargetTable)
		assert.Contains(t, calls[i].SQL, "delta.mlrate_features")
		assert.Contains(t, calls[i].SQL, "ai_predict")
		assert.Contains(t, calls[i].SQL, "mlrate_covariate")
		assert.Contains(t, calls[i].SQL, "models:/mlrate-watch-time/fold_"+string(rune('0'+i)))
	}

	// Verify query log: 1 mlrate_features + 3 mlrate_crossfit = 4 entries
	entries := qlWriter.AllEntries()
	assert.Len(t, entries, 4)

	featCount := 0
	crossfitCount := 0
	for _, e := range entries {
		assert.Equal(t, exp.ExperimentID, e.ExperimentID)
		assert.Equal(t, metric.MetricID, e.MetricID)
		switch e.JobType {
		case "mlrate_features":
			featCount++
		case "mlrate_crossfit":
			crossfitCount++
		}
	}
	assert.Equal(t, 1, featCount)
	assert.Equal(t, 3, crossfitCount)
}

func TestMLRATEJob_Run_DefaultFolds(t *testing.T) {
	job, executor, _ := setupMLRATEJob(t)
	ctx := context.Background()
	exp := mlrateTestExperiment()
	exp.MLRATEFolds = 0 // should default to 5
	metric := mlrateTestMetric()

	result, err := job.Run(ctx, exp, metric, "2024-02-15")
	require.NoError(t, err)

	assert.Equal(t, 5, result.Folds)
	// 1 feature prep + 5 fold predictions = 6 calls
	calls := executor.GetCalls()
	assert.Len(t, calls, 6)
}

func TestMLRATEJob_Run_DefaultLookbackDays(t *testing.T) {
	job, executor, _ := setupMLRATEJob(t)
	ctx := context.Background()
	exp := mlrateTestExperiment()
	metric := mlrateTestMetric()
	metric.MLRATELookbackDays = 0 // should default to 14

	result, err := job.Run(ctx, exp, metric, "2024-02-15")
	require.NoError(t, err)
	assert.NotNil(t, result)

	// Feature prep query should use default lookback of 14
	calls := executor.GetCalls()
	assert.Contains(t, calls[0].SQL, "14")
}

func TestMLRATEJob_Run_MissingFeatureEventTypes(t *testing.T) {
	job, _, _ := setupMLRATEJob(t)
	ctx := context.Background()
	exp := mlrateTestExperiment()
	metric := mlrateTestMetric()
	metric.MLRATEFeatureEventTypes = nil

	_, err := job.Run(ctx, exp, metric, "2024-02-15")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "no mlrate_feature_event_types")
}

func TestMLRATEJob_Run_MissingModelURI(t *testing.T) {
	job, _, _ := setupMLRATEJob(t)
	ctx := context.Background()
	exp := mlrateTestExperiment()
	metric := mlrateTestMetric()
	metric.MLRATEModelURI = ""

	_, err := job.Run(ctx, exp, metric, "2024-02-15")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "no mlrate_model_uri")
}

func TestMLRATEJob_Run_MissingStartDate(t *testing.T) {
	job, _, _ := setupMLRATEJob(t)
	ctx := context.Background()
	exp := mlrateTestExperiment()
	exp.StartedAt = ""
	metric := mlrateTestMetric()

	_, err := job.Run(ctx, exp, metric, "2024-02-15")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "no started_at date")
}

func TestMLRATEJob_Run_FoldPredictionsSQLContent(t *testing.T) {
	job, executor, _ := setupMLRATEJob(t)
	ctx := context.Background()
	exp := mlrateTestExperiment()
	metric := mlrateTestMetric()

	_, err := job.Run(ctx, exp, metric, "2024-02-15")
	require.NoError(t, err)

	calls := executor.GetCalls()
	// Verify each fold prediction targets the correct fold model
	for i := 1; i <= 3; i++ {
		predSQL := calls[i].SQL
		expectedFoldRef := "fold_" + string(rune('0'+i))
		assert.True(t, strings.Contains(predSQL, expectedFoldRef),
			"Fold %d prediction should reference fold_%d model", i, i)
		assert.Contains(t, predSQL, "NAMED_STRUCT")
		assert.Contains(t, predSQL, "'heartbeat'")
		assert.Contains(t, predSQL, "'stream_start'")
	}
}
