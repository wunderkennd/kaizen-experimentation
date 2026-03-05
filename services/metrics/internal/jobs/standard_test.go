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

	// Verify SQL executor was called for each metric + CUPED + delta method:
	// 4 metric value queries + 1 RATIO delta method + 2 CUPED covariates = 7 calls
	// (ctr_recommendation has CUPED, watch_time_minutes has CUPED)
	calls := executor.GetCalls()
	assert.Len(t, calls, 7)

	// Verify query log: 4 daily_metric + 1 delta_method + 2 cuped_covariate = 7
	entries := qlWriter.AllEntries()
	assert.Len(t, entries, 7)

	dailyMetricCount := 0
	deltaMethodCount := 0
	cupedCovariateCount := 0
	for _, entry := range entries {
		assert.Equal(t, "e0000000-0000-0000-0000-000000000001", entry.ExperimentID)
		assert.NotEmpty(t, entry.SQLText)
		switch entry.JobType {
		case "daily_metric":
			dailyMetricCount++
		case "delta_method":
			deltaMethodCount++
		case "cuped_covariate":
			cupedCovariateCount++
		}
	}
	assert.Equal(t, 4, dailyMetricCount)
	assert.Equal(t, 1, deltaMethodCount)
	assert.Equal(t, 2, cupedCovariateCount)
}

func TestStandardJob_Run_CorrectSQLTypes(t *testing.T) {
	job, executor, _ := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	calls := executor.GetCalls()
	// 4 metric values + 1 delta method + 2 CUPED covariates = 7
	require.Len(t, calls, 7)

	// ctr_recommendation is PROPORTION
	assert.True(t, strings.Contains(calls[0].SQL, "CASE WHEN COUNT"),
		"PROPORTION metric should use CASE WHEN COUNT")

	// ctr_recommendation CUPED covariate
	assert.True(t, strings.Contains(calls[1].SQL, "cuped_covariate"),
		"CUPED covariate query should contain cuped_covariate")
	assert.True(t, strings.Contains(calls[1].SQL, "pre_experiment_data"),
		"CUPED covariate query should contain pre_experiment_data")
	assert.Equal(t, "delta.metric_summaries", calls[1].TargetTable)

	// watch_time_minutes is MEAN
	assert.True(t, strings.Contains(calls[2].SQL, "AVG(metric_data.value)"),
		"MEAN metric should use AVG")

	// watch_time_minutes CUPED covariate
	assert.True(t, strings.Contains(calls[3].SQL, "cuped_covariate"),
		"CUPED covariate query should contain cuped_covariate")
	assert.Equal(t, "delta.metric_summaries", calls[3].TargetTable)

	// stream_start_rate is PROPORTION (no CUPED)
	assert.True(t, strings.Contains(calls[4].SQL, "CASE WHEN COUNT"),
		"PROPORTION metric should use CASE WHEN COUNT")

	// rebuffer_rate is RATIO: per-user ratio value
	assert.True(t, strings.Contains(calls[5].SQL, "numerator_sum / per_user.denominator_sum"),
		"RATIO metric should compute numerator/denominator ratio")
	assert.Equal(t, "delta.metric_summaries", calls[5].TargetTable)

	// rebuffer_rate delta method: variance components
	assert.True(t, strings.Contains(calls[6].SQL, "VAR_SAMP"),
		"Delta method query should have VAR_SAMP")
	assert.True(t, strings.Contains(calls[6].SQL, "COVAR_SAMP"),
		"Delta method query should have COVAR_SAMP")
	assert.Equal(t, "delta.daily_treatment_effects", calls[6].TargetTable)
}

func TestStandardJob_Run_CupedPreExperimentWindow(t *testing.T) {
	job, executor, _ := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	calls := executor.GetCalls()

	// Find CUPED covariate queries (they contain "pre_experiment_data")
	var cupedCalls []spark.MockCall
	for _, c := range calls {
		if strings.Contains(c.SQL, "pre_experiment_data") {
			cupedCalls = append(cupedCalls, c)
		}
	}
	require.Len(t, cupedCalls, 2)

	for _, c := range cupedCalls {
		// Must use experiment start date for pre-period boundary
		assert.Contains(t, c.SQL, "2024-01-08",
			"CUPED query must reference experiment start date")
		// Must use DATE_SUB for lookback window
		assert.Contains(t, c.SQL, "DATE_SUB",
			"CUPED query must use DATE_SUB for lookback window")
		// Must filter to before experiment start
		assert.Contains(t, c.SQL, "event_date <",
			"CUPED query must exclude post-experiment data")
		// Target is metric_summaries
		assert.Equal(t, "delta.metric_summaries", c.TargetTable)
	}
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

	// search_success_rate (PROPORTION) + ctr_recommendation (PROPORTION + CUPED) = 2 metrics
	assert.Equal(t, 2, result.MetricsComputed)

	calls := executor.GetCalls()
	// search_success_rate: 1 metric query
	// ctr_recommendation: 1 metric query + 1 CUPED = 2
	// Total: 3
	assert.Len(t, calls, 3)

	// All metric value queries should have exposure join
	for _, call := range calls {
		assert.Contains(t, call.SQL, "delta.exposures")
	}
}
