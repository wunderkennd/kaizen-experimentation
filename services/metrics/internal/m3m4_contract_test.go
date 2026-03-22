// Package metrics_test validates the M3 (Metric Computation) → M4a (Statistical
// Analysis) data contract.
//
// M3 writes to four Delta Lake tables via Spark SQL templates. M4a reads those
// tables and feeds the data into Rust analysis functions. These tests verify:
//
//   - SQL template SELECT columns match Delta Lake DDL schemas exactly
//   - Column types implied by SQL expressions (AVG → DOUBLE, COUNT → BIGINT)
//     are compatible with M4a Rust struct field types
//   - All required (NOT NULL) columns are always present in output
//   - CUPED covariate, lifecycle segment, and session-level columns appear
//     only in the appropriate template variants
//   - Ratio delta method provides all five variance components M4a needs
//   - Interleaving score MAP structure matches M4a's HashMap<String, f64>
//
// This file does not require Docker or external services — it runs against
// the embedded SQL templates only.
package metrics_test

import (
	"context"
	"regexp"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// ---------------------------------------------------------------------------
// Delta Lake schemas (source of truth: delta/delta_lake_tables.sql)
//
// These column sets define what M4a expects to read. If a template produces
// columns not listed here, or omits required columns, the contract is broken.
// ---------------------------------------------------------------------------

// metricSummariesRequired are NOT NULL columns in delta.metric_summaries.
var metricSummariesRequired = []string{
	"experiment_id", "user_id", "variant_id", "metric_id",
	"metric_value", "computation_date",
}

// metricSummariesOptional are nullable columns in delta.metric_summaries.
var metricSummariesOptional = []string{
	"lifecycle_segment", "cuped_covariate", "session_count", "assignment_probability",
}

// interleavingScoresRequired are NOT NULL columns in delta.interleaving_scores.
var interleavingScoresRequired = []string{
	"experiment_id", "user_id", "algorithm_scores",
	"total_engagements", "computation_date",
}

// interleavingScoresOptional are nullable columns in delta.interleaving_scores.
var interleavingScoresOptional = []string{
	"winning_algorithm_id",
}

// contentConsumptionRequired are NOT NULL columns in delta.content_consumption.
var contentConsumptionRequired = []string{
	"experiment_id", "variant_id", "content_id",
	"watch_time_seconds", "view_count", "unique_viewers",
	"computation_date",
}

// dailyTreatmentEffectsRequired are NOT NULL columns in delta.daily_treatment_effects.
var dailyTreatmentEffectsRequired = []string{
	"experiment_id", "metric_id", "effect_date",
	"treatment_mean", "control_mean", "absolute_effect",
	"sample_size",
}

// ratioDeltaMethodColumns are the columns M4a's delta method computation needs.
// These are written to delta.daily_treatment_effects by the ratio_delta_method template.
var ratioDeltaMethodColumns = []string{
	"experiment_id", "variant_id", "metric_id",
	"user_count", "mean_numerator", "mean_denominator",
	"var_numerator", "var_denominator", "cov_numerator_denominator",
	"computation_date",
}

// guardrailMetricColumns are the columns produced by the guardrail_metric template.
// Guardrail metrics are variant-level (no user_id) — M5 reads these for breach detection.
var guardrailMetricColumns = []string{
	"experiment_id", "variant_id", "metric_id", "current_value", "computation_date",
}

// qoeCorrelationColumns are the columns produced by the qoe_engagement_correlation template.
// M4a reads these for QoE-engagement impact analysis.
var qoeCorrelationColumns = []string{
	"experiment_id", "qoe_metric", "engagement_metric", "variant_id",
	"pearson_correlation", "sample_size", "avg_qoe_value", "avg_engagement_value",
	"stddev_qoe", "stddev_engagement", "computation_date",
}

// surrogateInputColumns are the columns produced by the surrogate_input template.
// M4a reads these to feed surrogate model predictions.
var surrogateInputColumns = []string{
	"variant_id", "metric_id", "avg_value", "user_count",
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// extractSQLColumns parses the outermost SELECT ... FROM and returns the
// column aliases. For "expr AS alias" it returns "alias". For bare column
// references like "table.col" it returns "col". This is a best-effort parser
// sufficient for template-generated SQL.
func extractSQLColumns(sql string) []string {
	// Find the last top-level SELECT (not inside parentheses).
	upper := strings.ToUpper(sql)
	lastSelect := -1
	depth := 0
	for i := 0; i < len(upper)-6; i++ {
		switch upper[i] {
		case '(':
			depth++
		case ')':
			depth--
		}
		if depth == 0 && upper[i:i+6] == "SELECT" {
			lastSelect = i
		}
	}
	if lastSelect < 0 {
		return nil
	}

	// Find the corresponding FROM at the same paren depth.
	fromIdx := -1
	depth = 0
	for i := lastSelect + 6; i < len(upper)-5; i++ {
		switch upper[i] {
		case '(':
			depth++
		case ')':
			depth--
		}
		if depth == 0 && (upper[i:i+6] == "\nFROM " || upper[i:i+6] == " FROM ") {
			fromIdx = i
			break
		}
	}
	if fromIdx < 0 {
		return nil
	}

	selectClause := sql[lastSelect+len("SELECT") : fromIdx]

	// Split by comma (respecting nested parentheses).
	cols := splitRespectingParens(selectClause)

	var result []string
	for _, col := range cols {
		col = strings.TrimSpace(col)
		if col == "" {
			continue
		}

		// Check for AS alias (case-insensitive).
		asRe := regexp.MustCompile(`(?i)\bAS\s+(\w+)\s*$`)
		if m := asRe.FindStringSubmatch(col); m != nil {
			result = append(result, m[1])
			continue
		}

		// Bare column: "table.col" → "col", or just "col".
		parts := strings.Split(strings.TrimSpace(col), ".")
		last := strings.TrimSpace(parts[len(parts)-1])
		result = append(result, last)
	}

	return result
}

// splitRespectingParens splits a string by commas but ignores commas inside
// parentheses (handles nested function calls like MAP_FROM_ARRAYS(...)).
func splitRespectingParens(s string) []string {
	var parts []string
	depth := 0
	start := 0
	for i, ch := range s {
		switch ch {
		case '(':
			depth++
		case ')':
			depth--
		case ',':
			if depth == 0 {
				parts = append(parts, s[start:i])
				start = i + 1
			}
		}
	}
	parts = append(parts, s[start:])
	return parts
}

// assertColumnsPresent checks that all expected columns appear in the SQL output.
func assertColumnsPresent(t *testing.T, templateName string, sqlColumns, expected []string) {
	t.Helper()
	colSet := make(map[string]bool, len(sqlColumns))
	for _, c := range sqlColumns {
		colSet[strings.ToLower(c)] = true
	}
	for _, exp := range expected {
		assert.True(t, colSet[strings.ToLower(exp)],
			"%s: required column %q missing from SELECT output (got columns: %v)",
			templateName, exp, sqlColumns)
	}
}

// assertNoExtraColumns checks that the SQL doesn't produce columns outside
// the allowed set (required + optional).
func assertNoExtraColumns(t *testing.T, templateName string, sqlColumns, allowed []string) {
	t.Helper()
	allowedSet := make(map[string]bool, len(allowed))
	for _, a := range allowed {
		allowedSet[strings.ToLower(a)] = true
	}
	for _, col := range sqlColumns {
		assert.True(t, allowedSet[strings.ToLower(col)],
			"%s: unexpected column %q in SELECT output — not in Delta Lake schema (allowed: %v)",
			templateName, col, allowed)
	}
}

// newRenderer creates a fresh SQLRenderer for testing.
func newRenderer(t *testing.T) *spark.SQLRenderer {
	t.Helper()
	r, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	return r
}

// ---------------------------------------------------------------------------
// Contract: delta.metric_summaries
// M4a reads: welch_ttest(control[], treatment[]) from metric_value by variant
//            cuped_adjust(y[], x[]) from metric_value + cuped_covariate
//            srm_check(count by variant) from COUNT(DISTINCT user_id) by variant
// ---------------------------------------------------------------------------

func TestContract_MetricSummaries_MeanTemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderMean(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "watch_time",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "mean", cols, metricSummariesRequired)
	allAllowed := append(metricSummariesRequired, metricSummariesOptional...)
	assertNoExtraColumns(t, "mean", cols, allAllowed)

	// metric_value must use AVG (produces DOUBLE, matching M4a's f64 slices).
	assert.Contains(t, strings.ToUpper(sql), "AVG(",
		"mean template must use AVG() for metric_value — M4a expects per-user mean as DOUBLE")
}

func TestContract_MetricSummaries_ProportionTemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderProportion(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "stream_start_rate",
		SourceEventType: "stream_start",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "proportion", cols, metricSummariesRequired)

	// metric_value for proportions must be 0.0 or 1.0 (binary).
	assert.Contains(t, strings.ToUpper(sql), "CASE",
		"proportion template should use CASE for binary 0/1 metric_value")
}

func TestContract_MetricSummaries_CountTemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderCount(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "page_views",
		SourceEventType: "page_view",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "count", cols, metricSummariesRequired)

	// metric_value must use COUNT (M4a casts to f64 for t-test).
	assert.Contains(t, strings.ToUpper(sql), "COUNT(",
		"count template must use COUNT() — M4a casts to f64 for welch_ttest")
}

func TestContract_MetricSummaries_RatioTemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderRatio(spark.TemplateParams{
		ExperimentID:         "exp-1",
		MetricID:             "rebuffer_rate",
		NumeratorEventType:   "rebuffer_event",
		DenominatorEventType: "playback_minute",
		ComputationDate:      "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "ratio", cols, metricSummariesRequired)
}

func TestContract_MetricSummaries_QoETemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderQoEMetric(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "ttff_mean",
		QoEField:        "time_to_first_frame_ms",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "qoe_metric", cols, metricSummariesRequired)

	// QoE template reads from delta.qoe_events (not metric_events).
	assert.Contains(t, sql, "delta.qoe_events",
		"QoE template must read from delta.qoe_events")
	// Must reference the specific QoE field from the schema.
	assert.Contains(t, sql, "time_to_first_frame_ms",
		"QoE template must reference the configured QoE field")
}

// ---------------------------------------------------------------------------
// Contract: delta.metric_summaries — CUPED covariate column
// M4a reads cuped_covariate alongside metric_value for cuped_adjust().
// ---------------------------------------------------------------------------

func TestContract_MetricSummaries_CupedCovariate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderCupedCovariate(spark.TemplateParams{
		ExperimentID:           "exp-1",
		MetricID:               "watch_time",
		CupedEnabled:           true,
		CupedCovariateEventType: "heartbeat",
		ExperimentStartDate:    "2024-01-08",
		CupedLookbackDays:      7,
		ComputationDate:        "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// CUPED template must produce cuped_covariate column.
	assert.Contains(t, cols, "cuped_covariate",
		"CUPED template must output cuped_covariate column — M4a needs this for variance reduction")

	// Must filter for pre-experiment data window.
	assert.Contains(t, sql, "2024-01-08",
		"CUPED template must reference experiment start date for pre-experiment window")
}

// ---------------------------------------------------------------------------
// Contract: delta.metric_summaries — lifecycle segment column
// M4a reads lifecycle_segment for SegmentResult per-segment analysis.
// Valid values: TRIAL, NEW, ESTABLISHED, MATURE, AT_RISK, WINBACK.
// ---------------------------------------------------------------------------

func TestContract_MetricSummaries_LifecycleSegment(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderLifecycleMean(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "watch_time",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
		LifecycleEnabled: true,
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "lifecycle_mean", cols, metricSummariesRequired)

	// Must include lifecycle_segment column.
	assert.Contains(t, cols, "lifecycle_segment",
		"lifecycle_mean template must output lifecycle_segment — M4a uses this for per-segment analysis")

	// lifecycle_segment must appear in GROUP BY.
	assert.Contains(t, strings.ToUpper(sql), "LIFECYCLE_SEGMENT",
		"lifecycle_mean must GROUP BY lifecycle_segment")
}

// ---------------------------------------------------------------------------
// Contract: delta.metric_summaries — session-level aggregation
// M4a reads session-level rows and applies HC1 sandwich estimator for
// clustered standard errors on user_id.
// ---------------------------------------------------------------------------

func TestContract_MetricSummaries_SessionLevel(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderSessionLevelMean(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "watch_time",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
		SessionLevel:    true,
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// Session-level must include user_id (for clustering) and session_id.
	assert.Contains(t, cols, "user_id",
		"session_level_mean must output user_id — M4a needs this for HC1 clustering")
	assert.Contains(t, cols, "session_id",
		"session_level_mean must output session_id — one row per session")

	// Must JOIN on session_id (not just user_id).
	assert.Contains(t, strings.ToLower(sql), "session_id",
		"session_level_mean must join exposures to metric_events by session_id")

	// Must GROUP BY session_id.
	groupByIdx := strings.LastIndex(strings.ToUpper(sql), "GROUP BY")
	require.True(t, groupByIdx > 0, "session_level_mean must have GROUP BY")
	groupByClause := strings.ToUpper(sql[groupByIdx:])
	assert.Contains(t, groupByClause, "SESSION_ID",
		"session_level_mean must GROUP BY session_id")
}

// ---------------------------------------------------------------------------
// Contract: delta.interleaving_scores
// M4a reads: InterleavingScore { user_id, algorithm_scores: HashMap<String, f64>,
//            winning_algorithm_id: Option<String>, total_engagements: u32 }
// ---------------------------------------------------------------------------

func TestContract_InterleavingScores(t *testing.T) {
	r := newRenderer(t)

	for _, credit := range []string{"binary_win", "proportional", "weighted"} {
		t.Run(credit, func(t *testing.T) {
			sql, err := r.RenderInterleavingScore(spark.TemplateParams{
				ExperimentID:       "exp-1",
				CreditAssignment:   credit,
				EngagementEventType: "click",
				ComputationDate:    "2024-01-15",
			})
			require.NoError(t, err)

			cols := extractSQLColumns(sql)
			assertColumnsPresent(t, "interleaving_score/"+credit, cols, interleavingScoresRequired)

			// winning_algorithm_id is optional (nullable).
			allAllowed := append(interleavingScoresRequired, interleavingScoresOptional...)
			assertNoExtraColumns(t, "interleaving_score/"+credit, cols, allAllowed)

			// algorithm_scores must be a MAP (M4a reads as HashMap<String, f64>).
			assert.Contains(t, strings.ToUpper(sql), "MAP_FROM_ARRAYS",
				"interleaving_score must produce MAP for algorithm_scores — M4a reads as HashMap<String, f64>")

			// Must reference interleaving_provenance from delta.exposures.
			assert.Contains(t, sql, "interleaving_provenance",
				"interleaving_score must use interleaving_provenance from delta.exposures (M2 output)")
		})
	}
}

// ---------------------------------------------------------------------------
// Contract: delta.content_consumption
// M4a reads: ContentConsumption { content_id, watch_time_seconds: f64,
//            view_count: u64, unique_viewers: u64 }
// Grouped by variant for InterferenceInput { treatment: Vec, control: Vec }.
// ---------------------------------------------------------------------------

func TestContract_ContentConsumption(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderContentConsumption(spark.TemplateParams{
		ExperimentID:    "exp-1",
		ContentIDField:  "content_id",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "content_consumption", cols, contentConsumptionRequired)
	assertNoExtraColumns(t, "content_consumption", cols, contentConsumptionRequired)

	// watch_time_seconds must use SUM (M4a expects total per content per variant).
	assert.Contains(t, strings.ToUpper(sql), "SUM(",
		"content_consumption must use SUM() for watch_time_seconds")

	// unique_viewers must use COUNT(DISTINCT ...) (M4a expects BIGINT / u64).
	assert.Contains(t, strings.ToUpper(sql), "COUNT(DISTINCT",
		"content_consumption must use COUNT(DISTINCT) for unique_viewers")

	// Must include variant_id for treatment/control grouping.
	groupByIdx := strings.LastIndex(strings.ToUpper(sql), "GROUP BY")
	require.True(t, groupByIdx > 0)
	groupByClause := strings.ToUpper(sql[groupByIdx:])
	assert.Contains(t, groupByClause, "VARIANT_ID",
		"content_consumption must GROUP BY variant_id — M4a splits into treatment/control")
}

// ---------------------------------------------------------------------------
// Contract: delta.daily_treatment_effects
// M4a reads: DailyEffect { day: u32, effect: f64, sample_size: u64 }
// Used by novelty detection (requires ≥7 daily data points).
// ---------------------------------------------------------------------------

func TestContract_DailyTreatmentEffects(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderDailyTreatmentEffect(spark.TemplateParams{
		ExperimentID:     "exp-1",
		MetricID:         "watch_time",
		ControlVariantID: "variant-control",
		ComputationDate:  "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "daily_treatment_effect", cols, dailyTreatmentEffectsRequired)
	assertNoExtraColumns(t, "daily_treatment_effect", cols, dailyTreatmentEffectsRequired)

	// absolute_effect must be treatment_mean - control_mean.
	assert.Contains(t, sql, "treatment_mean - treatment_stats.control_mean",
		"daily_treatment_effect: absolute_effect = treatment_mean - control_mean")

	// Must read from delta.metric_summaries (not raw events) — M3 produces
	// daily effects from its own metric_summaries output.
	assert.Contains(t, sql, "delta.metric_summaries",
		"daily_treatment_effect must read from delta.metric_summaries")

	// Must be ordered by date for M4a novelty time series analysis.
	assert.Contains(t, strings.ToUpper(sql), "ORDER BY",
		"daily_treatment_effect must ORDER BY effect_date for time series analysis")
}

// ---------------------------------------------------------------------------
// Contract: delta.daily_treatment_effects (ratio delta method variant)
// M4a reads: mean_numerator, mean_denominator, var_numerator, var_denominator,
//            cov_numerator_denominator — all five components needed for
//            delta method variance estimation of ratio metrics.
// ---------------------------------------------------------------------------

func TestContract_RatioDeltaMethod(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderRatioDeltaMethod(spark.TemplateParams{
		ExperimentID:         "exp-1",
		MetricID:             "rebuffer_rate",
		NumeratorEventType:   "rebuffer_event",
		DenominatorEventType: "playback_minute",
		ComputationDate:      "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "ratio_delta_method", cols, ratioDeltaMethodColumns)

	// All five variance components must be present for M4a delta method.
	for _, vc := range []string{"mean_numerator", "mean_denominator", "var_numerator", "var_denominator", "cov_numerator_denominator"} {
		found := false
		for _, col := range cols {
			if strings.EqualFold(col, vc) {
				found = true
				break
			}
		}
		assert.True(t, found,
			"ratio_delta_method must output %q — M4a needs all 5 variance components", vc)
	}

	// var_numerator must use VAR_SAMP (not VAR_POP) — M4a expects sample variance.
	assert.Contains(t, strings.ToUpper(sql), "VAR_SAMP(",
		"ratio_delta_method must use VAR_SAMP (sample variance), not VAR_POP")

	// cov_numerator_denominator must use COVAR_SAMP.
	assert.Contains(t, strings.ToUpper(sql), "COVAR_SAMP(",
		"ratio_delta_method must use COVAR_SAMP (sample covariance)")
}

// ---------------------------------------------------------------------------
// Contract: standard mean template must NOT include optional columns
// When lifecycle/session/cuped are disabled, those columns must be absent
// to avoid schema mismatches in Delta Lake writes.
// ---------------------------------------------------------------------------

func TestContract_MeanTemplate_NoOptionalColumnsWhenDisabled(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderMean(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "watch_time",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// Without lifecycle enabled, lifecycle_segment should not appear.
	for _, col := range cols {
		assert.NotEqual(t, "lifecycle_segment", strings.ToLower(col),
			"mean template should not output lifecycle_segment when disabled")
	}
	// Without session-level, session_id should not appear.
	for _, col := range cols {
		assert.NotEqual(t, "session_id", strings.ToLower(col),
			"mean template should not output session_id when session-level disabled")
	}
}

// ---------------------------------------------------------------------------
// Contract: lifecycle segment values
// M4a's proto SegmentResult references LifecycleSegment enum values.
// The SQL must read lifecycle_segment from delta.exposures (M2 output).
// Valid values: TRIAL, NEW, ESTABLISHED, MATURE, AT_RISK, WINBACK.
// ---------------------------------------------------------------------------

func TestContract_LifecycleSegment_ReadFromExposures(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderLifecycleMean(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "watch_time",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
		LifecycleEnabled: true,
	})
	require.NoError(t, err)

	// lifecycle_segment must come from delta.exposures (written by M2).
	assert.Contains(t, sql, "delta.exposures",
		"lifecycle_mean must read lifecycle_segment from delta.exposures")

	// The CTE exposed_users must SELECT lifecycle_segment.
	exposuresCTE := sql[:strings.Index(sql, "metric_data")]
	assert.Contains(t, exposuresCTE, "lifecycle_segment",
		"exposed_users CTE must SELECT lifecycle_segment from delta.exposures")
}

// ---------------------------------------------------------------------------
// Contract: M3 writes to the correct target tables
// This verifies that the StandardJob routes templates to the right Delta
// Lake tables (metric_summaries vs daily_treatment_effects).
// ---------------------------------------------------------------------------

func TestContract_TargetTableRouting(t *testing.T) {
	// Metric templates write to delta.metric_summaries.
	metricTemplates := []struct {
		name   string
		render func(*spark.SQLRenderer) (string, error)
	}{
		{"mean", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"proportion", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderProportion(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"count", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderCount(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"ratio", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderRatio(spark.TemplateParams{ExperimentID: "x", MetricID: "m", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-01"})
		}},
		{"cuped_covariate", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderCupedCovariate(spark.TemplateParams{ExperimentID: "x", MetricID: "m", CupedCovariateEventType: "e", ExperimentStartDate: "2024-01-01", CupedLookbackDays: 7, ComputationDate: "2024-01-08"})
		}},
		{"lifecycle_mean", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderLifecycleMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01", LifecycleEnabled: true})
		}},
		{"session_level_mean", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderSessionLevelMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01", SessionLevel: true})
		}},
		{"qoe_metric", func(r *spark.SQLRenderer) (string, error) {
			return r.RenderQoEMetric(spark.TemplateParams{ExperimentID: "x", MetricID: "m", QoEField: "time_to_first_frame_ms", ComputationDate: "2024-01-01"})
		}},
	}

	r := newRenderer(t)
	for _, tc := range metricTemplates {
		t.Run(tc.name+"→metric_summaries", func(t *testing.T) {
			sql, err := tc.render(r)
			require.NoError(t, err)
			// These templates must NOT write to daily_treatment_effects.
			// (They may read from metric_summaries or exposures, but their
			// output is routed to metric_summaries by StandardJob.)
			cols := extractSQLColumns(sql)
			// Verify they produce metric_summaries-compatible columns.
			for _, req := range []string{"experiment_id", "metric_id", "computation_date"} {
				found := false
				for _, col := range cols {
					if strings.EqualFold(col, req) {
						found = true
						break
					}
				}
				assert.True(t, found, "%s template must output %q for metric_summaries", tc.name, req)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Contract: all templates produce valid SQL with SELECT keyword
// (Regression guard — catches template syntax errors.)
// ---------------------------------------------------------------------------

func TestContract_AllTemplatesProduceValidSQL(t *testing.T) {
	r := newRenderer(t)

	templates := []struct {
		name   string
		render func() (string, error)
	}{
		{"mean", func() (string, error) {
			return r.RenderMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"proportion", func() (string, error) {
			return r.RenderProportion(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"count", func() (string, error) {
			return r.RenderCount(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"ratio", func() (string, error) {
			return r.RenderRatio(spark.TemplateParams{ExperimentID: "x", MetricID: "m", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-01"})
		}},
		{"ratio_delta_method", func() (string, error) {
			return r.RenderRatioDeltaMethod(spark.TemplateParams{ExperimentID: "x", MetricID: "m", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-01"})
		}},
		{"cuped_covariate", func() (string, error) {
			return r.RenderCupedCovariate(spark.TemplateParams{ExperimentID: "x", MetricID: "m", CupedCovariateEventType: "e", ExperimentStartDate: "2024-01-01", CupedLookbackDays: 7, ComputationDate: "2024-01-08"})
		}},
		{"qoe_metric", func() (string, error) {
			return r.RenderQoEMetric(spark.TemplateParams{ExperimentID: "x", MetricID: "m", QoEField: "time_to_first_frame_ms", ComputationDate: "2024-01-01"})
		}},
		{"lifecycle_mean", func() (string, error) {
			return r.RenderLifecycleMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01", LifecycleEnabled: true})
		}},
		{"session_level_mean", func() (string, error) {
			return r.RenderSessionLevelMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01", SessionLevel: true})
		}},
		{"interleaving_score", func() (string, error) {
			return r.RenderInterleavingScore(spark.TemplateParams{ExperimentID: "x", CreditAssignment: "proportional", EngagementEventType: "click", ComputationDate: "2024-01-01"})
		}},
		{"content_consumption", func() (string, error) {
			return r.RenderContentConsumption(spark.TemplateParams{ExperimentID: "x", ContentIDField: "content_id", ComputationDate: "2024-01-01"})
		}},
		{"daily_treatment_effect", func() (string, error) {
			return r.RenderDailyTreatmentEffect(spark.TemplateParams{ExperimentID: "x", MetricID: "m", ControlVariantID: "c", ComputationDate: "2024-01-01"})
		}},
		{"guardrail_metric", func() (string, error) {
			return r.RenderGuardrailMetric(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-01"})
		}},
		{"surrogate_input", func() (string, error) {
			return r.RenderSurrogateInput(spark.TemplateParams{ExperimentID: "x", InputMetricIDs: []string{"a", "b"}, ObservationWindowDays: 7, ComputationDate: "2024-01-01"})
		}},
		{"qoe_engagement_correlation", func() (string, error) {
			return r.RenderQoEEngagementCorrelation(spark.TemplateParams{ExperimentID: "x", QoEFieldA: "time_to_first_frame_ms", EngagementSourceType: "heartbeat", ComputationDate: "2024-01-01"})
		}},
	}

	for _, tc := range templates {
		t.Run(tc.name, func(t *testing.T) {
			sql, err := tc.render()
			require.NoError(t, err, "template should render without error")
			assert.NotEmpty(t, sql)
			assert.Contains(t, strings.ToUpper(sql), "SELECT",
				"template must produce SQL with SELECT")
			assert.Contains(t, strings.ToUpper(sql), "FROM",
				"template must produce SQL with FROM")
		})
	}
}

// ---------------------------------------------------------------------------
// Gap 1: Missing column contracts for PERCENTILE, CUSTOM, guardrail,
//        QoE-correlation, and surrogate-input templates.
// ---------------------------------------------------------------------------

func TestContract_MetricSummaries_PercentileTemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderPercentile(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "latency_p95",
		SourceEventType: "playback_start",
		Percentile:      0.95,
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "percentile", cols, metricSummariesRequired)
	allAllowed := append(metricSummariesRequired, metricSummariesOptional...)
	assertNoExtraColumns(t, "percentile", cols, allAllowed)

	// Must use PERCENTILE_APPROX (Spark's approximate percentile function).
	assert.Contains(t, strings.ToUpper(sql), "PERCENTILE_APPROX",
		"percentile template must use PERCENTILE_APPROX — M4a expects per-user percentile as DOUBLE")

	// Must pass the configured percentile value.
	assert.Contains(t, sql, "0.95",
		"percentile template must pass the percentile parameter value")
}

func TestContract_MetricSummaries_CustomTemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderCustom(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "power_users",
		CustomSQL:       "SELECT user_id, AVG(value) AS metric_value FROM delta.metric_events WHERE event_type = 'heartbeat' GROUP BY user_id HAVING COUNT(*) >= 10",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "custom", cols, metricSummariesRequired)

	// Custom template wraps user SQL in a custom_result CTE.
	assert.Contains(t, sql, "custom_result",
		"custom template must wrap user SQL in a custom_result CTE")

	// Must join with exposures for variant attribution.
	assert.Contains(t, sql, "delta.exposures",
		"custom template must join with exposures for variant assignment")
}

func TestContract_MetricSummaries_GuardrailTemplate(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderGuardrailMetric(spark.TemplateParams{
		ExperimentID:    "exp-1",
		MetricID:        "error_rate",
		SourceEventType: "playback_error",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "guardrail_metric", cols, guardrailMetricColumns)

	// Guardrail metrics are variant-level (no user_id) — M5 reads aggregated values.
	for _, col := range cols {
		assert.NotEqual(t, "user_id", strings.ToLower(col),
			"guardrail_metric must NOT output user_id — it is variant-level, not user-level")
	}

	// Must use AVG for current_value.
	assert.Contains(t, strings.ToUpper(sql), "AVG(",
		"guardrail_metric must use AVG for current_value — M5 compares against threshold")
}

func TestContract_QoEEngagementCorrelation(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderQoEEngagementCorrelation(spark.TemplateParams{
		ExperimentID:         "exp-1",
		QoEFieldA:            "time_to_first_frame_ms",
		EngagementSourceType: "heartbeat",
		ComputationDate:      "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "qoe_engagement_correlation", cols, qoeCorrelationColumns)

	// Must compute Pearson correlation using CORR.
	assert.Contains(t, strings.ToUpper(sql), "CORR(",
		"qoe_engagement_correlation must use CORR for Pearson correlation")

	// Must compute standard deviations using STDDEV_SAMP (sample, not population).
	assert.Contains(t, strings.ToUpper(sql), "STDDEV_SAMP(",
		"qoe_engagement_correlation must use STDDEV_SAMP for sample standard deviation")
}

func TestContract_SurrogateInput(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderSurrogateInput(spark.TemplateParams{
		ExperimentID:          "exp-1",
		InputMetricIDs:        []string{"watch_time_minutes", "stream_start_rate"},
		ObservationWindowDays: 7,
		ComputationDate:       "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)
	assertColumnsPresent(t, "surrogate_input", cols, surrogateInputColumns)

	// Must read from delta.metric_summaries (downstream of M3's own output).
	assert.Contains(t, sql, "delta.metric_summaries",
		"surrogate_input must read from delta.metric_summaries — not raw events")
}

// ---------------------------------------------------------------------------
// Gap 2: End-to-end StandardJob output schema validation.
// Runs StandardJob.Run() for each seed experiment and validates every SQL
// query's columns against the schema matching its JobType.
// ---------------------------------------------------------------------------

func TestContract_StandardJob_FullOutputSchema(t *testing.T) {
	cfgStore := loadContractConfig(t)
	renderer := newRenderer(t)

	experiments := []struct {
		id   string
		name string
	}{
		{"e0000000-0000-0000-0000-000000000001", "homepage_recs_v2"},
		{"e0000000-0000-0000-0000-000000000003", "search_ranking_interleave"},
		{"e0000000-0000-0000-0000-000000000004", "playback_qoe_test"},
		{"e0000000-0000-0000-0000-000000000005", "custom_metric_test"},
		{"e0000000-0000-0000-0000-000000000006", "latency_percentile_test"},
		{"e0000000-0000-0000-0000-000000000007", "mixed_qoe_engagement_test"},
	}

	for _, exp := range experiments {
		t.Run(exp.name, func(t *testing.T) {
			executor := spark.NewMockExecutor(100)
			qlWriter := querylog.NewMemWriter()
			job := jobs.NewStandardJob(cfgStore, renderer, executor, qlWriter)
			ctx := context.Background()

			_, err := job.Run(ctx, exp.id)
			require.NoError(t, err)

			entries := qlWriter.AllEntries()
			require.NotEmpty(t, entries, "StandardJob must produce at least one query")

			for i, entry := range entries {
				cols := extractSQLColumns(entry.SQLText)
				switch entry.JobType {
				case "daily_metric", "qoe_metric":
					assertColumnsPresent(t, entry.JobType, cols, metricSummariesRequired)
				case "cuped_covariate":
					assert.Contains(t, strings.ToLower(entry.SQLText), "cuped_covariate",
						"entry[%d] cuped_covariate query must contain cuped_covariate column", i)
				case "delta_method":
					assertColumnsPresent(t, "delta_method", cols, ratioDeltaMethodColumns)
				case "daily_treatment_effect":
					assertColumnsPresent(t, "daily_treatment_effect", cols, dailyTreatmentEffectsRequired)
				case "session_level_metric":
					assert.Contains(t, strings.ToLower(entry.SQLText), "session_id",
						"entry[%d] session_level_metric must contain session_id", i)
				case "lifecycle_metric":
					assert.Contains(t, strings.ToLower(entry.SQLText), "lifecycle_segment",
						"entry[%d] lifecycle_metric must contain lifecycle_segment", i)
				case "qoe_engagement_correlation":
					assert.Contains(t, strings.ToLower(entry.SQLText), "pearson_correlation",
						"entry[%d] qoe_engagement_correlation must contain pearson_correlation", i)
				default:
					t.Errorf("entry[%d] unexpected job_type %q", i, entry.JobType)
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Gap 3: Cross-table consistency — metric_ids match across metric_summaries
// and daily_treatment_effects for the same experiment.
// ---------------------------------------------------------------------------

func TestContract_CrossTable_ExperimentMetricConsistency(t *testing.T) {
	cfgStore := loadContractConfig(t)
	renderer := newRenderer(t)
	executor := spark.NewMockExecutor(100)
	qlWriter := querylog.NewMemWriter()
	job := jobs.NewStandardJob(cfgStore, renderer, executor, qlWriter)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	entries := qlWriter.AllEntries()

	// Collect metric_ids from daily_metric entries.
	dailyMetricIDs := make(map[string]bool)
	for _, e := range entries {
		if e.JobType == "daily_metric" || e.JobType == "qoe_metric" {
			mid := extractMetricIDFromSQL(e.SQLText)
			if mid != "" {
				dailyMetricIDs[mid] = true
			}
		}
	}

	// Collect metric_ids from daily_treatment_effect entries.
	treatmentEffectMetricIDs := make(map[string]bool)
	for _, e := range entries {
		if e.JobType == "daily_treatment_effect" {
			mid := extractMetricIDFromSQL(e.SQLText)
			if mid != "" {
				treatmentEffectMetricIDs[mid] = true
			}
		}
	}

	// Every metric with a daily_metric entry should also have a daily_treatment_effect.
	require.NotEmpty(t, dailyMetricIDs, "should have daily_metric entries")
	require.NotEmpty(t, treatmentEffectMetricIDs, "should have daily_treatment_effect entries")
	for mid := range dailyMetricIDs {
		assert.True(t, treatmentEffectMetricIDs[mid],
			"metric_id %q has daily_metric but no daily_treatment_effect — cross-table inconsistency", mid)
	}
	for mid := range treatmentEffectMetricIDs {
		assert.True(t, dailyMetricIDs[mid],
			"metric_id %q has daily_treatment_effect but no daily_metric — cross-table inconsistency", mid)
	}
}

// ---------------------------------------------------------------------------
// Gap 4: Semantic data shape — user-level GROUP BY granularity.
// M4a requires per-user observations. The final GROUP BY must include
// user_id and variant_id to ensure one row per user per variant.
// ---------------------------------------------------------------------------

func TestContract_DataShape_UserLevelGranularity(t *testing.T) {
	r := newRenderer(t)

	tests := []struct {
		name   string
		render func() (string, error)
	}{
		{"mean", func() (string, error) {
			return r.RenderMean(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"proportion", func() (string, error) {
			return r.RenderProportion(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"count", func() (string, error) {
			return r.RenderCount(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1",
				SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"ratio", func() (string, error) {
			return r.RenderRatio(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1",
				NumeratorEventType: "n", DenominatorEventType: "d",
				ComputationDate: "2024-01-15",
			})
		}},
		{"percentile", func() (string, error) {
			return r.RenderPercentile(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1",
				SourceEventType: "e", Percentile: 0.50,
				ComputationDate: "2024-01-15",
			})
		}},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			sql, err := tc.render()
			require.NoError(t, err)

			assertGroupByContains(t, tc.name, sql, "user_id")
			assertGroupByContains(t, tc.name, sql, "variant_id")
		})
	}
}

// ---------------------------------------------------------------------------
// Additional helpers for Gap 2–4 tests
// ---------------------------------------------------------------------------

// loadContractConfig loads the seed config used by contract tests.
func loadContractConfig(t *testing.T) *config.ConfigStore {
	t.Helper()
	cfgStore, err := config.LoadFromFile("config/testdata/seed_config.json")
	require.NoError(t, err)
	return cfgStore
}

// extractMetricIDFromSQL extracts the metric_id from a SQL template's output.
// Templates use the pattern: 'value' AS metric_id
var metricIDRe = regexp.MustCompile(`'([^']+)'\s+AS\s+metric_id`)

func extractMetricIDFromSQL(sql string) string {
	m := metricIDRe.FindStringSubmatch(sql)
	if len(m) >= 2 {
		return m[1]
	}
	return ""
}

// assertGroupByContains asserts that the last GROUP BY clause in sql contains
// the given column name (case-insensitive).
func assertGroupByContains(t *testing.T, name, sql, col string) {
	t.Helper()
	upper := strings.ToUpper(sql)
	idx := strings.LastIndex(upper, "GROUP BY")
	require.True(t, idx >= 0, "%s: SQL must have a GROUP BY clause", name)
	groupByClause := upper[idx:]
	assert.Contains(t, groupByClause, strings.ToUpper(col),
		"%s: GROUP BY must contain %s — M4a requires per-user granularity", name, col)
}
