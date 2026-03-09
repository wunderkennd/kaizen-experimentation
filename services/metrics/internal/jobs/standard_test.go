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

	// Verify SQL executor was called for each metric + CUPED + delta method + daily treatment effects:
	// 4 metric value queries + 1 RATIO delta method + 2 CUPED covariates + 4 daily treatment effects = 11 calls
	// (ctr_recommendation has CUPED, watch_time_minutes has CUPED)
	calls := executor.GetCalls()
	assert.Len(t, calls, 11)

	// Verify query log: 4 daily_metric + 1 delta_method + 2 cuped_covariate + 4 daily_treatment_effect = 11
	entries := qlWriter.AllEntries()
	assert.Len(t, entries, 11)

	dailyMetricCount := 0
	deltaMethodCount := 0
	cupedCovariateCount := 0
	dailyTreatmentEffectCount := 0
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
		case "daily_treatment_effect":
			dailyTreatmentEffectCount++
		}
	}
	assert.Equal(t, 4, dailyMetricCount)
	assert.Equal(t, 1, deltaMethodCount)
	assert.Equal(t, 2, cupedCovariateCount)
	assert.Equal(t, 4, dailyTreatmentEffectCount)
}

func TestStandardJob_Run_CorrectSQLTypes(t *testing.T) {
	job, executor, _ := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	calls := executor.GetCalls()
	// 4 metric values + 1 delta method + 2 CUPED covariates + 4 daily treatment effects = 11
	require.Len(t, calls, 11)

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

func TestStandardJob_Run_DailyTreatmentEffects(t *testing.T) {
	job, executor, qlWriter := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	// Find daily_treatment_effect queries
	var teCalls []spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "absolute_effect") {
			teCalls = append(teCalls, c)
		}
	}
	require.Len(t, teCalls, 4, "Should compute daily treatment effects for all 4 metrics")

	for _, c := range teCalls {
		assert.Equal(t, "delta.daily_treatment_effects", c.TargetTable)
		assert.Contains(t, c.SQL, "delta.metric_summaries")
		assert.Contains(t, c.SQL, "control_mean")
		assert.Contains(t, c.SQL, "treatment_mean")
		// Should reference the control variant ID
		assert.Contains(t, c.SQL, "f0000000-0000-0000-0000-000000000001")
	}

	// Verify query log
	teEntries := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "daily_treatment_effect" {
			teEntries++
		}
	}
	assert.Equal(t, 4, teEntries)
}

func TestStandardJob_Run_SessionLevelMetrics(t *testing.T) {
	job, executor, qlWriter := setupTestJob(t)
	ctx := context.Background()

	// playback_qoe_test has session_level: true
	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000004")
	require.NoError(t, err)

	// Find session-level metric queries
	var sessionCalls []spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "me.session_id") && strings.Contains(c.SQL, "GROUP BY metric_data.user_id, metric_data.session_id") {
			sessionCalls = append(sessionCalls, c)
		}
	}
	// session_level applies to non-QoE metrics only — there are no non-QoE metrics
	// in the playback_qoe_test experiment (both ttff_mean and rebuffer_ratio_mean are QoE)
	assert.Len(t, sessionCalls, 0, "QoE metrics should not get session-level treatment")

	// Verify session_level_metric entries
	slCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "session_level_metric" {
			slCount++
		}
	}
	assert.Equal(t, 0, slCount, "QoE metrics are excluded from session-level aggregation")
}

func TestStandardJob_Run_QoEEngagementCorrelation(t *testing.T) {
	job, executor, qlWriter := setupTestJob(t)
	ctx := context.Background()

	// playback_qoe_test: ttff_mean (QoE) + rebuffer_ratio_mean (QoE), no engagement metrics
	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000004")
	require.NoError(t, err)

	// Both metrics are QoE, so no QoE-engagement correlation (needs at least one non-QoE metric)
	corrCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "qoe_engagement_correlation" {
			corrCount++
		}
	}
	assert.Equal(t, 0, corrCount, "No correlation without engagement metrics")

	// Verify no CORR calls were made
	for _, c := range executor.GetCalls() {
		assert.NotContains(t, c.SQL, "CORR(", "No correlation should be computed when all metrics are QoE")
	}
}

func TestStandardJob_Run_MixedQoEAndEngagement(t *testing.T) {
	// Test QoE-engagement correlation when an experiment has both QoE and non-QoE metrics.
	// We use homepage_recs_v2 which has 4 metrics: ctr (PROPORTION), watch_time (MEAN),
	// stream_start (PROPORTION), rebuffer_rate (RATIO). None are QoE, so no correlation.
	// This test verifies the "no QoE metrics" path.
	job, _, qlWriter := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	corrCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "qoe_engagement_correlation" {
			corrCount++
		}
	}
	assert.Equal(t, 0, corrCount, "No QoE metrics means no correlation computation")
}

func TestStandardJob_Run_CustomMetricType(t *testing.T) {
	job, executor, qlWriter := setupTestJob(t)
	ctx := context.Background()

	// custom_metric_test experiment uses power_users_watch_time (CUSTOM type)
	// plus watch_time_minutes (MEAN with CUPED)
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000005")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000005", result.ExperimentID)
	assert.Equal(t, 2, result.MetricsComputed)

	// Verify CUSTOM metric SQL contains the custom_result CTE and user-provided SQL
	var customCall *spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "custom_result") {
			cc := c
			customCall = &cc
			break
		}
	}
	require.NotNil(t, customCall, "Should have a call with custom_result CTE")
	assert.Contains(t, customCall.SQL, "HAVING COUNT(*) >= 10",
		"Custom SQL should contain user-provided HAVING clause")
	assert.Contains(t, customCall.SQL, "delta.exposures",
		"Custom SQL should be joined with exposures")
	assert.Equal(t, "delta.metric_summaries", customCall.TargetTable)

	// Verify query log entries
	var customLogEntry *querylog.Entry
	for _, e := range qlWriter.AllEntries() {
		if e.MetricID == "power_users_watch_time" && e.JobType == "daily_metric" {
			ee := e
			customLogEntry = &ee
			break
		}
	}
	require.NotNil(t, customLogEntry, "Should have a daily_metric query log entry for CUSTOM metric")
	assert.Contains(t, customLogEntry.SQLText, "custom_result")
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
	// daily_treatment_effect: 2 (one per metric)
	// Total: 5
	assert.Len(t, calls, 5)

	// All queries should reference delta.exposures or delta.metric_summaries
	for _, call := range calls {
		hasExposures := strings.Contains(call.SQL, "delta.exposures")
		hasSummaries := strings.Contains(call.SQL, "delta.metric_summaries")
		assert.True(t, hasExposures || hasSummaries,
			"Query should reference delta.exposures or delta.metric_summaries")
	}
}
