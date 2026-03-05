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

var testParams = TemplateParams{
	ExperimentID:    "exp-001",
	ComputationDate: "2024-01-15",
}

func TestRenderMean(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	p := testParams
	p.MetricID = "watch_time_minutes"
	p.SourceEventType = "heartbeat"

	sql, err := r.RenderMean(p)
	require.NoError(t, err)

	expected := readGolden(t, "mean_expected.sql")
	assert.Equal(t, expected, sql)
}

func TestRenderProportion(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	p := testParams
	p.MetricID = "ctr_recommendation"
	p.SourceEventType = "impression"

	sql, err := r.RenderProportion(p)
	require.NoError(t, err)

	expected := readGolden(t, "proportion_expected.sql")
	assert.Equal(t, expected, sql)
}

func TestRenderCount(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	p := testParams
	p.MetricID = "stream_start_count"
	p.SourceEventType = "stream_start"

	sql, err := r.RenderCount(p)
	require.NoError(t, err)

	expected := readGolden(t, "count_expected.sql")
	assert.Equal(t, expected, sql)
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

	expected := readGolden(t, "ratio_expected.sql")
	assert.Equal(t, expected, sql)
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

	expected := readGolden(t, "ratio_delta_method_expected.sql")
	assert.Equal(t, expected, sql)
}

func TestRenderForType(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	p := testParams
	p.MetricID = "test_metric"
	p.SourceEventType = "test_event"
	p.NumeratorEventType = "num_event"
	p.DenominatorEventType = "denom_event"

	tests := []struct {
		metricType string
		wantErr    bool
	}{
		{"MEAN", false},
		{"PROPORTION", false},
		{"COUNT", false},
		{"RATIO", false},
		{"mean", false},   // case-insensitive
		{"ratio", false},  // case-insensitive
		{"CUSTOM", true},  // unsupported
		{"INVALID", true},
	}

	for _, tt := range tests {
		t.Run(tt.metricType, func(t *testing.T) {
			_, err := r.RenderForType(tt.metricType, p)
			if tt.wantErr {
				assert.Error(t, err)
			} else {
				assert.NoError(t, err)
			}
		})
	}
}

func TestRenderRatio_ContainsKeyFields(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	p := TemplateParams{
		ExperimentID:         "test-exp-123",
		MetricID:             "my_ratio",
		NumeratorEventType:   "revenue",
		DenominatorEventType: "sessions",
		ComputationDate:      "2024-06-01",
	}

	sql, err := r.RenderRatio(p)
	require.NoError(t, err)

	assert.Contains(t, sql, "test-exp-123")
	assert.Contains(t, sql, "my_ratio")
	assert.Contains(t, sql, "revenue")
	assert.Contains(t, sql, "sessions")
	assert.Contains(t, sql, "numerator_sum")
	assert.Contains(t, sql, "denominator_sum")
	assert.Contains(t, sql, "numerator_sum / per_user.denominator_sum")

	deltaSQL, err := r.RenderRatioDeltaMethod(p)
	require.NoError(t, err)

	assert.Contains(t, deltaSQL, "VAR_SAMP(per_user.numerator_sum)")
	assert.Contains(t, deltaSQL, "VAR_SAMP(per_user.denominator_sum)")
	assert.Contains(t, deltaSQL, "COVAR_SAMP(per_user.numerator_sum, per_user.denominator_sum)")
	assert.Contains(t, deltaSQL, "mean_numerator")
	assert.Contains(t, deltaSQL, "mean_denominator")
}

func TestRenderSQL_ContainsKeyFields(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	p := TemplateParams{
		ExperimentID:    "test-exp-123",
		MetricID:        "my_metric",
		SourceEventType: "click",
		ComputationDate: "2024-06-01",
	}

	sql, err := r.RenderMean(p)
	require.NoError(t, err)

	assert.Contains(t, sql, "test-exp-123")
	assert.Contains(t, sql, "my_metric")
	assert.Contains(t, sql, "click")
	assert.Contains(t, sql, "2024-06-01")
	assert.Contains(t, sql, "delta.exposures")
	assert.Contains(t, sql, "delta.metric_events")
	assert.Contains(t, sql, "AVG(metric_data.value)")
}
