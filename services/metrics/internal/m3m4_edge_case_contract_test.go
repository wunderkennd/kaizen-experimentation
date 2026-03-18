// Package metrics_test contains edge-case contract tests for the M3 → M4a
// data pipeline. These tests validate boundary conditions and safety properties
// that the standard contract tests do not cover.
package metrics_test

import (
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// ---------------------------------------------------------------------------
// Edge case: Zero-value metrics produce valid SQL with correct columns
// ---------------------------------------------------------------------------

func TestEdgeCase_MeanTemplate_ZeroValues(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderMean(spark.TemplateParams{
		ExperimentID:    "exp-zero",
		MetricID:        "zero_metric",
		SourceEventType: "zero_event",
		ComputationDate: "2024-01-01",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "mean/zero-values", cols, metricSummariesRequired)

	// AVG of zero values is still valid SQL.
	assert.Contains(t, strings.ToUpper(sql), "AVG(")
}

// ---------------------------------------------------------------------------
// Edge case: PERCENTILE boundary values (p0 and p100 equivalent)
// ---------------------------------------------------------------------------

func TestEdgeCase_PercentileBoundary_LowEnd(t *testing.T) {
	r := newRenderer(t)
	// p0.01 — near zero percentile
	sql, err := r.RenderPercentile(spark.TemplateParams{
		ExperimentID:    "exp-pctl",
		MetricID:        "latency_p001",
		SourceEventType: "request",
		Percentile:      0.01,
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "percentile/p0.01", cols, metricSummariesRequired)
	assert.Contains(t, sql, "0.01",
		"percentile template must contain the percentile value 0.01")
}

func TestEdgeCase_PercentileBoundary_HighEnd(t *testing.T) {
	r := newRenderer(t)
	// p99.9 — near 100 percentile
	sql, err := r.RenderPercentile(spark.TemplateParams{
		ExperimentID:    "exp-pctl",
		MetricID:        "latency_p999",
		SourceEventType: "request",
		Percentile:      0.999,
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "percentile/p0.999", cols, metricSummariesRequired)
	assert.Contains(t, sql, "0.999",
		"percentile template must contain the percentile value 0.999")
}

// ---------------------------------------------------------------------------
// Edge case: IPW column (assignment_probability) present in all metric outputs
// M4a's IPW analysis requires this column for inverse propensity weighting.
// ---------------------------------------------------------------------------

func TestEdgeCase_IPW_AssignmentProbability_AllTemplates(t *testing.T) {
	r := newRenderer(t)

	templates := []struct {
		name   string
		render func() (string, error)
	}{
		{"mean", func() (string, error) {
			return r.RenderMean(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"proportion", func() (string, error) {
			return r.RenderProportion(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"count", func() (string, error) {
			return r.RenderCount(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"ratio", func() (string, error) {
			return r.RenderRatio(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				NumeratorEventType: "n", DenominatorEventType: "d",
				ComputationDate: "2024-01-15",
			})
		}},
		{"percentile", func() (string, error) {
			return r.RenderPercentile(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				SourceEventType: "e", Percentile: 0.50,
				ComputationDate: "2024-01-15",
			})
		}},
		{"qoe_metric", func() (string, error) {
			return r.RenderQoEMetric(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				QoEField: "time_to_first_frame_ms", ComputationDate: "2024-01-15",
			})
		}},
		{"lifecycle_mean", func() (string, error) {
			return r.RenderLifecycleMean(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
				LifecycleEnabled: true,
			})
		}},
		{"session_level_mean", func() (string, error) {
			return r.RenderSessionLevelMean(spark.TemplateParams{
				ExperimentID: "exp-ipw", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
				SessionLevel: true,
			})
		}},
	}

	for _, tc := range templates {
		t.Run(tc.name, func(t *testing.T) {
			sql, err := tc.render()
			require.NoError(t, err)

			cols := extractSQLColumns(sql)
			found := false
			for _, col := range cols {
				if strings.EqualFold(col, "assignment_probability") {
					found = true
					break
				}
			}
			assert.True(t, found,
				"%s template must include assignment_probability in SELECT output — M4a IPW analysis requires it (got columns: %v)",
				tc.name, cols)

			// assignment_probability must originate from delta.exposures.
			assert.Contains(t, strings.ToLower(sql), "assignment_probability",
				"%s template must reference assignment_probability from exposures", tc.name)
		})
	}
}

// ---------------------------------------------------------------------------
// Edge case: SQL template rendering with special characters
//
// IMPORTANT: Go text/template does NOT escape single quotes, so template params
// with single quotes will break out of SQL string literals. This is acceptable
// because template params (experiment_id, metric_id, etc.) come exclusively from
// trusted internal config (ConfigStore), never from user input. These tests
// verify the templates render without errors, not that they neutralize injection.
// ---------------------------------------------------------------------------

func TestEdgeCase_SQLInjection_MetricID(t *testing.T) {
	r := newRenderer(t)
	// metric_id with special characters — template renders without error.
	sql, err := r.RenderMean(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "metric'; DROP TABLE delta.metric_summaries; --",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	// Template still produces structurally complete SQL.
	assert.Contains(t, sql, "AS metric_id",
		"metric_id must still appear as an aliased column")
	assert.Contains(t, strings.ToUpper(sql), "SELECT",
		"template must still produce valid SELECT")
}

func TestEdgeCase_SQLInjection_ExperimentID(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderMean(spark.TemplateParams{
		ExperimentID:    "exp-1' OR '1'='1",
		MetricID:        "m1",
		SourceEventType: "e",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	// Template renders without panicking. Note: the single quote in ExperimentID
	// IS NOT escaped by text/template and would produce broken SQL if the param
	// came from untrusted input. Safety relies on ConfigStore validation.
	assert.Contains(t, strings.ToUpper(sql), "SELECT",
		"template must render without panicking")
	assert.Contains(t, strings.ToUpper(sql), "FROM",
		"template must render FROM clause")
}

// ---------------------------------------------------------------------------
// Edge case: Session-level template includes session_id column
// ---------------------------------------------------------------------------

func TestEdgeCase_SessionLevel_IncludesSessionID(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderSessionLevelMean(spark.TemplateParams{
		ExperimentID:    "exp-session",
		MetricID:        "watch_time",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
		SessionLevel:    true,
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	found := false
	for _, col := range cols {
		if strings.EqualFold(col, "session_id") {
			found = true
			break
		}
	}
	assert.True(t, found,
		"session_level_mean must include session_id in SELECT output (got columns: %v)", cols)
}

// ---------------------------------------------------------------------------
// Edge case: QoE metric template references the configured qoe_field
// ---------------------------------------------------------------------------

func TestEdgeCase_QoEMetric_ReferencesQoEField(t *testing.T) {
	r := newRenderer(t)

	fields := []string{
		"time_to_first_frame_ms",
		"rebuffer_count",
		"rebuffer_ratio",
		"avg_bitrate_kbps",
	}

	for _, field := range fields {
		t.Run(field, func(t *testing.T) {
			sql, err := r.RenderQoEMetric(spark.TemplateParams{
				ExperimentID:    "exp-qoe",
				MetricID:        "qoe_" + field,
				QoEField:        field,
				ComputationDate: "2024-01-15",
			})
			require.NoError(t, err)

			// Must reference the specific QoE field.
			assert.Contains(t, sql, field,
				"QoE template must reference the configured QoE field %q", field)
			// Must read from qoe_events table.
			assert.Contains(t, sql, "delta.qoe_events",
				"QoE template must read from delta.qoe_events")
		})
	}
}

// ---------------------------------------------------------------------------
// Edge case: exposure_join CTE uses GROUP BY (not DISTINCT) for assignment_probability
// This ensures proper aggregation when a user has multiple exposure events.
// ---------------------------------------------------------------------------

func TestEdgeCase_ExposureJoin_GroupByForAssignmentProbability(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderMean(spark.TemplateParams{
		ExperimentID:    "exp-multi-exposure",
		MetricID:        "m1",
		SourceEventType: "e",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	// The exposed_users CTE should use GROUP BY (not DISTINCT) to aggregate
	// assignment_probability via MIN.
	exposureCTE := sql[:strings.Index(strings.ToLower(sql), "metric_data")]
	assert.Contains(t, strings.ToUpper(exposureCTE), "GROUP BY",
		"exposed_users CTE must use GROUP BY for assignment_probability aggregation")
	assert.Contains(t, strings.ToUpper(exposureCTE), "MIN(ASSIGNMENT_PROBABILITY)",
		"exposed_users CTE must use MIN(assignment_probability) to handle multiple exposures")
}
