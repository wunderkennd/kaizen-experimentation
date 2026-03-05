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

func setupTestJob(t *testing.T) (*StandardJob, *spark.MockExecutor, *querylog.MemWriter) {
	t.Helper()

	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()

	job := NewStandardJob(cfgStore, renderer, executor, qlWriter)
	return job, executor, qlWriter
}

func TestStandardJob_Run(t *testing.T) {
	job, executor, qlWriter := setupTestJob(t)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", result.ExperimentID)
	// homepage_recs_v2 has: ctr_recommendation (PROPORTION), watch_time_minutes (MEAN),
	// stream_start_rate (PROPORTION), rebuffer_rate (RATIO)
	assert.Equal(t, 4, result.MetricsComputed)
	assert.False(t, result.CompletedAt.IsZero())

	// Verify SQL executor was called for each metric.
	// 3 standard metrics + 1 RATIO metric value + 1 RATIO delta method = 5 calls
	calls := executor.GetCalls()
	assert.Len(t, calls, 5)

	// Verify query log has entries for all: 3 standard + 1 ratio + 1 delta method = 5
	entries := qlWriter.AllEntries()
	assert.Len(t, entries, 5)

	dailyMetricCount := 0
	deltaMethodCount := 0
	for _, entry := range entries {
		assert.Equal(t, "e0000000-0000-0000-0000-000000000001", entry.ExperimentID)
		assert.NotEmpty(t, entry.SQLText)
		switch entry.JobType {
		case "daily_metric":
			dailyMetricCount++
		case "delta_method":
			deltaMethodCount++
		}
	}
	assert.Equal(t, 4, dailyMetricCount)
	assert.Equal(t, 1, deltaMethodCount)
}

func TestStandardJob_Run_CorrectSQLTypes(t *testing.T) {
	job, executor, _ := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	calls := executor.GetCalls()
	// 3 standard + 1 ratio value + 1 ratio delta method = 5
	require.Len(t, calls, 5)

	// ctr_recommendation is PROPORTION
	assert.True(t, strings.Contains(calls[0].SQL, "CASE WHEN COUNT"),
		"PROPORTION metric should use CASE WHEN COUNT")

	// watch_time_minutes is MEAN
	assert.True(t, strings.Contains(calls[1].SQL, "AVG(metric_data.value)"),
		"MEAN metric should use AVG")

	// stream_start_rate is PROPORTION
	assert.True(t, strings.Contains(calls[2].SQL, "CASE WHEN COUNT"),
		"PROPORTION metric should use CASE WHEN COUNT")

	// rebuffer_rate is RATIO: per-user ratio value
	assert.True(t, strings.Contains(calls[3].SQL, "numerator_sum / per_user.denominator_sum"),
		"RATIO metric should compute numerator/denominator ratio")
	assert.Equal(t, "delta.metric_summaries", calls[3].TargetTable)

	// rebuffer_rate delta method: variance components
	assert.True(t, strings.Contains(calls[4].SQL, "VAR_SAMP"),
		"Delta method query should have VAR_SAMP")
	assert.True(t, strings.Contains(calls[4].SQL, "COVAR_SAMP"),
		"Delta method query should have COVAR_SAMP")
	assert.Equal(t, "delta.daily_treatment_effects", calls[4].TargetTable)
}

func TestStandardJob_Run_NotFound(t *testing.T) {
	job, _, _ := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "nonexistent")
	assert.Error(t, err)
}

func TestStandardJob_Run_AllExperimentsWithExposureJoin(t *testing.T) {
	job, executor, _ := setupTestJob(t)
	ctx := context.Background()

	// Run for search_ranking_interleave
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)

	// search_success_rate (PROPORTION) + ctr_recommendation (PROPORTION)
	assert.Equal(t, 2, result.MetricsComputed)

	calls := executor.GetCalls()
	for _, call := range calls {
		assert.Contains(t, call.SQL, "WITH exposed_users AS")
		assert.Contains(t, call.SQL, "delta.exposures")
	}
}
