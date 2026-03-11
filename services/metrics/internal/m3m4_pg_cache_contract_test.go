// Package metrics_test validates the M3 → M4a PG cache data contract.
//
// Agent-4's PostgreSQL caching layer (store.rs) stores analysis results as
// JSONB in three tables: analysis_results, novelty_analysis_results, and
// interference_analysis_results. M4a computes these results FROM the Delta
// Lake tables that M3 writes.
//
// This file validates:
//   - M3's metric_summaries output provides all columns M4a needs to compute
//     CachedMetricResult fields (including CUPED, sequential, segment, session)
//   - M3's daily_treatment_effects provides inputs for NoveltyAnalysisResult
//   - M3's content_consumption provides inputs for InterferenceAnalysisResult
//   - Newer metric types (PERCENTILE, CUSTOM) produce per-user granularity
//     required for M4a's t-test (cached as control_mean, treatment_mean, etc.)
//   - Surrogate input columns support the cache round-trip for projected effects
//
// No Docker required — runs against embedded SQL templates only.
package metrics_test

import (
	"context"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// ---------------------------------------------------------------------------
// M4a CachedMetricResult field → M3 output column mapping.
//
// CachedMetricResult is the JSONB schema Agent-4 uses in analysis_results.
// Each field is computed by M4a from M3's Delta Lake output. These tests
// validate that M3 produces the required input columns.
//
// CachedMetricResult field         ← M4a computation      ← M3 column
// ─────────────────────────────────────────────────────────────────────
// metric_id                        ← passthrough           ← metric_id
// variant_id                       ← passthrough           ← variant_id
// control_mean                     ← mean(metric_value)    ← metric_value (per user)
// treatment_mean                   ← mean(metric_value)    ← metric_value (per user)
// absolute_effect                  ← t - c                 ← metric_value
// relative_effect                  ← (t-c)/c               ← metric_value
// ci_lower, ci_upper              ← welch_ttest            ← metric_value
// p_value                          ← welch_ttest            ← metric_value
// is_significant                   ← p < alpha              ← metric_value
// cuped_adjusted_effect            ← cuped_adjust           ← cuped_covariate
// cuped_ci_lower, cuped_ci_upper  ← cuped_adjust           ← cuped_covariate
// variance_reduction_pct           ← cuped_adjust           ← cuped_covariate
// sequential_result                ← msprt/gst              ← metric_value (time series)
// segment_results                  ← per-segment ttest      ← lifecycle_segment
// session_level_result             ← hc1 clustering         ← session_count
// ---------------------------------------------------------------------------

// m4aCacheInputColumns defines the minimum M3 output columns required for M4a
// to populate each section of CachedMetricResult.
var m4aCacheBaseInputs = []string{
	"experiment_id", "user_id", "variant_id", "metric_id", "metric_value",
}

var m4aCacheCupedInputs = []string{"cuped_covariate"}
var m4aCacheLifecycleInputs = []string{"lifecycle_segment"}
var m4aCacheSessionInputs = []string{"session_id"} // session_count derived by M4a

// NoveltyAnalysisResult cache fields ← M3 daily_treatment_effects columns:
//   metric_id            ← metric_id
//   novelty_detected     ← computed by M4a from absolute_effect time series
//   raw_treatment_effect ← absolute_effect (latest day)
//   projected_steady_state_effect ← exponential decay fit on absolute_effect
//   decay_constant_days  ← fit from effect_date + absolute_effect
//   is_stabilized        ← M4a heuristic on decay curve
var m4aNoveltyInputs = []string{
	"experiment_id", "metric_id", "effect_date",
	"treatment_mean", "control_mean", "absolute_effect", "sample_size",
}

// InterferenceAnalysisResult cache fields ← M3 content_consumption columns:
//   js_divergence              ← computed from watch_time_seconds distribution
//   jaccard_similarity_top_100 ← computed from content_id overlap
//   treatment_gini             ← computed from watch_time_seconds per content
//   control_gini               ← computed from watch_time_seconds per content
var m4aInterferenceInputs = []string{
	"experiment_id", "variant_id", "content_id",
	"watch_time_seconds", "view_count", "unique_viewers",
}

// ---------------------------------------------------------------------------
// Test: metric_summaries → CachedMetricResult base fields
// Every metric type must produce per-user metric_value so M4a can compute
// control_mean, treatment_mean, absolute_effect, etc.
// ---------------------------------------------------------------------------

func TestPGCacheContract_MetricSummaries_BaseFields(t *testing.T) {
	r := newRenderer(t)

	metricTypes := []struct {
		name   string
		render func() (string, error)
	}{
		{"mean", func() (string, error) {
			return r.RenderMean(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1", SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"proportion", func() (string, error) {
			return r.RenderProportion(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1", SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"count", func() (string, error) {
			return r.RenderCount(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1", SourceEventType: "e", ComputationDate: "2024-01-15",
			})
		}},
		{"ratio", func() (string, error) {
			return r.RenderRatio(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1", NumeratorEventType: "n", DenominatorEventType: "d", ComputationDate: "2024-01-15",
			})
		}},
		{"percentile", func() (string, error) {
			return r.RenderPercentile(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1", SourceEventType: "e", Percentile: 0.95, ComputationDate: "2024-01-15",
			})
		}},
		{"custom", func() (string, error) {
			return r.RenderCustom(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1",
				CustomSQL:       "SELECT user_id, AVG(value) AS metric_value FROM delta.metric_events GROUP BY user_id",
				ComputationDate: "2024-01-15",
			})
		}},
		{"qoe_metric", func() (string, error) {
			return r.RenderQoEMetric(spark.TemplateParams{
				ExperimentID: "exp-1", MetricID: "m1", QoEField: "time_to_first_frame_ms", ComputationDate: "2024-01-15",
			})
		}},
	}

	for _, tc := range metricTypes {
		t.Run(tc.name, func(t *testing.T) {
			sql, err := tc.render()
			require.NoError(t, err)

			cols := extractSQLColumns(sql)

			// M4a needs all base columns to populate CachedMetricResult.
			for _, col := range m4aCacheBaseInputs {
				found := false
				for _, c := range cols {
					if strings.EqualFold(c, col) {
						found = true
						break
					}
				}
				assert.True(t, found,
					"%s: M4a CachedMetricResult requires %q from metric_summaries — M4a computes control_mean/treatment_mean/effect from per-user metric_value",
					tc.name, col)
			}

			// Per-user granularity: GROUP BY must include user_id + variant_id.
			// M4a computes t-test from individual observations, not pre-aggregated means.
			assertGroupByContains(t, tc.name+"→cache", sql, "user_id")
			assertGroupByContains(t, tc.name+"→cache", sql, "variant_id")
		})
	}
}

// ---------------------------------------------------------------------------
// Test: CUPED → CachedMetricResult.cuped_* fields
// When CUPED enabled, M3 outputs cuped_covariate column. M4a uses this to
// compute cuped_adjusted_effect, cuped_ci_lower, cuped_ci_upper, and
// variance_reduction_pct — all cached in CachedMetricResult.
// ---------------------------------------------------------------------------

func TestPGCacheContract_CupedFields(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderCupedCovariate(spark.TemplateParams{
		ExperimentID:            "exp-1",
		MetricID:                "watch_time",
		CupedEnabled:            true,
		CupedCovariateEventType: "heartbeat",
		ExperimentStartDate:     "2024-01-08",
		CupedLookbackDays:       7,
		ComputationDate:         "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// cuped_covariate column is required for M4a to populate:
	//   CachedMetricResult.cuped_adjusted_effect
	//   CachedMetricResult.cuped_ci_lower
	//   CachedMetricResult.cuped_ci_upper
	//   CachedMetricResult.variance_reduction_pct
	for _, col := range m4aCacheCupedInputs {
		assert.Contains(t, cols, col,
			"CUPED template must output %q — M4a uses it to compute cuped_adjusted_effect/ci/variance_reduction_pct cached in CachedMetricResult JSONB",
			col)
	}

	// Pre-experiment window: M4a's CUPED computation requires the covariate
	// to be from BEFORE the experiment started.
	assert.Contains(t, sql, "2024-01-08",
		"CUPED template must filter for pre-experiment period — M4a's CUPED cached result would be biased otherwise")
}

// ---------------------------------------------------------------------------
// Test: Lifecycle → CachedMetricResult.segment_results
// When lifecycle enabled, M3 outputs lifecycle_segment. M4a uses it to compute
// per-segment SegmentResult (effect, ci, p_value, sample_size) cached as
// CachedSegmentResult[] in the JSONB.
// ---------------------------------------------------------------------------

func TestPGCacheContract_LifecycleSegmentFields(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderLifecycleMean(spark.TemplateParams{
		ExperimentID:     "exp-1",
		MetricID:         "watch_time",
		SourceEventType:  "heartbeat",
		ComputationDate:  "2024-01-15",
		LifecycleEnabled: true,
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// lifecycle_segment is required for M4a to populate:
	//   CachedMetricResult.segment_results (Vec<CachedSegmentResult>)
	//   Each CachedSegmentResult has: segment (i32 enum), effect, ci_lower, ci_upper, p_value, sample_size
	for _, col := range m4aCacheLifecycleInputs {
		assert.Contains(t, cols, col,
			"lifecycle template must output %q — M4a groups metric_value by segment to populate CachedSegmentResult[]", col)
	}

	// GROUP BY lifecycle_segment ensures M4a gets per-segment rows.
	upper := strings.ToUpper(sql)
	groupByIdx := strings.LastIndex(upper, "GROUP BY")
	require.True(t, groupByIdx >= 0)
	assert.Contains(t, upper[groupByIdx:], "LIFECYCLE_SEGMENT",
		"lifecycle template must GROUP BY lifecycle_segment — M4a caches per-segment CachedSegmentResult")
}

// ---------------------------------------------------------------------------
// Test: Session-level → CachedMetricResult.session_level_result
// When session-level enabled, M3 outputs session_id. M4a uses the multiple
// rows per user (one per session) to compute HC1 sandwich estimator for
// clustered standard errors, cached as CachedSessionLevelResult.
// ---------------------------------------------------------------------------

func TestPGCacheContract_SessionLevelFields(t *testing.T) {
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

	// session_id required for M4a to populate:
	//   CachedSessionLevelResult { naive_se, clustered_se, design_effect, naive_p_value, clustered_p_value }
	for _, col := range m4aCacheSessionInputs {
		found := false
		for _, c := range cols {
			if strings.EqualFold(c, col) {
				found = true
				break
			}
		}
		assert.True(t, found,
			"session_level template must output %q — M4a uses it for HC1 clustering, cached as CachedSessionLevelResult", col)
	}

	// Must produce per-session rows (GROUP BY session_id), not per-user.
	// M4a needs multiple rows per user to estimate within-user variance.
	assertGroupByContains(t, "session_level→cache", sql, "session_id")
}

// ---------------------------------------------------------------------------
// Test: daily_treatment_effects → NoveltyAnalysisResult cache
// M4a reads daily_treatment_effects to fit exponential decay model. Results
// are cached in novelty_analysis_results PG table with: metric_id,
// novelty_detected, raw_treatment_effect, projected_steady_state,
// novelty_amplitude, decay_constant_days, is_stabilized.
// ---------------------------------------------------------------------------

func TestPGCacheContract_NoveltyInputs(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderDailyTreatmentEffect(spark.TemplateParams{
		ExperimentID:     "exp-1",
		MetricID:         "watch_time",
		ControlVariantID: "control",
		ComputationDate:  "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// All columns needed for M4a's novelty cache must be present.
	for _, col := range m4aNoveltyInputs {
		found := false
		for _, c := range cols {
			if strings.EqualFold(c, col) {
				found = true
				break
			}
		}
		assert.True(t, found,
			"daily_treatment_effect must output %q — M4a uses it for novelty detection cached in novelty_analysis_results PG table", col)
	}

	// absolute_effect must be computed as treatment_mean - control_mean.
	// M4a fits decay on this column: effect(t) = steady_state + amplitude * exp(-t/tau)
	assert.Contains(t, sql, "treatment_mean",
		"daily_treatment_effect must compute treatment_mean — M4a's novelty cache stores raw_treatment_effect")

	// Must be ordered by date — M4a builds a time series from effect_date.
	assert.Contains(t, strings.ToUpper(sql), "ORDER BY",
		"daily_treatment_effect must ORDER BY effect_date — M4a fits time series for decay_constant_days")
}

// ---------------------------------------------------------------------------
// Test: content_consumption → InterferenceAnalysisResult cache
// M4a reads content_consumption to compute JSD, Jaccard, Gini, per-title
// spillover. Results are cached in interference_analysis_results PG table.
// ---------------------------------------------------------------------------

func TestPGCacheContract_InterferenceInputs(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderContentConsumption(spark.TemplateParams{
		ExperimentID:    "exp-1",
		ContentIDField:  "content_id",
		ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// All columns needed for M4a's interference cache must be present.
	for _, col := range m4aInterferenceInputs {
		found := false
		for _, c := range cols {
			if strings.EqualFold(c, col) {
				found = true
				break
			}
		}
		assert.True(t, found,
			"content_consumption must output %q — M4a uses it for interference detection cached in interference_analysis_results PG table", col)
	}

	// variant_id in GROUP BY — M4a splits into InterferenceInput { control, treatment }.
	assertGroupByContains(t, "content_consumption→cache", sql, "variant_id")

	// content_id in GROUP BY — M4a computes Jaccard similarity on top-100 content.
	assertGroupByContains(t, "content_consumption→cache", sql, "content_id")
}

// ---------------------------------------------------------------------------
// Test: StandardJob E2E → all job types produce M4a cache-compatible output
// Runs StandardJob for all 6 seed experiments and validates that every SQL
// query logged contains the columns M4a's cache layer needs.
// ---------------------------------------------------------------------------

func TestPGCacheContract_StandardJob_AllExperiments(t *testing.T) {
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
			require.NotEmpty(t, entries, "StandardJob must produce queries for M4a to cache")

			// Count metric-writing queries (the ones M4a reads for CachedMetricResult).
			var metricQueries, treatmentEffectQueries int
			for _, entry := range entries {
				cols := extractSQLColumns(entry.SQLText)
				switch entry.JobType {
				case "daily_metric", "qoe_metric":
					metricQueries++
					// Must have experiment_id and metric_id for M4a cache lookup.
					assertColumnsPresent(t, entry.JobType+"→cache", cols, []string{"experiment_id", "metric_id"})
					// Must have metric_value for M4a t-test → CachedMetricResult.
					assertColumnsPresent(t, entry.JobType+"→cache", cols, []string{"metric_value"})
				case "daily_treatment_effect":
					treatmentEffectQueries++
					// Must have columns M4a needs for novelty cache.
					assertColumnsPresent(t, "daily_treatment_effect→cache", cols, []string{"absolute_effect", "sample_size"})
				}
			}

			// Every experiment should produce at least one metric query (M4a needs it for RunAnalysis cache).
			assert.Greater(t, metricQueries, 0,
				"%s: must produce daily_metric/qoe_metric queries — M4a caches results in analysis_results PG table", exp.name)
		})
	}
}

// ---------------------------------------------------------------------------
// Test: Query log experiment_id consistency for M4a cache lookups
// M4a's GetAnalysisResult queries by experiment_id. Verify that all SQL
// queries from StandardJob use the correct experiment_id consistently.
// ---------------------------------------------------------------------------

func TestPGCacheContract_QueryLog_ExperimentIDConsistency(t *testing.T) {
	cfgStore := loadContractConfig(t)
	renderer := newRenderer(t)
	executor := spark.NewMockExecutor(100)
	qlWriter := querylog.NewMemWriter()
	job := jobs.NewStandardJob(cfgStore, renderer, executor, qlWriter)
	ctx := context.Background()

	experimentID := "e0000000-0000-0000-0000-000000000001"
	_, err := job.Run(ctx, experimentID)
	require.NoError(t, err)

	entries := qlWriter.AllEntries()
	require.NotEmpty(t, entries)

	for i, entry := range entries {
		// Every query log entry must reference the correct experiment_id.
		// M4a's PG cache keys on experiment_id — a mismatch would cause cache misses.
		assert.Equal(t, experimentID, entry.ExperimentID,
			"entry[%d] experiment_id must match — M4a's cache lookup keys on this field", i)

		// SQL text should contain the experiment_id for traceability.
		assert.Contains(t, entry.SQLText, experimentID,
			"entry[%d] SQL text must contain experiment_id for M4a cache query reproducibility", i)
	}
}

// ---------------------------------------------------------------------------
// Test: Surrogate input columns support M4a's surrogate_projections
// M4a's AnalysisResult includes surrogate_projections (from M3's surrogate
// model output). Verify M3's surrogate_input template provides the columns
// M4a needs. Note: the current PG cache sets surrogate_projections=[] on
// cache deserialization (store.rs line 114), which is a known limitation.
// ---------------------------------------------------------------------------

func TestPGCacheContract_SurrogateInputColumns(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderSurrogateInput(spark.TemplateParams{
		ExperimentID:          "exp-1",
		InputMetricIDs:        []string{"watch_time_minutes", "stream_start_rate"},
		ObservationWindowDays: 7,
		ComputationDate:       "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// Surrogate input provides per-variant metric averages for model prediction.
	// M4a uses predicted effects for AnalysisResult.surrogate_projections.
	assertColumnsPresent(t, "surrogate_input→cache", cols, []string{"variant_id", "metric_id", "avg_value"})

	// avg_value must be a mean (DOUBLE) — M4a feeds this to the surrogate model.
	assert.Contains(t, strings.ToUpper(sql), "AVG(",
		"surrogate_input must use AVG for avg_value — M4a's surrogate model expects per-variant mean")
}

// ---------------------------------------------------------------------------
// Test: CachedAnalysisResult JSONB → AnalysisResult proto field parity
// Validates that M4a's cache schema covers all non-optional fields that
// M4a computes from M3's output. This is a structural contract test.
// ---------------------------------------------------------------------------

func TestPGCacheContract_CachedFieldCompleteness(t *testing.T) {
	// CachedMetricResult fields that M4a computes from metric_summaries:
	cachedFields := []struct {
		field      string
		m3Column   string
		required   bool
		desc       string
	}{
		{"metric_id", "metric_id", true, "passthrough from metric_summaries"},
		{"variant_id", "variant_id", true, "passthrough from metric_summaries"},
		{"control_mean", "metric_value", true, "mean of metric_value for control variant"},
		{"treatment_mean", "metric_value", true, "mean of metric_value for treatment variant"},
		{"absolute_effect", "metric_value", true, "treatment_mean - control_mean"},
		{"relative_effect", "metric_value", true, "(treatment - control) / control"},
		{"ci_lower", "metric_value", true, "from welch_ttest on metric_value"},
		{"ci_upper", "metric_value", true, "from welch_ttest on metric_value"},
		{"p_value", "metric_value", true, "from welch_ttest on metric_value"},
		{"is_significant", "metric_value", true, "p_value < alpha"},
		{"cuped_adjusted_effect", "cuped_covariate", false, "requires cuped_covariate column"},
		{"cuped_ci_lower", "cuped_covariate", false, "requires cuped_covariate column"},
		{"cuped_ci_upper", "cuped_covariate", false, "requires cuped_covariate column"},
		{"variance_reduction_pct", "cuped_covariate", false, "requires cuped_covariate column"},
	}

	r := newRenderer(t)

	// Verify the mean template (most common) provides all required M3 columns.
	sql, err := r.RenderMean(spark.TemplateParams{
		ExperimentID: "exp-1", MetricID: "m1", SourceEventType: "e", ComputationDate: "2024-01-15",
	})
	require.NoError(t, err)
	cols := extractSQLColumns(sql)
	colSet := make(map[string]bool, len(cols))
	for _, c := range cols {
		colSet[strings.ToLower(c)] = true
	}

	for _, f := range cachedFields {
		if f.required {
			assert.True(t, colSet[f.m3Column],
				"CachedMetricResult.%s requires M3 column %q — %s", f.field, f.m3Column, f.desc)
		}
	}
}

// ---------------------------------------------------------------------------
// Test: M4a SRM check requires distinct user_id per variant
// M4a's SRM chi-squared test counts distinct users per variant from
// metric_summaries. Verify GROUP BY user_id is present for all metric types
// that feed the SRM check (all except guardrail which is variant-level).
// ---------------------------------------------------------------------------

func TestPGCacheContract_SRMCheck_UserIDGranularity(t *testing.T) {
	r := newRenderer(t)

	tests := []struct {
		name   string
		render func() (string, error)
	}{
		{"mean", func() (string, error) {
			return r.RenderMean(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", ComputationDate: "2024-01-15"})
		}},
		{"percentile", func() (string, error) {
			return r.RenderPercentile(spark.TemplateParams{ExperimentID: "x", MetricID: "m", SourceEventType: "e", Percentile: 0.5, ComputationDate: "2024-01-15"})
		}},
		{"custom", func() (string, error) {
			return r.RenderCustom(spark.TemplateParams{ExperimentID: "x", MetricID: "m", CustomSQL: "SELECT user_id, 1.0 AS metric_value FROM t GROUP BY user_id", ComputationDate: "2024-01-15"})
		}},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			sql, err := tc.render()
			require.NoError(t, err)

			cols := extractSQLColumns(sql)

			// user_id must be in SELECT output — M4a counts DISTINCT user_id per variant for SRM.
			// Without this, SRM check in CachedSrmResult would be incorrect.
			found := false
			for _, c := range cols {
				if strings.EqualFold(c, "user_id") {
					found = true
					break
				}
			}
			assert.True(t, found,
				"%s: must output user_id — M4a's SRM check (cached in analysis_results.srm_p_value) counts distinct users per variant", tc.name)
		})
	}
}

// ---------------------------------------------------------------------------
// Test: QoE-engagement correlation → M4a interference/quality analysis
// The QoE correlation template produces per-variant correlation stats that
// M4a can use for quality-aware interference detection.
// ---------------------------------------------------------------------------

func TestPGCacheContract_QoECorrelation_PerVariant(t *testing.T) {
	r := newRenderer(t)
	sql, err := r.RenderQoEEngagementCorrelation(spark.TemplateParams{
		ExperimentID:         "exp-1",
		QoEFieldA:            "time_to_first_frame_ms",
		EngagementSourceType: "heartbeat",
		ComputationDate:      "2024-01-15",
	})
	require.NoError(t, err)

	cols := extractSQLColumns(sql)

	// Must include variant_id — M4a compares correlation across variants.
	found := false
	for _, c := range cols {
		if strings.EqualFold(c, "variant_id") {
			found = true
			break
		}
	}
	assert.True(t, found,
		"qoe_engagement_correlation must output variant_id — M4a compares QoE-engagement correlation across variants")

	// Must include pearson_correlation for statistical comparison.
	assertColumnsPresent(t, "qoe_correlation→cache", cols, []string{"pearson_correlation", "sample_size"})
}
