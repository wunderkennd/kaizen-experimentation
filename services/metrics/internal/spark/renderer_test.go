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
