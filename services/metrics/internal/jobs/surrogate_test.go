package jobs

import (
	"context"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

// testInputProvider tracks the SQL received during Fetch, wrapping MockInputMetricsProvider.
type testInputProvider struct {
	MockInputMetricsProvider
	SQLReceived string
}

func (m *testInputProvider) Fetch(ctx context.Context, sql string) (surrogate.InputMetrics, error) {
	m.SQLReceived = sql
	return m.MockInputMetricsProvider.Fetch(ctx, sql)
}

func setupSurrogateJob(t *testing.T, inputs surrogate.InputMetrics) (*SurrogateJob, *testInputProvider, *querylog.MemWriter, *surrogate.MemProjectionWriter) {
	t.Helper()

	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	provider := &testInputProvider{MockInputMetricsProvider: MockInputMetricsProvider{Inputs: inputs}}
	qlWriter := querylog.NewMemWriter()
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()

	job := NewSurrogateJob(cfgStore, renderer, provider, qlWriter, modelLoader, projWriter)
	return job, provider, qlWriter, projWriter
}

func TestSurrogateJob_Run(t *testing.T) {
	inputs := surrogate.InputMetrics{
		// control variant
		"f0000000-0000-0000-0000-000000000001": {
			"watch_time_minutes": 45.0,
			"stream_start_rate":  0.8,
		},
		// treatment variant
		"f0000000-0000-0000-0000-000000000002": {
			"watch_time_minutes": 52.0,
			"stream_start_rate":  0.85,
		},
	}

	job, provider, qlWriter, projWriter := setupSurrogateJob(t, inputs)
	ctx := context.Background()

	// homepage_recs_v2 has surrogate_model_id = "sm-churn-predictor-001"
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", result.ExperimentID)
	assert.Equal(t, 1, result.MetricsComputed, "should project 1 treatment variant")
	assert.False(t, result.CompletedAt.IsZero())

	// Verify SQL was rendered and passed to the input provider.
	assert.Contains(t, provider.SQLReceived, "delta.metric_summaries")
	assert.Contains(t, provider.SQLReceived, "'watch_time_minutes', 'stream_start_rate'")

	// Verify query log: 1 surrogate_input entry.
	entries := qlWriter.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "surrogate_input", entries[0].JobType)
	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", entries[0].ExperimentID)
	assert.Equal(t, "churn_7d", entries[0].MetricID, "should log target metric ID")
	assert.NotEmpty(t, entries[0].SQLText)

	// Verify projection was written.
	records := projWriter.AllRecords()
	require.Len(t, records, 1)
	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", records[0].ExperimentID)
	assert.Equal(t, "f0000000-0000-0000-0000-000000000002", records[0].VariantID)
	assert.Equal(t, "sm-churn-predictor-001", records[0].ModelID)
	assert.Equal(t, 0.72, records[0].CalibrationRSquared)
	assert.False(t, records[0].ComputedAt.IsZero())
}

func TestSurrogateJob_Run_ProjectedEffect(t *testing.T) {
	// Model: y = 0.35 + (-0.015 * watch_time) + (-0.25 * stream_start_rate)
	// Control: 0.35 + (-0.015 * 45.0) + (-0.25 * 0.8) = 0.35 - 0.675 - 0.2 = -0.525
	// Treatment: 0.35 + (-0.015 * 52.0) + (-0.25 * 0.85) = 0.35 - 0.78 - 0.2125 = -0.6425
	// Effect = treatment - control = -0.6425 - (-0.525) = -0.1175
	inputs := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {
			"watch_time_minutes": 45.0,
			"stream_start_rate":  0.8,
		},
		"f0000000-0000-0000-0000-000000000002": {
			"watch_time_minutes": 52.0,
			"stream_start_rate":  0.85,
		},
	}

	job, _, _, projWriter := setupSurrogateJob(t, inputs)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	records := projWriter.AllRecords()
	require.Len(t, records, 1)

	// Verify the projected effect matches the linear model calculation.
	assert.InDelta(t, -0.1175, records[0].ProjectedEffect, 1e-6)

	// CI should bracket the effect: lower < effect < upper
	assert.Less(t, records[0].ProjectionCILower, records[0].ProjectedEffect)
	assert.Greater(t, records[0].ProjectionCIUpper, records[0].ProjectedEffect)
}

func TestSurrogateJob_Run_NoSurrogateModel(t *testing.T) {
	job, _, _, projWriter := setupSurrogateJob(t, nil)
	ctx := context.Background()

	// search_ranking_interleave has no surrogate_model_id
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)

	assert.Equal(t, 0, result.MetricsComputed)
	assert.Len(t, projWriter.AllRecords(), 0, "no projections should be written")
}

func TestSurrogateJob_Run_ExperimentNotFound(t *testing.T) {
	job, _, _, _ := setupSurrogateJob(t, nil)
	ctx := context.Background()

	_, err := job.Run(ctx, "nonexistent")
	require.NoError(t, err, "should return 0 metrics, not error, since no surrogate model is linked")
}

func TestSurrogateJob_Run_InputSQLContainsKeyFields(t *testing.T) {
	inputs := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"watch_time_minutes": 45.0, "stream_start_rate": 0.8},
		"f0000000-0000-0000-0000-000000000002": {"watch_time_minutes": 52.0, "stream_start_rate": 0.85},
	}

	job, provider, _, _ := setupSurrogateJob(t, inputs)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	sql := provider.SQLReceived
	assert.Contains(t, sql, "e0000000-0000-0000-0000-000000000001")
	assert.Contains(t, sql, "AVG(ms.metric_value)")
	assert.Contains(t, sql, "GROUP BY ms.variant_id, ms.metric_id")
	assert.Contains(t, sql, "DATE_SUB")
	assert.Contains(t, sql, "7", "observation window should be 7 days")
}

func TestSurrogateJob_Run_MultipleVariants(t *testing.T) {
	// Add a third variant to test multiple projections.
	inputs := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"watch_time_minutes": 45.0, "stream_start_rate": 0.8},
		"f0000000-0000-0000-0000-000000000002": {"watch_time_minutes": 52.0, "stream_start_rate": 0.85},
		"f0000000-0000-0000-0000-extra-variant": {"watch_time_minutes": 40.0, "stream_start_rate": 0.75},
	}

	job, _, _, projWriter := setupSurrogateJob(t, inputs)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	// Should produce projections for 2 non-control variants.
	assert.Equal(t, 2, result.MetricsComputed)

	records := projWriter.AllRecords()
	assert.Len(t, records, 2)

	variantIDs := map[string]bool{}
	for _, r := range records {
		variantIDs[r.VariantID] = true
		assert.Equal(t, "sm-churn-predictor-001", r.ModelID)
		assert.NotEqual(t, "f0000000-0000-0000-0000-000000000001", r.VariantID,
			"control variant should not have a projection")
	}
	assert.True(t, variantIDs["f0000000-0000-0000-0000-000000000002"])
	assert.True(t, variantIDs["f0000000-0000-0000-0000-extra-variant"])
}
