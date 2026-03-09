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

// loadE2EConfig loads the dedicated e2e pipeline test config.
func loadE2EConfig(t *testing.T) *config.ConfigStore {
	t.Helper()
	cfg, err := config.LoadFromFile("testdata/e2e_pipeline_config.json")
	require.NoError(t, err)
	return cfg
}

func setupE2EJob(t *testing.T) (*StandardJob, *spark.MockExecutor, *querylog.MemWriter) {
	t.Helper()
	cfg := loadE2EConfig(t)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	return NewStandardJob(cfg, renderer, executor, qlWriter), executor, qlWriter
}

// =================================================================
// 3.4: Session-level experiment pipeline
// =================================================================

func TestE2E_SessionLevel_StandardMetrics(t *testing.T) {
	// Experiment e2e-session-001: session_level=true with non-QoE metrics
	// (watch_time_minutes MEAN, stream_start_rate PROPORTION).
	// This is the key gap: existing tests only tested session_level with QoE metrics
	// where session_level_mean is skipped.
	job, executor, qlWriter := setupE2EJob(t)
	ctx := context.Background()

	result, err := job.Run(ctx, "e2e-session-001")
	require.NoError(t, err)
	assert.Equal(t, 2, result.MetricsComputed)

	// Verify session-level queries were generated for both metrics.
	calls := executor.GetCalls()
	var sessionCalls []spark.MockCall
	for _, c := range calls {
		if strings.Contains(c.SQL, "me.session_id") {
			sessionCalls = append(sessionCalls, c)
		}
	}
	require.Len(t, sessionCalls, 2, "Both non-QoE metrics should get session-level treatment")

	for _, c := range sessionCalls {
		assert.Contains(t, c.SQL, "session_id IS NOT NULL",
			"Session-level query must filter for non-null session_id")
		assert.Contains(t, c.SQL, "GROUP BY metric_data.user_id, metric_data.session_id",
			"Session-level query must group by user_id and session_id")
		assert.Equal(t, "delta.metric_summaries", c.TargetTable)
	}

	// Verify query log has session_level_metric entries.
	slCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "session_level_metric" {
			slCount++
		}
	}
	assert.Equal(t, 2, slCount, "Both metrics should produce session_level_metric query log entries")
}

func TestE2E_SessionLevel_SQL_JoinsOnSessionID(t *testing.T) {
	// Verify the session-level SQL correctly joins on session_id between
	// exposures and metric_events (not just user_id).
	job, executor, _ := setupE2EJob(t)

	_, err := job.Run(context.Background(), "e2e-session-001")
	require.NoError(t, err)

	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "me.session_id") {
			assert.Contains(t, c.SQL, "me.session_id = eu.session_id",
				"Session-level must join exposures↔events on session_id")
		}
	}
}

// =================================================================
// 3.5: QoE pipeline
// =================================================================

func TestE2E_QoE_OnlyMetrics(t *testing.T) {
	// Experiment e2e-qoe-001: only QoE metrics (ttff_mean, rebuffer_ratio_mean).
	// Verifies: QoE template used, reads from delta.qoe_events (not metric_events),
	// no session-level queries (QoE excluded), treatment effects computed.
	job, executor, qlWriter := setupE2EJob(t)

	result, err := job.Run(context.Background(), "e2e-qoe-001")
	require.NoError(t, err)
	assert.Equal(t, 2, result.MetricsComputed)

	calls := executor.GetCalls()

	// Verify QoE metrics read from delta.qoe_events.
	var qoeCalls []spark.MockCall
	for _, c := range calls {
		if strings.Contains(c.SQL, "delta.qoe_events") {
			qoeCalls = append(qoeCalls, c)
		}
	}
	assert.Len(t, qoeCalls, 2, "Both QoE metrics should read from delta.qoe_events")

	// Verify NO metric reads from delta.metric_events (these are QoE-only).
	for _, c := range qoeCalls {
		assert.NotContains(t, c.SQL, "delta.metric_events",
			"QoE metric should not reference delta.metric_events")
	}

	// Verify QoE field substitution.
	var ttffFound, rebufferFound bool
	for _, c := range qoeCalls {
		if strings.Contains(c.SQL, "time_to_first_frame_ms") {
			ttffFound = true
		}
		if strings.Contains(c.SQL, "rebuffer_ratio") {
			rebufferFound = true
		}
	}
	assert.True(t, ttffFound, "ttff_mean must reference time_to_first_frame_ms field")
	assert.True(t, rebufferFound, "rebuffer_ratio_mean must reference rebuffer_ratio field")

	// Verify query log has qoe_metric entries.
	qoeLogCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "qoe_metric" {
			qoeLogCount++
		}
	}
	assert.Equal(t, 2, qoeLogCount)

	// Verify no session-level queries (QoE metrics are excluded from session-level).
	for _, c := range calls {
		assert.False(t, strings.Contains(c.SQL, "me.session_id"),
			"QoE-only experiment should not produce session-level queries")
	}
}

func TestE2E_QoE_TreatmentEffects(t *testing.T) {
	// QoE experiment should still produce daily treatment effects.
	job, executor, qlWriter := setupE2EJob(t)

	_, err := job.Run(context.Background(), "e2e-qoe-001")
	require.NoError(t, err)

	var teCalls []spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "absolute_effect") {
			teCalls = append(teCalls, c)
		}
	}
	assert.Len(t, teCalls, 2, "Daily treatment effects for both QoE metrics")

	for _, c := range teCalls {
		assert.Equal(t, "delta.daily_treatment_effects", c.TargetTable)
		assert.Contains(t, c.SQL, "ctrl-q1", "Treatment effect must reference control variant")
	}

	teLogCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "daily_treatment_effect" {
			teLogCount++
		}
	}
	assert.Equal(t, 2, teLogCount)
}

func TestE2E_QoE_NoCorrelationWithoutEngagement(t *testing.T) {
	// QoE-only experiment: no non-QoE engagement metrics → no correlation computed.
	job, executor, qlWriter := setupE2EJob(t)

	_, err := job.Run(context.Background(), "e2e-qoe-001")
	require.NoError(t, err)

	for _, c := range executor.GetCalls() {
		assert.NotContains(t, c.SQL, "CORR(",
			"No correlation without engagement metrics")
	}

	for _, e := range qlWriter.AllEntries() {
		assert.NotEqual(t, "qoe_engagement_correlation", e.JobType)
	}
}

// =================================================================
// 3.5: QoE-engagement correlation (mixed experiment)
// =================================================================

func TestE2E_Mixed_QoEEngagementCorrelation(t *testing.T) {
	// Experiment e2e-mixed-001: ttff_mean (QoE) + watch_time_minutes (engagement MEAN)
	//                           + stream_start_rate (engagement PROPORTION).
	// This is the key gap: existing tests never exercised an experiment with both
	// QoE and non-QoE metrics, so the correlation code path was untested.
	job, executor, qlWriter := setupE2EJob(t)

	result, err := job.Run(context.Background(), "e2e-mixed-001")
	require.NoError(t, err)
	assert.Equal(t, 3, result.MetricsComputed)

	// Find correlation queries.
	var corrCalls []spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "CORR(") {
			corrCalls = append(corrCalls, c)
		}
	}

	// 1 QoE metric × 2 engagement metrics = 2 correlation queries.
	assert.Len(t, corrCalls, 2,
		"Should compute correlation for each QoE × engagement pair")

	for _, c := range corrCalls {
		assert.Contains(t, c.SQL, "delta.qoe_events",
			"Correlation must join QoE data")
		assert.Contains(t, c.SQL, "delta.metric_events",
			"Correlation must join engagement data")
		assert.Contains(t, c.SQL, "pearson_correlation")
		assert.Contains(t, c.SQL, "STDDEV_SAMP")
		assert.Equal(t, "delta.daily_treatment_effects", c.TargetTable)
	}

	// Verify query log.
	corrLogCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "qoe_engagement_correlation" {
			corrLogCount++
		}
	}
	assert.Equal(t, 2, corrLogCount)
}

func TestE2E_Mixed_CorrectMetricTypes(t *testing.T) {
	// Verify that in a mixed experiment, QoE metrics use the QoE template
	// and standard metrics use standard templates.
	job, executor, _ := setupE2EJob(t)

	_, err := job.Run(context.Background(), "e2e-mixed-001")
	require.NoError(t, err)

	calls := executor.GetCalls()

	// First call should be the QoE metric (ttff_mean is primary).
	assert.Contains(t, calls[0].SQL, "delta.qoe_events",
		"QoE metric should read from qoe_events")

	// Find standard metric calls (non-QoE).
	var stdCalls []spark.MockCall
	for _, c := range calls {
		if strings.Contains(c.SQL, "delta.metric_events") && !strings.Contains(c.SQL, "CORR(") {
			stdCalls = append(stdCalls, c)
		}
	}
	assert.GreaterOrEqual(t, len(stdCalls), 2,
		"Non-QoE metrics should read from delta.metric_events")
}

// =================================================================
// 3.4: Lifecycle stratification pipeline
// =================================================================

func TestE2E_Lifecycle_Stratification(t *testing.T) {
	// Experiment e2e-lifecycle-001: lifecycle_stratification_enabled=true
	// with non-QoE metrics. Should produce lifecycle_mean queries.
	job, executor, qlWriter := setupE2EJob(t)

	result, err := job.Run(context.Background(), "e2e-lifecycle-001")
	require.NoError(t, err)
	assert.Equal(t, 2, result.MetricsComputed)

	// Find lifecycle queries.
	var lcCalls []spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, "lifecycle_segment") {
			lcCalls = append(lcCalls, c)
		}
	}
	require.Len(t, lcCalls, 2, "Both non-QoE metrics should get lifecycle stratification")

	for _, c := range lcCalls {
		assert.Contains(t, c.SQL, "lifecycle_segment",
			"Lifecycle query must include lifecycle_segment")
		assert.Contains(t, c.SQL, "GROUP BY metric_data.user_id, metric_data.variant_id, metric_data.lifecycle_segment",
			"Lifecycle query must group by lifecycle_segment")
		assert.Equal(t, "delta.metric_summaries", c.TargetTable)
	}

	// Verify query log.
	lcLogCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "lifecycle_metric" {
			lcLogCount++
		}
	}
	assert.Equal(t, 2, lcLogCount)
}

// =================================================================
// Full pipeline: session + lifecycle + QoE + engagement
// =================================================================

func TestE2E_FullPipeline_AllFeaturesCombined(t *testing.T) {
	// Experiment e2e-full-001: session_level + lifecycle + mixed QoE & engagement.
	// Metrics: ttff_mean (QoE), watch_time_minutes (standard MEAN), rebuffer_ratio_mean (QoE).
	// This tests all 3.4/3.5 features firing simultaneously.
	job, executor, qlWriter := setupE2EJob(t)

	result, err := job.Run(context.Background(), "e2e-full-001")
	require.NoError(t, err)
	assert.Equal(t, 3, result.MetricsComputed)

	calls := executor.GetCalls()

	// Categorize all SQL calls.
	var (
		qoeMetricCalls     int
		stdMetricCalls     int
		sessionLevelCalls  int
		lifecycleCalls     int
		correlationCalls   int
		treatmentEffects   int
	)
	for _, c := range calls {
		sql := c.SQL
		switch {
		case strings.Contains(sql, "CORR("):
			correlationCalls++
		case strings.Contains(sql, "absolute_effect"):
			treatmentEffects++
		case strings.Contains(sql, "lifecycle_segment"):
			lifecycleCalls++
		case strings.Contains(sql, "me.session_id"):
			sessionLevelCalls++
		case strings.Contains(sql, "delta.qoe_events"):
			qoeMetricCalls++
		case strings.Contains(sql, "delta.metric_events") || strings.Contains(sql, "delta.exposures"):
			stdMetricCalls++
		}
	}

	// 2 QoE metrics (ttff_mean, rebuffer_ratio_mean).
	assert.Equal(t, 2, qoeMetricCalls, "2 QoE metric queries")

	// 1 standard metric (watch_time_minutes).
	assert.Equal(t, 1, stdMetricCalls, "1 standard metric query")

	// Session-level: only for non-QoE metric (watch_time_minutes). QoE excluded.
	assert.Equal(t, 1, sessionLevelCalls,
		"Session-level only for non-QoE metrics (watch_time_minutes)")

	// Lifecycle: only for non-QoE metric (watch_time_minutes). QoE excluded.
	assert.Equal(t, 1, lifecycleCalls,
		"Lifecycle only for non-QoE metrics (watch_time_minutes)")

	// QoE-engagement correlation: 2 QoE × 1 engagement (watch_time_minutes is MEAN, not RATIO).
	assert.Equal(t, 2, correlationCalls,
		"2 QoE metrics × 1 engagement metric = 2 correlations")

	// Daily treatment effects: 3 metrics.
	assert.Equal(t, 3, treatmentEffects,
		"Daily treatment effects for all 3 metrics")

	// Verify query log completeness.
	entries := qlWriter.AllEntries()
	jobTypeCounts := make(map[string]int)
	for _, e := range entries {
		jobTypeCounts[e.JobType]++
		assert.Equal(t, "e2e-full-001", e.ExperimentID,
			"All query log entries should reference this experiment")
		assert.NotEmpty(t, e.SQLText, "SQL text must be logged")
	}

	assert.Equal(t, 2, jobTypeCounts["qoe_metric"])
	assert.Equal(t, 1, jobTypeCounts["daily_metric"])
	assert.Equal(t, 1, jobTypeCounts["session_level_metric"])
	assert.Equal(t, 1, jobTypeCounts["lifecycle_metric"])
	assert.Equal(t, 2, jobTypeCounts["qoe_engagement_correlation"])
	assert.Equal(t, 3, jobTypeCounts["daily_treatment_effect"])

	// Total expected: 2 QoE + 1 std + 1 session + 1 lifecycle + 2 correlation + 3 TE = 10
	assert.Equal(t, 10, len(entries), "Full pipeline should produce 10 query log entries")
}

func TestE2E_FullPipeline_SQLTransparency(t *testing.T) {
	// Every computation must be logged — verify SQL text is non-empty
	// and contains the experiment ID for traceability.
	job, _, qlWriter := setupE2EJob(t)

	_, err := job.Run(context.Background(), "e2e-full-001")
	require.NoError(t, err)

	for _, e := range qlWriter.AllEntries() {
		assert.NotEmpty(t, e.SQLText, "Every query must have SQL text")
		// Treatment effect queries reference metric_summaries, not experiment_id directly.
		if e.JobType != "daily_treatment_effect" && e.JobType != "qoe_engagement_correlation" {
			assert.Contains(t, e.SQLText, "e2e-full-001",
				"SQL should reference experiment_id for traceability (job_type=%s)", e.JobType)
		}
	}
}

func TestE2E_FullPipeline_DeltaLakeTargetTables(t *testing.T) {
	// Verify correct Delta Lake target tables for each query type.
	job, executor, _ := setupE2EJob(t)

	_, err := job.Run(context.Background(), "e2e-full-001")
	require.NoError(t, err)

	for _, c := range executor.GetCalls() {
		sql := c.SQL
		switch {
		case strings.Contains(sql, "absolute_effect") || strings.Contains(sql, "CORR("):
			assert.Equal(t, "delta.daily_treatment_effects", c.TargetTable,
				"Treatment effects and correlations → daily_treatment_effects")
		default:
			assert.Equal(t, "delta.metric_summaries", c.TargetTable,
				"Metric values → metric_summaries")
		}
	}
}
