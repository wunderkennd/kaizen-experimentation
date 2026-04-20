package spark

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func goldenPath(name string) string {
	_, filename, _, _ := runtime.Caller(0)
	return filepath.Join(filepath.Dir(filename), "..", "..", "testdata", "golden", name)
}

func readGolden(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(goldenPath(name))
	require.NoError(t, err)
	return strings.TrimSpace(string(data))
}

var testParams = TemplateParams{ExperimentID: "exp-001", ComputationDate: "2024-01-15"}

func TestRenderMean(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "watch_time_minutes"
	p.SourceEventType = "heartbeat"
	sql, err := r.RenderMean(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "mean_expected.sql"), sql)
}

func TestRenderProportion(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "ctr_recommendation"
	p.SourceEventType = "impression"
	sql, err := r.RenderProportion(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "proportion_expected.sql"), sql)
}

func TestRenderCount(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "stream_start_count"
	p.SourceEventType = "stream_start"
	sql, err := r.RenderCount(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "count_expected.sql"), sql)
}

func TestRenderRatio(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "rebuffer_rate"
	p.NumeratorEventType = "rebuffer_event"
	p.DenominatorEventType = "playback_minute"
	sql, err := r.RenderRatio(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "ratio_expected.sql"), sql)
}

func TestRenderRatioDeltaMethod(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "rebuffer_rate"
	p.NumeratorEventType = "rebuffer_event"
	p.DenominatorEventType = "playback_minute"
	sql, err := r.RenderRatioDeltaMethod(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "ratio_delta_method_expected.sql"), sql)
}

func TestRenderForType(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "test_metric"
	p.SourceEventType = "test_event"
	p.NumeratorEventType = "num_event"
	p.DenominatorEventType = "denom_event"
	for _, tc := range []struct{ mt string; wantErr bool }{
		{"MEAN", false}, {"PROPORTION", false}, {"COUNT", false}, {"RATIO", false},
		{"mean", false}, {"ratio", false}, {"CUSTOM", true}, {"INVALID", true},
	} {
		t.Run(tc.mt, func(t *testing.T) {
			_, err := r.RenderForType(tc.mt, p)
			if tc.wantErr { assert.Error(t, err) } else { assert.NoError(t, err) }
		})
	}
}

func TestRenderRatio_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-123", MetricID: "my_ratio", NumeratorEventType: "revenue", DenominatorEventType: "sessions", ComputationDate: "2024-06-01"}
	sql, _ := r.RenderRatio(p)
	assert.Contains(t, sql, "test-exp-123")
	assert.Contains(t, sql, "numerator_sum / per_user.denominator_sum")
	deltaSQL, _ := r.RenderRatioDeltaMethod(p)
	assert.Contains(t, deltaSQL, "VAR_SAMP(per_user.numerator_sum)")
	assert.Contains(t, deltaSQL, "COVAR_SAMP(per_user.numerator_sum, per_user.denominator_sum)")
}

func TestRenderCupedCovariate(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := testParams
	p.MetricID = "watch_time_minutes"
	p.CupedEnabled = true
	p.CupedCovariateEventType = "heartbeat"
	p.ExperimentStartDate = "2024-01-08"
	p.CupedLookbackDays = 7
	sql, err := r.RenderCupedCovariate(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "cuped_covariate_expected.sql"), sql)
}

func TestRenderCupedCovariate_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-456", MetricID: "my_metric", ComputationDate: "2024-06-01", CupedEnabled: true, CupedCovariateEventType: "heartbeat", ExperimentStartDate: "2024-05-20", CupedLookbackDays: 7}
	sql, _ := r.RenderCupedCovariate(p)
	assert.Contains(t, sql, "test-exp-456")
	assert.Contains(t, sql, "cuped_covariate")
	assert.Contains(t, sql, "DATE_SUB")
}

func TestRenderSQL_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-123", MetricID: "my_metric", SourceEventType: "click", ComputationDate: "2024-06-01"}
	sql, _ := r.RenderMean(p)
	assert.Contains(t, sql, "test-exp-123")
	assert.Contains(t, sql, "delta.exposures")
	assert.Contains(t, sql, "AVG(metric_data.value)")
}

func TestRenderGuardrailMetric(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := testParams
	p.MetricID = "rebuffer_rate"
	p.SourceEventType = "qoe_rebuffer"
	sql, err := r.RenderGuardrailMetric(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "guardrail_metric_expected.sql"), sql)
}

func TestRenderGuardrailMetric_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-789", MetricID: "my_guardrail", SourceEventType: "error_event", ComputationDate: "2024-06-15"}
	sql, _ := r.RenderGuardrailMetric(p)
	assert.Contains(t, sql, "test-exp-789")
	assert.Contains(t, sql, "GROUP BY eu.variant_id")
	assert.Contains(t, sql, "current_value")
}

func TestRenderQoEMetric(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "ttff_mean"
	p.QoEField = "time_to_first_frame_ms"
	sql, err := r.RenderQoEMetric(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "qoe_metric_expected.sql"), sql)
}

func TestRenderQoEMetric_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-qoe", MetricID: "rebuffer_ratio_mean", QoEField: "rebuffer_ratio", ComputationDate: "2024-06-01"}
	sql, _ := r.RenderQoEMetric(p)
	assert.Contains(t, sql, "delta.qoe_events")
	assert.Contains(t, sql, "rebuffer_ratio")
	assert.Contains(t, sql, "AVG(qoe_data.value)")
	assert.NotContains(t, sql, "delta.metric_events", "QoE metric should NOT read from metric_events")
}

func TestRenderEBVSRate(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "ebvs_rate"
	sql, err := r.RenderEBVSRate(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "ebvs_rate_expected.sql"), sql)
}

func TestRenderEBVSRate_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-ebvs", MetricID: "ebvs_rate", ComputationDate: "2024-06-01"}
	sql, _ := r.RenderEBVSRate(p)
	assert.Contains(t, sql, "delta.qoe_events")
	assert.Contains(t, sql, "ebvs_detected")
	assert.Contains(t, sql, "CASE WHEN qoe_sessions.ebvs_detected THEN 1 ELSE 0 END")
	assert.Contains(t, sql, "NULLIF(COUNT(*), 0)", "denominator must guard against zero-session variants")
	assert.NotContains(t, sql, "delta.metric_events", "EBVS rate reads qoe_events, not metric_events")
}

func TestRenderContentConsumption(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.ContentIDField = "content_id"
	sql, err := r.RenderContentConsumption(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "content_consumption_expected.sql"), sql)
}

func TestRenderContentConsumption_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-cc", ComputationDate: "2024-06-01", ContentIDField: "content_id"}
	sql, _ := r.RenderContentConsumption(p)
	assert.Contains(t, sql, "watch_time_seconds")
	assert.Contains(t, sql, "view_count")
	assert.Contains(t, sql, "unique_viewers")
	assert.Contains(t, sql, "GROUP BY content_events.variant_id, content_events.content_id")
}

func TestRenderDailyTreatmentEffect(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "watch_time_minutes"
	p.ControlVariantID = "ctrl-001"
	sql, err := r.RenderDailyTreatmentEffect(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "daily_treatment_effect_expected.sql"), sql)
}

func TestRenderDailyTreatmentEffect_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-te", MetricID: "my_metric", ComputationDate: "2024-06-01", ControlVariantID: "ctrl-variant"}
	sql, _ := r.RenderDailyTreatmentEffect(p)
	assert.Contains(t, sql, "delta.metric_summaries")
	assert.Contains(t, sql, "control_mean")
	assert.Contains(t, sql, "treatment_mean")
	assert.Contains(t, sql, "absolute_effect")
	assert.Contains(t, sql, "ctrl-variant")
}

func TestRenderLifecycleMean(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "watch_time_minutes"
	p.SourceEventType = "heartbeat"
	p.LifecycleEnabled = true
	sql, err := r.RenderLifecycleMean(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "lifecycle_mean_expected.sql"), sql)
}

func TestRenderLifecycleMean_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-lc", MetricID: "my_metric", SourceEventType: "heartbeat", ComputationDate: "2024-06-01", LifecycleEnabled: true}
	sql, _ := r.RenderLifecycleMean(p)
	assert.Contains(t, sql, "lifecycle_segment")
	assert.Contains(t, sql, "GROUP BY metric_data.user_id, metric_data.variant_id, metric_data.lifecycle_segment")
}

func TestRenderSurrogateInput(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.InputMetricIDs = []string{"watch_time_minutes", "stream_start_rate"}
	p.ObservationWindowDays = 7
	sql, err := r.RenderSurrogateInput(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "surrogate_input_expected.sql"), sql)
}

func TestRenderSurrogateInput_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{
		ExperimentID:          "test-exp-surr",
		ComputationDate:       "2024-06-01",
		InputMetricIDs:        []string{"metric_a", "metric_b", "metric_c"},
		ObservationWindowDays: 14,
	}
	sql, _ := r.RenderSurrogateInput(p)
	assert.Contains(t, sql, "delta.metric_summaries")
	assert.Contains(t, sql, "'metric_a', 'metric_b', 'metric_c'")
	assert.Contains(t, sql, "DATE_SUB")
	assert.Contains(t, sql, "14")
	assert.Contains(t, sql, "GROUP BY ms.variant_id, ms.metric_id")
	assert.Contains(t, sql, "avg_value")
}

func TestRenderInterleavingScore(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.EngagementEventType = "click"
	p.CreditAssignment = "proportional"
	sql, err := r.RenderInterleavingScore(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "interleaving_score_expected.sql"), sql)
}

func TestRenderInterleavingScore_BinaryWin(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-il", ComputationDate: "2024-06-01", EngagementEventType: "click", CreditAssignment: "binary_win"}
	sql, err := r.RenderInterleavingScore(p)
	require.NoError(t, err)
	assert.Contains(t, sql, "CASE WHEN SUM(engagement_value) > 0 THEN 1.0 ELSE 0.0 END AS credit")
	assert.Contains(t, sql, "interleaving_provenance")
}

func TestRenderInterleavingScore_Weighted(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-il", ComputationDate: "2024-06-01", EngagementEventType: "click", CreditAssignment: "weighted"}
	sql, err := r.RenderInterleavingScore(p)
	require.NoError(t, err)
	assert.Contains(t, sql, "SUM(engagement_value) AS credit")
	assert.Contains(t, sql, "interleaving_provenance")
}

func TestRenderInterleavingScore_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-il", ComputationDate: "2024-06-01", EngagementEventType: "click", CreditAssignment: "proportional"}
	sql, _ := r.RenderInterleavingScore(p)
	assert.Contains(t, sql, "delta.exposures")
	assert.Contains(t, sql, "interleaving_provenance")
	assert.Contains(t, sql, "algorithm_scores")
	assert.Contains(t, sql, "winning_algorithm_id")
	assert.Contains(t, sql, "total_engagements")
	assert.Contains(t, sql, "MAP_FROM_ARRAYS")
	assert.Contains(t, sql, "source_algorithm_id", "Interleaving scores should derive credit from source_algorithm_id")
}

func TestRenderSessionLevelMean(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "watch_time_minutes"
	p.SourceEventType = "heartbeat"
	p.SessionLevel = true
	sql, err := r.RenderSessionLevelMean(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "session_level_mean_expected.sql"), sql)
}

func TestRenderSessionLevelMean_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-sl", MetricID: "my_metric", SourceEventType: "heartbeat", ComputationDate: "2024-06-01", SessionLevel: true}
	sql, _ := r.RenderSessionLevelMean(p)
	assert.Contains(t, sql, "session_id")
	assert.Contains(t, sql, "me.session_id")
	assert.Contains(t, sql, "GROUP BY metric_data.user_id, metric_data.session_id, metric_data.variant_id")
}

func TestRenderQoEEngagementCorrelation(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.QoEFieldA = "time_to_first_frame_ms"
	p.EngagementSourceType = "heartbeat"
	sql, err := r.RenderQoEEngagementCorrelation(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "qoe_engagement_correlation_expected.sql"), sql)
}

func TestRenderQoEEngagementCorrelation_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{ExperimentID: "test-exp-corr", QoEFieldA: "rebuffer_ratio", EngagementSourceType: "stream_end", ComputationDate: "2024-06-01"}
	sql, _ := r.RenderQoEEngagementCorrelation(p)
	assert.Contains(t, sql, "delta.qoe_events")
	assert.Contains(t, sql, "delta.metric_events")
	assert.Contains(t, sql, "CORR(joined.qoe_value, joined.engagement_value)")
	assert.Contains(t, sql, "pearson_correlation")
	assert.Contains(t, sql, "STDDEV_SAMP")
}

// ADR-015 Phase 2: MLRATE cross-fitting templates.

func TestRenderMLRATEFeatures(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.ExperimentStartDate = "2024-01-08"
	p.MLRATEFolds = 5
	p.MLRATEFeatureEventTypes = []string{"heartbeat", "stream_start"}
	p.MLRATELookbackDays = 14
	sql, err := r.RenderMLRATEFeatures(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "mlrate_features_expected.sql"), sql)
}

func TestRenderMLRATEFeatures_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{
		ExperimentID:            "test-exp-mlrate",
		ComputationDate:         "2024-06-01",
		ExperimentStartDate:     "2024-05-15",
		MLRATEFolds:             3,
		MLRATEFeatureEventTypes: []string{"heartbeat", "stream_start", "click"},
		MLRATELookbackDays:      7,
	}
	sql, err := r.RenderMLRATEFeatures(p)
	require.NoError(t, err)
	assert.Contains(t, sql, "delta.exposures")
	assert.Contains(t, sql, "delta.metric_events")
	assert.Contains(t, sql, "'heartbeat', 'stream_start', 'click'")
	assert.Contains(t, sql, "DATE_SUB")
	assert.Contains(t, sql, "2024-05-15")
	assert.Contains(t, sql, "% 3 + 1")
	assert.Contains(t, sql, "fold_id")
	assert.Contains(t, sql, "feature_heartbeat")
	assert.Contains(t, sql, "feature_stream_start")
	assert.Contains(t, sql, "feature_click")
}

func TestRenderMLRATECrossFitPredict(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "watch_time_minutes"
	p.MLRATEFolds = 5
	p.MLRATEFeatureEventTypes = []string{"heartbeat", "stream_start"}
	p.MLRATEModelURI = "models:/mlrate-watch-time"
	p.MLRATEFoldID = 2
	sql, err := r.RenderMLRATECrossFitPredict(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "mlrate_crossfit_predict_expected.sql"), sql)
}

func TestRenderMLRATECrossFitPredict_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{
		ExperimentID:            "test-exp-mlrate",
		MetricID:                "ctr_recommendation",
		ComputationDate:         "2024-06-01",
		MLRATEFolds:             5,
		MLRATEFeatureEventTypes: []string{"impression", "click"},
		MLRATEModelURI:          "models:/mlrate-ctr",
		MLRATEFoldID:            3,
	}
	sql, err := r.RenderMLRATECrossFitPredict(p)
	require.NoError(t, err)
	assert.Contains(t, sql, "delta.mlrate_features")
	assert.Contains(t, sql, "ai_predict")
	assert.Contains(t, sql, "models:/mlrate-ctr/fold_3")
	assert.Contains(t, sql, "NAMED_STRUCT")
	assert.Contains(t, sql, "mlrate_covariate")
	assert.Contains(t, sql, "fold_id = 3")
	assert.Contains(t, sql, "'ctr_recommendation' AS metric_id")
}

// ADR-021: Feedback loop contamination template.

func TestRenderFeedbackLoopContamination(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)
	p := testParams
	p.MetricID = "watch_time_minutes"
	p.ControlVariantID = "ctrl-001"
	sql, err := r.RenderFeedbackLoopContamination(p)
	require.NoError(t, err)
	assert.Equal(t, readGolden(t, "feedback_loop_contamination_expected.sql"), sql)
}

func TestRenderFeedbackLoopContamination_ContainsKeyFields(t *testing.T) {
	r, _ := NewSQLRenderer()
	p := TemplateParams{
		ExperimentID:    "test-exp-fl",
		MetricID:        "some_metric",
		ControlVariantID: "ctrl-variant",
		ComputationDate: "2024-06-01",
	}
	sql, _ := r.RenderFeedbackLoopContamination(p)
	assert.Contains(t, sql, "delta.model_retraining_events")
	assert.Contains(t, sql, "treatment_contamination_fraction")
	assert.Contains(t, sql, "pre_retrain_effect")
	assert.Contains(t, sql, "post_retrain_effect")
	assert.Contains(t, sql, "ARRAY_CONTAINS(active_experiment_ids, 'test-exp-fl')")
	assert.Contains(t, sql, "ctrl-variant")
}
