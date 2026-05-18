package jobs

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/status"
)

func setupTestJob(t *testing.T) (*StandardJob, *spark.MockExecutor, *querylog.MemWriter, *status.MockWriter) {
	t.Helper()

	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	statusWriter := status.NewMockWriter()

	job := NewStandardJob(cfgStore, renderer, executor, qlWriter, WithStatusWriter(statusWriter))
	return job, executor, qlWriter, statusWriter
}

func TestStandardJob_Run(t *testing.T) {
	job, executor, qlWriter, _ := setupTestJob(t)
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
	job, executor, _, _ := setupTestJob(t)
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
	job, executor, _, _ := setupTestJob(t)
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
	job, _, _, _ := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "nonexistent")
	assert.Error(t, err)
}

func TestStandardJob_Run_DailyTreatmentEffects(t *testing.T) {
	job, executor, qlWriter, _ := setupTestJob(t)
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
	job, executor, qlWriter, _ := setupTestJob(t)
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
	job, executor, qlWriter, _ := setupTestJob(t)
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
	job, _, qlWriter, _ := setupTestJob(t)
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

func TestStandardJob_Run_PercentileMetricType(t *testing.T) {
	job, executor, qlWriter, _ := setupTestJob(t)
	ctx := context.Background()

	// latency_percentile_test experiment uses latency_p50_ms (PERCENTILE type)
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000006")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000006", result.ExperimentID)
	assert.Equal(t, 1, result.MetricsComputed)

	// Verify PERCENTILE metric SQL uses PERCENTILE_APPROX with correct value
	var pctCall *spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "PERCENTILE_APPROX") {
			cc := c
			pctCall = &cc
			break
		}
	}
	require.NotNil(t, pctCall, "Should have a call with PERCENTILE_APPROX")
	assert.Contains(t, pctCall.SQL, "0.5",
		"PERCENTILE SQL should contain the percentile value 0.5")
	assert.Contains(t, pctCall.SQL, "delta.exposures",
		"PERCENTILE SQL should join with exposures")
	assert.Equal(t, "delta.metric_summaries", pctCall.TargetTable)

	// Verify query log entries
	var pctLogEntry *querylog.Entry
	for _, e := range qlWriter.AllEntries() {
		if e.MetricID == "latency_p50_ms" && e.JobType == "daily_metric" {
			ee := e
			pctLogEntry = &ee
			break
		}
	}
	require.NotNil(t, pctLogEntry, "Should have a daily_metric query log entry for PERCENTILE metric")
	assert.Contains(t, pctLogEntry.SQLText, "PERCENTILE_APPROX")
}

func TestStandardJob_Run_CustomMetricType(t *testing.T) {
	job, executor, qlWriter, _ := setupTestJob(t)
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

func TestStandardJob_Run_MLRATECrossFitting(t *testing.T) {
	job, executor, qlWriter, _ := setupTestJob(t)
	ctx := context.Background()

	// mlrate_crossfit_test (e...0008): mlrate_enabled=true, mlrate_folds=3
	// watch_time_minutes_mlrate: MEAN + MLRATE with 2 features (heartbeat, stream_start)
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000008")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000008", result.ExperimentID)
	assert.Equal(t, 1, result.MetricsComputed)

	calls := executor.GetCalls()
	// 1 daily metric + 1 MLRATE features + 3 MLRATE crossfit + 1 daily treatment effect = 6
	assert.Len(t, calls, 6)

	// First call: daily metric (MEAN) → delta.metric_summaries
	assert.Equal(t, "delta.metric_summaries", calls[0].TargetTable)
	assert.Contains(t, calls[0].SQL, "AVG(metric_data.value)")

	// Second call: MLRATE feature prep → delta.mlrate_features
	assert.Equal(t, "delta.mlrate_features", calls[1].TargetTable)
	assert.Contains(t, calls[1].SQL, "fold_id")
	assert.Contains(t, calls[1].SQL, "'heartbeat', 'stream_start'")

	// Calls 3-5: MLRATE fold predictions → delta.metric_summaries
	for i := 2; i <= 4; i++ {
		assert.Equal(t, "delta.metric_summaries", calls[i].TargetTable)
		assert.Contains(t, calls[i].SQL, "ai_predict")
		assert.Contains(t, calls[i].SQL, "mlrate_covariate")
	}

	// Last call: daily treatment effect → delta.daily_treatment_effects
	assert.Equal(t, "delta.daily_treatment_effects", calls[5].TargetTable)

	// Verify query log
	entries := qlWriter.AllEntries()
	mlrateFeatCount := 0
	mlrateCrossfitCount := 0
	for _, e := range entries {
		switch e.JobType {
		case "mlrate_features":
			mlrateFeatCount++
		case "mlrate_crossfit":
			mlrateCrossfitCount++
		}
	}
	assert.Equal(t, 1, mlrateFeatCount)
	assert.Equal(t, 3, mlrateCrossfitCount)
}

func TestStandardJob_Run_AllExperimentsWithExposureJoin(t *testing.T) {
	job, executor, _, _ := setupTestJob(t)
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

func TestIsLegacyStyle(t *testing.T) {
	t.Run("legacy types return true", func(t *testing.T) {
		legacy := []string{
			"MEAN", "PROPORTION", "COUNT", "RATIO", "PERCENTILE", "CUSTOM",
			"mean", "proportion", "count", "ratio", "percentile", "custom",
			"Custom", "Ratio",
		}
		for _, mt := range legacy {
			assert.True(t, isLegacyStyle(mt), "%q should be legacy", mt)
		}
	})

	t.Run("ADR-026 Phase 1 types return false", func(t *testing.T) {
		nonLegacy := []string{
			"FILTERED_MEAN", "COMPOSITE", "WINDOWED_COUNT",
			"filtered_mean", "composite", "windowed_count",
		}
		for _, mt := range nonLegacy {
			assert.False(t, isLegacyStyle(mt), "%q should not be legacy", mt)
		}
	})

	t.Run("unknown types return false", func(t *testing.T) {
		unknown := []string{"", " ", "UNKNOWN_TYPE", "FOO_BAR"}
		for _, mt := range unknown {
			assert.False(t, isLegacyStyle(mt), "%q should not be legacy", mt)
		}
	})
}

func TestStandardJob_Run_ADR026Phase1_NewTypes(t *testing.T) {
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_adr026_phase1.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(123)
	qlWriter := querylog.NewMemWriter()
	statusWriter := status.NewMockWriter()
	job := NewStandardJob(cfgStore, renderer, executor, qlWriter, WithStatusWriter(statusWriter))

	ctx := context.Background()
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-0000000adr26")
	require.NoError(t, err)
	// ADR-026 #475: composite_engagement is now correctly skipped because its
	// operands (watch_time_minutes, stream_start_rate) live in a different
	// experiment's metric list and are not part of this scheduling pass. Only
	// the 3 metrics whose dependencies resolve will run primary SQL.
	assert.Equal(t, 3, result.MetricsComputed,
		"3 metrics (2 new-type with no operands + 1 legacy) compute; COMPOSITE skipped due to upstream-missing operands")

	calls := executor.GetCalls()
	require.GreaterOrEqual(t, len(calls), 3, "executor should receive >= 3 SQL calls (one per metric, plus any post-processing)")

	// Each metric's primary SQL must contain its type-distinctive identifier.
	// Use the first executor call per metric_id from the query log.
	entries := qlWriter.AllEntries()
	sqlByMetric := make(map[string]string, 3)
	for _, e := range entries {
		if e.JobType == "daily_metric" {
			sqlByMetric[e.MetricID] = e.SQLText
		}
	}

	t.Run("filtered_mean SQL", func(t *testing.T) {
		sql, ok := sqlByMetric["mobile_avg_watch_time"]
		require.True(t, ok, "FILTERED_MEAN metric must have produced SQL")
		assert.Contains(t, sql, "filtered_data",
			"FILTERED_MEAN template must produce a `filtered_data` CTE")
		assert.Contains(t, sql, "platform = 'mobile'",
			"FILTERED_MEAN SQL must inline the configured filter_sql")
		assert.Contains(t, sql, "me.duration_ms",
			"FILTERED_MEAN SQL must reference the configured value_column")
	})

	t.Run("composite skipped (operands out of scheduling pass)", func(t *testing.T) {
		// ADR-026 #475: COMPOSITE references operands that don't belong to this
		// experiment's metric list, so the scheduler marks it
		// SkippedUpstreamFailure rather than rendering against missing inputs.
		_, ran := sqlByMetric["composite_engagement"]
		assert.False(t, ran,
			"COMPOSITE with out-of-pass operands must NOT produce daily_metric SQL under topo-order gating")

		var composite *status.Entry
		for i := range statusWriter.Entries {
			if statusWriter.Entries[i].MetricID == "composite_engagement" {
				composite = &statusWriter.Entries[i]
				break
			}
		}
		require.NotNil(t, composite, "composite_engagement must have a recorded status entry")
		assert.Equal(t, status.SkippedUpstreamFailure, composite.Status,
			"composite skipped because operands aren't part of this scheduling pass")
	})

	t.Run("windowed_count SQL", func(t *testing.T) {
		sql, ok := sqlByMetric["stream_starts_24h"]
		require.True(t, ok, "WINDOWED_COUNT metric must have produced SQL")
		assert.Contains(t, sql, "windowed_events",
			"WINDOWED_COUNT template must produce a `windowed_events` CTE")
		assert.Contains(t, sql, "INTERVAL 24 HOURS",
			"WINDOWED_COUNT SQL must inline the configured window_hours")
		assert.Contains(t, sql, "me.event_type = 'stream_start'",
			"WINDOWED_COUNT SQL must inline the configured event_type")
	})

	// Build a set lookup of post-processing JobType values per metric_id from the query log.
	postProcessJobTypes := map[string]bool{
		"session_level_metric": true,
		"lifecycle_metric":     true,
		"cuped_covariate":      true,
	}
	postProcByMetric := make(map[string][]string) // metric_id -> []JobType
	for _, e := range entries {
		if postProcessJobTypes[e.JobType] {
			postProcByMetric[e.MetricID] = append(postProcByMetric[e.MetricID], e.JobType)
		}
	}

	t.Run("new types skip legacy post-processing", func(t *testing.T) {
		// composite_engagement is excluded here because it is now skipped
		// at the topo-order gate before any post-processing could run.
		newTypeIDs := []string{"mobile_avg_watch_time", "stream_starts_24h"}
		for _, id := range newTypeIDs {
			assert.Empty(t, postProcByMetric[id],
				"%s (new ADR-026 Phase 1 type) should NOT emit any session-level / lifecycle / cuped_covariate SQL", id)
		}
	})

	t.Run("legacy types still run post-processing", func(t *testing.T) {
		got := postProcByMetric["legacy_watch_time"]
		assert.Contains(t, got, "session_level_metric",
			"legacy MEAN metric should produce session_level_metric SQL when session_level is enabled")
		assert.Contains(t, got, "lifecycle_metric",
			"legacy MEAN metric should produce lifecycle_metric SQL when lifecycle_stratification is enabled")
		// Note: legacy_watch_time has no cuped_covariate_metric_id, so no cuped_covariate entry expected.
	})
}

// TestStandardJob_Run_FailFastStillMarksDownstreamComposite is the regression
// guard for the ADR-026 #475 fail-fast follow-up. When a non-COMPOSITE operand
// fails, Run still early-returns the wrapped error (preserving the chaos suite
// contract), but the deferred status flush must now also mark any downstream
// COMPOSITE as SkippedUpstreamFailure so M4a can distinguish "failed-upstream"
// from "never scheduled" in metric_computation_status.
func TestStandardJob_Run_FailFastStillMarksDownstreamComposite(t *testing.T) {
	// Build a minimal inline fixture with a non-COMPOSITE operand `op_a` and a
	// COMPOSITE `comp_b` that depends on it. Both belong to the same experiment
	// so topo-order schedules op_a first, comp_b second.
	dir := t.TempDir()
	fixturePath := filepath.Join(dir, "seed_failfast_composite.json")
	const fixture = `{
		"experiments": [
			{
				"experiment_id": "e0000000-0000-0000-0000-00000000ff01",
				"name": "failfast_composite_smoke",
				"type": "STANDARD",
				"state": "RUNNING",
				"started_at": "2026-05-01",
				"primary_metric_id": "op_a",
				"secondary_metric_ids": ["comp_b"],
				"variants": [
					{"variant_id": "control", "name": "Control", "traffic_fraction": 0.5, "is_control": true},
					{"variant_id": "treatment", "name": "Treatment", "traffic_fraction": 0.5, "is_control": false}
				]
			}
		],
		"metrics": [
			{
				"metric_id": "op_a",
				"name": "Operand A (MEAN)",
				"type": "MEAN",
				"source_event_type": "heartbeat"
			},
			{
				"metric_id": "comp_b",
				"name": "Downstream composite",
				"type": "COMPOSITE",
				"source_event_type": "n/a",
				"operator": "WEIGHTED_SUM",
				"operands": [
					{"metric_id": "op_a", "weight": 1.0}
				]
			}
		]
	}`
	require.NoError(t, os.WriteFile(fixturePath, []byte(fixture), 0o600))

	cfgStore, err := config.LoadFromFile(fixturePath)
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	// Fail on the very first executor call so op_a (first in topo order) fails
	// before comp_b is even reached. This is the canonical fail-fast scenario.
	sentinel := fmt.Errorf("spark cluster unreachable")
	executor := NewFailingExecutor(0, sentinel)
	qlWriter := querylog.NewMemWriter()
	statusWriter := status.NewMockWriter()

	job := NewStandardJob(cfgStore, renderer, executor, qlWriter, WithStatusWriter(statusWriter))

	_, runErr := job.Run(context.Background(), "e0000000-0000-0000-0000-00000000ff01")

	// 1. Fail-fast contract preserved: Run returns the wrapped operand error.
	require.Error(t, runErr)
	assert.Contains(t, runErr.Error(), "jobs: execute metric op_a",
		"error must identify the failing operand")
	assert.ErrorIs(t, runErr, sentinel, "original sentinel error must remain in the wrap chain")

	// 2. Status table now records BOTH the failed operand AND the downstream
	//    COMPOSITE — the latter as SkippedUpstreamFailure so M4a can distinguish
	//    "failed upstream" from "never scheduled".
	snap := statusWriter.Snapshot()
	byMetric := make(map[string]status.Entry, len(snap))
	for _, e := range snap {
		byMetric[e.MetricID] = e
	}

	failed, hasFailed := byMetric["op_a"]
	require.True(t, hasFailed, "op_a must have a status row recording the failure")
	assert.Equal(t, status.Failed, failed.Status, "op_a status must be Failed")

	skipped, hasSkipped := byMetric["comp_b"]
	require.True(t, hasSkipped,
		"comp_b must have a status row even though Run early-returned before visiting it (ADR-026 #475)")
	assert.Equal(t, status.SkippedUpstreamFailure, skipped.Status,
		"comp_b status must be SkippedUpstreamFailure, not omitted")
	assert.Contains(t, skipped.Reason, "op_a",
		"comp_b skip reason should mention the blocking operand")
}

// TestStandardJob_Run_CompositeRunsAfterOperands is the ADR-026 #475 happy-path
// guard for topo-order scheduling: a COMPOSITE metric whose operands are
// independent non-COMPOSITE metrics must have its daily-metric SQL executed
// strictly AFTER both operand queries, because composite.sql.tmpl reads from
// delta.metric_summaries and would return NULL if scheduled before its
// operands wrote their rows. The ordering between unrelated operands is
// intentionally unconstrained (statusMap / topo-order do not promise stable
// ordering for siblings).
func TestStandardJob_Run_CompositeRunsAfterOperands(t *testing.T) {
	// Inline fixture: three metrics on a single experiment.
	//   session_score   -- MEAN over heartbeat events
	//   click_rate      -- PROPORTION over click events (single source_event_type
	//                      matches how PROPORTION is wired in seed_config.json;
	//                      numerator/denominator fields belong to RATIO)
	//   engagement_index -- COMPOSITE WEIGHTED_SUM(session_score*0.6 + click_rate*0.4)
	dir := t.TempDir()
	fixturePath := filepath.Join(dir, "seed_composite_happy_path.json")
	const fixture = `{
		"experiments": [
			{
				"experiment_id": "e0000000-0000-0000-0000-00000000aa01",
				"name": "composite_happy_path",
				"type": "STANDARD",
				"state": "RUNNING",
				"started_at": "2026-05-01",
				"primary_metric_id": "engagement_index",
				"secondary_metric_ids": ["session_score", "click_rate"],
				"variants": [
					{"variant_id": "control", "name": "Control", "traffic_fraction": 0.5, "is_control": true},
					{"variant_id": "treatment", "name": "Treatment", "traffic_fraction": 0.5, "is_control": false}
				]
			}
		],
		"metrics": [
			{
				"metric_id": "session_score",
				"name": "Session score (MEAN)",
				"type": "MEAN",
				"source_event_type": "session_end",
				"value_column": "session_score"
			},
			{
				"metric_id": "click_rate",
				"name": "Click rate (PROPORTION)",
				"type": "PROPORTION",
				"source_event_type": "click"
			},
			{
				"metric_id": "engagement_index",
				"name": "Engagement index (COMPOSITE)",
				"type": "COMPOSITE",
				"source_event_type": "n/a",
				"operator": "WEIGHTED_SUM",
				"operands": [
					{"metric_id": "session_score", "weight": 0.6},
					{"metric_id": "click_rate", "weight": 0.4}
				]
			}
		]
	}`
	require.NoError(t, os.WriteFile(fixturePath, []byte(fixture), 0o600))

	cfgStore, err := config.LoadFromFile(fixturePath)
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	statusWriter := status.NewMockWriter()

	job := NewStandardJob(cfgStore, renderer, executor, qlWriter, WithStatusWriter(statusWriter))

	_, runErr := job.Run(context.Background(), "e0000000-0000-0000-0000-00000000aa01")
	require.NoError(t, runErr, "happy path must not surface any error")

	// Locate the index of each metric's daily-metric SQL in the executor's
	// recorded call list. Every daily-metric template (mean, proportion,
	// composite) emits `'<metric_id>' AS metric_id` in its projection, which
	// uniquely identifies the originating render. Daily-treatment-effect SQL
	// also references metric IDs but uses `ms.metric_id = '...'` (qualified)
	// so it does not collide with the literal-projection search below.
	calls := executor.GetCalls()
	dailyIdxOf := func(metricID string) int {
		needle := "'" + metricID + "' AS metric_id"
		for i, c := range calls {
			if strings.Contains(c.SQL, needle) {
				return i
			}
		}
		return -1
	}

	idxSession := dailyIdxOf("session_score")
	idxClick := dailyIdxOf("click_rate")
	idxComposite := dailyIdxOf("engagement_index")

	require.NotEqual(t, -1, idxSession, "session_score daily-metric SQL must be executed")
	require.NotEqual(t, -1, idxClick, "click_rate daily-metric SQL must be executed")
	require.NotEqual(t, -1, idxComposite, "engagement_index COMPOSITE SQL must be executed")

	// Topological invariant: both operands strictly precede the COMPOSITE.
	// Order between session_score and click_rate is intentionally unconstrained.
	assert.Less(t, idxSession, idxComposite,
		"session_score must execute before engagement_index (COMPOSITE reads operand rows from delta.metric_summaries)")
	assert.Less(t, idxClick, idxComposite,
		"click_rate must execute before engagement_index (COMPOSITE reads operand rows from delta.metric_summaries)")

	// All three metrics must be recorded as Completed in metric_computation_status.
	snap := statusWriter.Snapshot()
	byMetric := make(map[string]status.Entry, len(snap))
	for _, e := range snap {
		byMetric[e.MetricID] = e
	}
	for _, id := range []string{"session_score", "click_rate", "engagement_index"} {
		entry, ok := byMetric[id]
		require.Truef(t, ok, "%s must have a status row in the happy path", id)
		assert.Equalf(t, status.Completed, entry.Status,
			"%s status must be Completed (got %v)", id, entry.Status)
	}
}

// TestStandardJob_Run_SkippedCompositeDoesNotPostProcess is the Devin BUG-0002
// regression on #556: when a COMPOSITE is skipped (operand missing/failed,
// cycle, etc.), the post-processing loops at standard.go:436 (daily treatment
// effects) and standard.go:478 (QoE-engagement correlation) must NOT iterate
// over it. Before the fix, both loops walked the unfiltered `metrics` slice
// and rendered post-processing SQL that read from delta.metric_summaries for
// the skipped COMPOSITE — writing empty/stale rows to
// delta.daily_treatment_effects.
func TestStandardJob_Run_SkippedCompositeDoesNotPostProcess(t *testing.T) {
	// Inline fixture: a COMPOSITE whose operands are NOT in this experiment's
	// metric list, so the topo-order gate marks it SkippedUpstreamFailure. The
	// experiment has a control variant so the daily-treatment-effect post-pass
	// would otherwise fire for every entry in `metrics`.
	dir := t.TempDir()
	fixturePath := filepath.Join(dir, "seed_skipped_no_post.json")
	const fixture = `{
		"experiments": [
			{
				"experiment_id": "e0000000-0000-0000-0000-00000000bb02",
				"name": "skip_no_post",
				"type": "STANDARD",
				"state": "RUNNING",
				"started_at": "2026-05-01",
				"primary_metric_id": "session_score",
				"secondary_metric_ids": ["engagement_index"],
				"variants": [
					{"variant_id": "control", "name": "Control", "traffic_fraction": 0.5, "is_control": true},
					{"variant_id": "treatment", "name": "Treatment", "traffic_fraction": 0.5, "is_control": false}
				]
			}
		],
		"metrics": [
			{
				"metric_id": "session_score",
				"name": "Session score (MEAN)",
				"type": "MEAN",
				"source_event_type": "session_end",
				"value_column": "session_score"
			},
			{
				"metric_id": "engagement_index",
				"name": "Engagement (COMPOSITE, operands OUT of pass)",
				"type": "COMPOSITE",
				"source_event_type": "n/a",
				"operator": "WEIGHTED_SUM",
				"operands": [
					{"metric_id": "watch_time_minutes", "weight": 0.6},
					{"metric_id": "stream_start_rate", "weight": 0.4}
				]
			}
		]
	}`
	require.NoError(t, os.WriteFile(fixturePath, []byte(fixture), 0o600))

	cfgStore, err := config.LoadFromFile(fixturePath)
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	statusWriter := status.NewMockWriter()

	job := NewStandardJob(cfgStore, renderer, executor, qlWriter, WithStatusWriter(statusWriter))

	_, runErr := job.Run(context.Background(), "e0000000-0000-0000-0000-00000000bb02")
	require.NoError(t, runErr)

	// The COMPOSITE must be recorded as SkippedUpstreamFailure.
	snap := statusWriter.Snapshot()
	byMetric := make(map[string]status.Entry, len(snap))
	for _, e := range snap {
		byMetric[e.MetricID] = e
	}
	require.Equal(t, status.SkippedUpstreamFailure, byMetric["engagement_index"].Status,
		"COMPOSITE with out-of-pass operands must land SkippedUpstreamFailure")
	require.Equal(t, status.Completed, byMetric["session_score"].Status,
		"sibling metric should still complete")

	// Inspect every recorded executor call. No call's SQL should reference the
	// skipped COMPOSITE's metric_id. Daily-treatment-effect SQL has the form
	// `... WHERE ms.metric_id = '<id>' ...` so a substring check is sufficient.
	for _, call := range executor.GetCalls() {
		assert.NotContainsf(t, call.SQL, "'engagement_index'",
			"post-processing must not iterate over skipped COMPOSITE (call SQL: %s)", call.SQL)
	}
}
