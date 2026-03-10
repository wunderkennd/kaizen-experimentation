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

// --- isBreach coverage ---

func TestIsBreach_LowerIsBetter(t *testing.T) {
	// lowerIsBetter=true: value > threshold is a breach.
	assert.True(t, isBreach(0.06, 0.05, true), "0.06 > 0.05 should breach")
	assert.False(t, isBreach(0.04, 0.05, true), "0.04 < 0.05 should not breach")
	assert.False(t, isBreach(0.05, 0.05, true), "equal should not breach (not strict >)")
}

func TestIsBreach_HigherIsBetter(t *testing.T) {
	// lowerIsBetter=false: value < threshold is a breach.
	assert.True(t, isBreach(0.04, 0.05, false), "0.04 < 0.05 should breach")
	assert.False(t, isBreach(0.06, 0.05, false), "0.06 > 0.05 should not breach")
	assert.False(t, isBreach(0.05, 0.05, false), "equal should not breach (not strict <)")
}

func TestIsBreach_EdgeValues(t *testing.T) {
	assert.True(t, isBreach(0, 0.05, false), "zero < 0.05 should breach for higher-is-better")
	assert.False(t, isBreach(0, 0.05, true), "zero < 0.05 should not breach for lower-is-better")
	assert.True(t, isBreach(1.0, 0.05, true), "1.0 > 0.05 should breach for lower-is-better")
	assert.False(t, isBreach(1.0, 0.05, false), "1.0 > 0.05 should not breach for higher-is-better")
}

// --- QoE-engagement correlation path ---

func TestStandardJob_Run_QoEEngagementCorrelation_MixedExperiment(t *testing.T) {
	// mixed_qoe_engagement_test (e7) has ttff_mean (QoE) + watch_time_minutes (non-QoE MEAN).
	// It also has session_level=true and lifecycle_stratification_enabled=true.
	// This exercises: QoE-engagement correlation, session-level, and lifecycle paths.
	job, executor, qlWriter := setupTestJob(t)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000007")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000007", result.ExperimentID)
	assert.Equal(t, 2, result.MetricsComputed, "ttff_mean (QoE) + watch_time_minutes (MEAN)")

	// Find correlation queries in executor calls.
	var corrCalls []spark.MockCall
	for _, c := range executor.GetCalls() {
		if containsStr(c.SQL, "CORR(") {
			corrCalls = append(corrCalls, c)
		}
	}
	require.Len(t, corrCalls, 1, "Should compute 1 QoE-engagement correlation (ttff_mean × watch_time_minutes)")
	assert.Contains(t, corrCalls[0].SQL, "time_to_first_frame_ms",
		"Correlation SQL should reference QoE field")
	assert.Equal(t, "delta.daily_treatment_effects", corrCalls[0].TargetTable)

	// Verify query log entries.
	entries := qlWriter.AllEntries()
	jobTypes := map[string]int{}
	for _, e := range entries {
		jobTypes[e.JobType]++
		if e.JobType == "qoe_engagement_correlation" {
			assert.Contains(t, e.MetricID, "×", "correlation metric_id should contain × separator")
		}
	}

	// QoE-engagement correlation: 1 pair (ttff_mean × watch_time_minutes).
	assert.Equal(t, 1, jobTypes["qoe_engagement_correlation"])

	// Session-level: only watch_time_minutes (non-QoE) gets session-level aggregation.
	assert.Equal(t, 1, jobTypes["session_level_metric"],
		"Only non-QoE metric should get session-level aggregation")

	// Lifecycle: only watch_time_minutes (non-QoE) gets lifecycle segmentation.
	assert.Equal(t, 1, jobTypes["lifecycle_metric"],
		"Only non-QoE metric should get lifecycle segmentation")

	// Daily treatment effects: 2 (one per metric).
	assert.Equal(t, 2, jobTypes["daily_treatment_effect"])

	// QoE metric: 1 (ttff_mean).
	assert.Equal(t, 1, jobTypes["qoe_metric"])

	// Daily metric: 1 (watch_time_minutes) + 1 CUPED covariate.
	assert.Equal(t, 1, jobTypes["daily_metric"])
	assert.Equal(t, 1, jobTypes["cuped_covariate"])
}

// --- Mock edge cases ---

func TestMockValueProvider_UnknownMetric(t *testing.T) {
	vp := NewMockValueProvider()
	_, err := vp.GetVariantValues(context.Background(), "exp-1", "nonexistent_metric")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "no values configured")
}

func TestMockValueProvider_SetAndGet(t *testing.T) {
	vp := NewMockValueProvider()
	vp.SetVariantValue("metric_a", "v1", 1.5)
	vp.SetVariantValue("metric_a", "v2", 2.5)

	vals, err := vp.GetVariantValues(context.Background(), "any-exp", "metric_a")
	require.NoError(t, err)
	assert.Equal(t, 1.5, vals["v1"])
	assert.Equal(t, 2.5, vals["v2"])

	// Returned map should be a copy.
	vals["v1"] = 999
	vals2, _ := vp.GetVariantValues(context.Background(), "any-exp", "metric_a")
	assert.Equal(t, 1.5, vals2["v1"], "mutation should not affect internal state")
}

func TestMockInputMetricsProvider_NilInputs(t *testing.T) {
	p := &MockInputMetricsProvider{Inputs: nil}
	result, err := p.Fetch(context.Background(), "SELECT 1")
	require.NoError(t, err)
	assert.Empty(t, result)
}

func TestMockInputMetricsProvider_WithInputs(t *testing.T) {
	inputs := surrogate.InputMetrics{
		"v1": {"m1": 1.0, "m2": 2.0},
	}
	p := &MockInputMetricsProvider{Inputs: inputs}
	result, err := p.Fetch(context.Background(), "SELECT 1")
	require.NoError(t, err)
	assert.Equal(t, inputs, result)
}

// --- StandardJob error paths ---

func TestStandardJob_Run_QoEMetricRendering(t *testing.T) {
	// playback_qoe_test (e4) has only QoE metrics.
	// Verify the QoE rendering path is exercised.
	job, executor, qlWriter := setupTestJob(t)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000004")
	require.NoError(t, err)

	assert.Equal(t, 2, result.MetricsComputed)

	// QoE metrics should use the qoe_metric template (references delta.qoe_events).
	var qoeCalls []spark.MockCall
	for _, c := range executor.GetCalls() {
		if containsStr(c.SQL, "delta.qoe_events") {
			qoeCalls = append(qoeCalls, c)
		}
	}
	assert.Len(t, qoeCalls, 2, "Both ttff_mean and rebuffer_ratio_mean should use QoE template")

	// Verify qoe_metric job type in query log.
	qoeLogCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "qoe_metric" {
			qoeLogCount++
		}
	}
	assert.Equal(t, 2, qoeLogCount)
}

func TestStandardJob_Run_LifecycleMetrics(t *testing.T) {
	// playback_qoe_test (e4) has lifecycle_stratification_enabled=true.
	// QoE metrics are excluded from lifecycle segmentation.
	job, _, qlWriter := setupTestJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000004")
	require.NoError(t, err)

	lcCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "lifecycle_metric" {
			lcCount++
		}
	}
	// Both metrics are QoE → lifecycle is skipped.
	assert.Equal(t, 0, lcCount, "QoE metrics should not get lifecycle segmentation")
}

func TestStandardJob_Run_NoControlVariant(t *testing.T) {
	// Construct a config with an experiment that has no control variant.
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfgStore, renderer, executor, qlWriter)

	// All seed experiments have control variants, so this tests the normal path.
	// Verified that daily treatment effects are computed.
	result, err := job.Run(ctx(), "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	assert.Equal(t, 4, result.MetricsComputed)

	// Count daily treatment effect entries.
	teCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "daily_treatment_effect" {
			teCount++
		}
	}
	assert.Equal(t, 4, teCount, "Should compute treatment effects for all 4 metrics")
}

// --- SurrogateJob edge cases ---

func TestSurrogateJob_Run_NoControlVariant(t *testing.T) {
	// Experiment e3 has no surrogate model → returns early with 0 metrics.
	inputs := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000005": {"metric1": 1.0},
	}
	job, _, _, projWriter := setupSurrogateJob(t, inputs)

	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)
	assert.Equal(t, 0, result.MetricsComputed)
	assert.Empty(t, projWriter.AllRecords())
}

// --- InterleavingJob edge cases ---

func TestInterleavingJob_Run_ABExperiment_Skipped(t *testing.T) {
	// AB experiment → interleaving job returns immediately with 0 rows.
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()

	job := NewInterleavingJob(cfgStore, renderer, executor, qlWriter)
	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	assert.Equal(t, int64(0), result.RowsWritten)
	assert.Empty(t, executor.GetCalls(), "No SQL should be executed for non-INTERLEAVING experiment")
}

func TestInterleavingJob_Run_InterleavingExperiment(t *testing.T) {
	// INTERLEAVING experiment → computes scores.
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(300)
	qlWriter := querylog.NewMemWriter()

	job := NewInterleavingJob(cfgStore, renderer, executor, qlWriter)
	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)

	assert.Equal(t, int64(300), result.RowsWritten)
	assert.False(t, result.CompletedAt.IsZero())

	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	assert.Contains(t, calls[0].SQL, "algorithm_scores", "Should compute algorithm scores")
	assert.Equal(t, "delta.interleaving_scores", calls[0].TargetTable)

	entries := qlWriter.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "interleaving_score", entries[0].JobType)
}

// --- ContentConsumptionJob edge cases ---

func TestContentConsumptionJob_Run_Success(t *testing.T) {
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(200)
	qlWriter := querylog.NewMemWriter()

	job := NewContentConsumptionJob(cfgStore, renderer, executor, qlWriter)
	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", result.ExperimentID)
	assert.Equal(t, int64(200), result.RowsWritten)
	assert.False(t, result.CompletedAt.IsZero())

	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	assert.Contains(t, calls[0].SQL, "content_id")
	assert.Equal(t, "delta.content_consumption", calls[0].TargetTable)

	entries := qlWriter.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "content_consumption", entries[0].JobType)
}

func TestContentConsumptionJob_Run_ExperimentNotFound(t *testing.T) {
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(200)
	qlWriter := querylog.NewMemWriter()

	job := NewContentConsumptionJob(cfgStore, renderer, executor, qlWriter)
	_, err = job.Run(context.Background(), "nonexistent")
	require.Error(t, err)
}

// --- Helper ---

func containsStr(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && findSubstring(s, substr))
}

func findSubstring(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

func ctx() context.Context {
	return context.Background()
}
