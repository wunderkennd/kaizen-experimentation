package spark

import (
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// minimalParams returns a TemplateParams with all fields populated to their
// minimum valid values, ensuring every template can render without errors.
func minimalParams() TemplateParams {
	return TemplateParams{
		ExperimentID:         "exp-validation-001",
		MetricID:             "test_metric",
		SourceEventType:      "test_event",
		ComputationDate:      "2024-01-15",
		NumeratorEventType:   "numerator_event",
		DenominatorEventType: "denominator_event",
		CupedEnabled:         true,
		CupedCovariateEventType: "covariate_event",
		ExperimentStartDate:  "2024-01-08",
		CupedLookbackDays:    7,
		QoEField:             "time_to_first_frame_ms",
		ControlVariantID:     "variant-control-001",
		LifecycleEnabled:     true,
		ContentIDField:       "content_id",
		InputMetricIDs:       []string{"metric_a", "metric_b"},
		ObservationWindowDays: 7,
		CreditAssignment:     "proportional",
		EngagementEventType:  "click",
		SessionLevel:         true,
		QoEFieldA:            "time_to_first_frame_ms",
		QoEFieldB:            "watch_time",
		EngagementSourceType: "heartbeat",
		Percentile:           0.50,
		CustomSQL:            "SELECT user_id, AVG(value) AS metric_value FROM delta.metric_events GROUP BY user_id",
		// Provider-side metric fields (ADR-014).
		LongtailThreshold: 0.80,
		ProviderField:     "provider_id",
		GenreField:        "genre",
	}
}

// templateSpec defines a template's name, render function, required SQL fragments,
// and forbidden SQL fragments for validation.
type templateSpec struct {
	name     string
	render   func(*SQLRenderer, TemplateParams) (string, error)
	contains []string // SQL fragments that must appear
	absent   []string // SQL fragments that must NOT appear
}

func allTemplateSpecs() []templateSpec {
	return []templateSpec{
		{
			name:   "mean",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderMean(p) },
			contains: []string{
				"AVG(metric_data.value)", "metric_id", "experiment_id",
				"delta.exposures", "delta.metric_events", "GROUP BY",
			},
		},
		{
			name:   "proportion",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderProportion(p) },
			contains: []string{
				"CASE WHEN COUNT", "metric_id", "delta.exposures",
				"LEFT JOIN metric_data",
			},
		},
		{
			name:   "count",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderCount(p) },
			contains: []string{
				"COUNT(md.value)", "CAST", "DOUBLE", "delta.exposures",
				"LEFT JOIN metric_data",
			},
		},
		{
			name:   "ratio",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderRatio(p) },
			contains: []string{
				"numerator_data", "denominator_data", "per_user",
				"numerator_sum", "denominator_sum",
				"WHEN per_user.denominator_sum = 0.0 THEN 0.0",
			},
		},
		{
			name:   "ratio_delta_method",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderRatioDeltaMethod(p) },
			contains: []string{
				"VAR_SAMP", "COVAR_SAMP",
				"mean_numerator", "mean_denominator",
				"var_numerator", "var_denominator",
			},
		},
		{
			name:   "percentile",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderPercentile(p) },
			contains: []string{
				"PERCENTILE_APPROX", "0.5",
				"delta.exposures", "delta.metric_events",
			},
		},
		{
			name: "custom",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderCustom(p) },
			contains: []string{
				"custom_result", "exposed_users",
				"delta.exposures", "cr.user_id",
			},
		},
		{
			name:   "cuped_covariate",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderCupedCovariate(p) },
			contains: []string{
				"pre_experiment_data", "cuped_covariate",
				"DATE_SUB", "event_date <",
				"2024-01-08", // experiment start date
			},
		},
		{
			name:   "guardrail_metric",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderGuardrailMetric(p) },
			contains: []string{
				"current_value", "variant_id",
				"delta.exposures", "delta.metric_events",
			},
		},
		{
			name:   "daily_treatment_effect",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderDailyTreatmentEffect(p) },
			contains: []string{
				"control_mean", "treatment_mean", "absolute_effect",
				"delta.metric_summaries", "sample_size",
				"variant-control-001", // control variant ID
			},
		},
		{
			name:   "qoe_metric",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderQoEMetric(p) },
			contains: []string{
				"delta.qoe_events", "time_to_first_frame_ms",
				"AVG(qoe_data.value)",
			},
			absent: []string{"delta.metric_events"}, // QoE reads from qoe_events, not metric_events
		},
		{
			name:   "ebvs_rate",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderEBVSRate(p) },
			contains: []string{
				"delta.qoe_events", "ebvs_detected",
				"CASE WHEN qoe_sessions.ebvs_detected THEN 1 ELSE 0 END",
				"NULLIF(COUNT(*), 0)",
			},
			absent: []string{"delta.metric_events"}, // EBVS rate reads from qoe_events
		},
		{
			name:   "lifecycle_mean",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderLifecycleMean(p) },
			contains: []string{
				"lifecycle_segment", "GROUP BY metric_data.user_id, metric_data.variant_id, metric_data.lifecycle_segment",
			},
		},
		{
			name:   "session_level_mean",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderSessionLevelMean(p) },
			contains: []string{
				"session_id", "me.session_id",
				"GROUP BY metric_data.user_id, metric_data.session_id",
			},
		},
		{
			name:   "content_consumption",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderContentConsumption(p) },
			contains: []string{
				"content_id", "watch_time_seconds", "view_count", "unique_viewers",
				"COUNT(DISTINCT content_events.user_id)",
			},
		},
		{
			name:   "surrogate_input",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderSurrogateInput(p) },
			contains: []string{
				"delta.metric_summaries", "metric_id IN",
				"'metric_a'", "'metric_b'", // input metric IDs
				"DATE_SUB",
			},
		},
		{
			name:   "interleaving_score",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderInterleavingScore(p) },
			contains: []string{
				"interleaving_provenance", "source_algorithm_id",
				"winning_algorithm_id", "total_engagements",
				"MAP_FROM_ARRAYS",
			},
		},
		{
			name:   "qoe_engagement_correlation",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderQoEEngagementCorrelation(p) },
			contains: []string{
				"CORR(", "pearson_correlation",
				"delta.qoe_events", "delta.metric_events",
				"STDDEV_SAMP",
			},
		},
		// Provider-side metrics (ADR-014) — experiment level.
		{
			name:   "catalog_coverage_rate",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderCatalogCoverageRate(p) },
			contains: []string{
				"delta.content_catalog", "covered_items", "total_items",
				"NULLIF(ct.total_items", "delta.exposures",
			},
		},
		{
			name:   "catalog_gini_coefficient",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderCatalogGiniCoefficient(p) },
			contains: []string{
				"delta.content_catalog", "gini_coefficient",
				"ROW_NUMBER()", "rank_asc", "total_impressions",
			},
		},
		{
			name:   "catalog_entropy",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderCatalogEntropy(p) },
			contains: []string{
				"catalog_entropy", "LOG(", "total_impressions",
				"variant_content_impressions", "delta.metric_events",
			},
		},
		{
			name:   "longtail_impression_share",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderLongtailImpressionShare(p) },
			contains: []string{
				"PERCENT_RANK()", "longtail_content",
				"longtail_impressions", "0.8", // LongtailThreshold
				"delta.metric_events",
			},
		},
		{
			name:   "provider_exposure_gini",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderProviderExposureGini(p) },
			contains: []string{
				"delta.content_catalog", "provider_gini", "provider_id",
				"ROW_NUMBER()", "rank_asc",
			},
		},
		{
			name:   "provider_exposure_parity",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderProviderExposureParity(p) },
			contains: []string{
				"delta.content_catalog", "provider_parity", "provider_id",
				"MIN(provider_share)", "MAX(provider_share)",
			},
		},
		// Provider-side metrics (ADR-014) — user level.
		{
			name:   "user_genre_entropy",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderUserGenreEntropy(p) },
			contains: []string{
				"delta.content_catalog", "genre", "user_entropy",
				"assignment_probability", "LOG(",
			},
		},
		{
			name:   "user_discovery_rate",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderUserDiscoveryRate(p) },
			contains: []string{
				"pre_experiment_content", "experiment_content", "new_content",
				"2024-01-08", // ExperimentStartDate
				"assignment_probability",
			},
		},
		{
			name:   "user_provider_diversity",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderUserProviderDiversity(p) },
			contains: []string{
				"delta.content_catalog", "distinct_providers", "provider_id",
				"COUNT(DISTINCT", "assignment_probability",
			},
		},
		{
			name:   "intra_list_distance",
			render: func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderIntraListDistance(p) },
			contains: []string{
				"delta.content_catalog", "genre", "POWER(",
				"user_ild", "1.0 -", "assignment_probability",
			},
		},
	}
}

// TestTemplateValidation_AllTemplatesRender verifies every template renders
// successfully with minimal valid parameters and produces non-empty SQL.
func TestTemplateValidation_AllTemplatesRender(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	params := minimalParams()

	for _, spec := range allTemplateSpecs() {
		t.Run(spec.name, func(t *testing.T) {
			sql, err := spec.render(r, params)
			require.NoError(t, err, "template %q should render without error", spec.name)
			assert.NotEmpty(t, sql, "template %q should produce non-empty SQL", spec.name)
		})
	}
}

// TestTemplateValidation_SQLStructure verifies each rendered template contains
// expected SQL fragments and does not contain forbidden fragments.
func TestTemplateValidation_SQLStructure(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	params := minimalParams()

	for _, spec := range allTemplateSpecs() {
		t.Run(spec.name, func(t *testing.T) {
			sql, err := spec.render(r, params)
			require.NoError(t, err)

			for _, fragment := range spec.contains {
				assert.Contains(t, sql, fragment,
					"template %q should contain %q", spec.name, fragment)
			}
			for _, fragment := range spec.absent {
				assert.NotContains(t, sql, fragment,
					"template %q should NOT contain %q", spec.name, fragment)
			}
		})
	}
}

// TestTemplateValidation_ParameterSubstitution verifies that template parameters
// are correctly substituted in the rendered SQL (no raw Go template syntax remains).
func TestTemplateValidation_ParameterSubstitution(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	params := minimalParams()

	for _, spec := range allTemplateSpecs() {
		t.Run(spec.name, func(t *testing.T) {
			sql, err := spec.render(r, params)
			require.NoError(t, err)

			// No unresolved template syntax should remain.
			assert.NotContains(t, sql, "{{", "rendered SQL should not contain raw template opening braces")
			assert.NotContains(t, sql, "}}", "rendered SQL should not contain raw template closing braces")
			assert.NotContains(t, sql, "<no value>", "rendered SQL should not contain <no value> placeholder")

			// The experiment ID should be substituted.
			assert.Contains(t, sql, "exp-validation-001",
				"experiment_id should be substituted")
		})
	}
}

// TestTemplateValidation_SelectOnly verifies all rendered SQL starts with
// SELECT or WITH (no DDL/DML statements).
func TestTemplateValidation_SelectOnly(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	params := minimalParams()

	for _, spec := range allTemplateSpecs() {
		t.Run(spec.name, func(t *testing.T) {
			sql, err := spec.render(r, params)
			require.NoError(t, err)

			upper := strings.ToUpper(strings.TrimSpace(sql))
			isSelect := strings.HasPrefix(upper, "SELECT") || strings.HasPrefix(upper, "WITH")
			assert.True(t, isSelect,
				"template %q should produce SQL starting with SELECT or WITH, got: %.40s...", spec.name, sql)
		})
	}
}

// TestTemplateValidation_NoSemicolons verifies no rendered SQL contains
// semicolons, which would allow statement chaining.
func TestTemplateValidation_NoSemicolons(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	params := minimalParams()

	for _, spec := range allTemplateSpecs() {
		t.Run(spec.name, func(t *testing.T) {
			sql, err := spec.render(r, params)
			require.NoError(t, err)

			assert.NotContains(t, sql, ";",
				"template %q should not contain semicolons (no statement chaining)", spec.name)
		})
	}
}

// TestTemplateValidation_InterleavingCreditAssignmentVariants verifies the
// interleaving_score template renders correctly for all 3 credit assignment modes.
func TestTemplateValidation_InterleavingCreditAssignmentVariants(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	tests := []struct {
		mode     string
		contains string
	}{
		{"binary_win", "CASE WHEN SUM(engagement_value) > 0 THEN 1.0 ELSE 0.0 END"},
		{"weighted", "SUM(engagement_value) AS credit"},
		{"proportional", "CAST(COUNT(*) AS DOUBLE) AS credit"},
	}

	for _, tc := range tests {
		t.Run(tc.mode, func(t *testing.T) {
			params := minimalParams()
			params.CreditAssignment = tc.mode

			sql, err := r.RenderInterleavingScore(params)
			require.NoError(t, err)
			assert.Contains(t, sql, tc.contains,
				"credit_assignment=%q should produce expected SQL fragment", tc.mode)
		})
	}
}

// TestTemplateValidation_EmptyStringParams verifies templates render with empty
// string parameters (simulating missing optional fields) — they should still
// produce valid SQL structure, even if the values are empty placeholders.
func TestTemplateValidation_EmptyStringParams(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	// Templates that use exposure_join and only need basic fields.
	basicTemplates := []struct {
		name   string
		render func(*SQLRenderer, TemplateParams) (string, error)
	}{
		{"mean", func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderMean(p) }},
		{"proportion", func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderProportion(p) }},
		{"count", func(r *SQLRenderer, p TemplateParams) (string, error) { return r.RenderCount(p) }},
	}

	for _, tc := range basicTemplates {
		t.Run(tc.name+"_empty_params", func(t *testing.T) {
			// All empty strings — template should still render (Go templates
			// substitute zero values for missing fields).
			sql, err := tc.render(r, TemplateParams{})
			require.NoError(t, err, "template %q should render with zero-value params", tc.name)
			assert.NotEmpty(t, sql)

			// Structure should be intact: WITH, SELECT, FROM, GROUP BY.
			assert.Contains(t, sql, "WITH")
			assert.Contains(t, sql, "SELECT")
			assert.Contains(t, sql, "FROM")
			assert.Contains(t, sql, "GROUP BY")
		})
	}
}

// TestTemplateValidation_SurrogateInputMetricIDs verifies the surrogate_input
// template correctly iterates over InputMetricIDs.
func TestTemplateValidation_SurrogateInputMetricIDs(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	t.Run("multiple_ids", func(t *testing.T) {
		params := minimalParams()
		params.InputMetricIDs = []string{"watch_time", "stream_starts", "ctr"}

		sql, err := r.RenderSurrogateInput(params)
		require.NoError(t, err)
		assert.Contains(t, sql, "'watch_time'")
		assert.Contains(t, sql, "'stream_starts'")
		assert.Contains(t, sql, "'ctr'")
		// Verify comma separation.
		assert.Contains(t, sql, "'watch_time', 'stream_starts', 'ctr'")
	})

	t.Run("single_id", func(t *testing.T) {
		params := minimalParams()
		params.InputMetricIDs = []string{"single_metric"}

		sql, err := r.RenderSurrogateInput(params)
		require.NoError(t, err)
		assert.Contains(t, sql, "'single_metric'")
		// No trailing comma after single element.
		assert.NotContains(t, sql, "'single_metric',")
	})

	t.Run("empty_ids", func(t *testing.T) {
		params := minimalParams()
		params.InputMetricIDs = []string{}

		sql, err := r.RenderSurrogateInput(params)
		require.NoError(t, err)
		// Should render with empty IN clause.
		assert.Contains(t, sql, "metric_id IN ()")
	})
}

// TestTemplateValidation_PercentileValues verifies the percentile template
// renders different quantile values correctly.
func TestTemplateValidation_PercentileValues(t *testing.T) {
	r, err := NewSQLRenderer()
	require.NoError(t, err)

	percentiles := []float64{0.25, 0.50, 0.75, 0.90, 0.95, 0.99}
	for _, pct := range percentiles {
		params := minimalParams()
		params.Percentile = pct

		sql, err := r.RenderPercentile(params)
		require.NoError(t, err, "percentile=%g should render", pct)
		assert.Contains(t, sql, "PERCENTILE_APPROX",
			"percentile=%g should use PERCENTILE_APPROX", pct)
	}
}

// TestTemplateValidation_TemplateCount ensures we test all templates and
// catch any new templates that are added without validation coverage.
func TestTemplateValidation_TemplateCount(t *testing.T) {
	specs := allTemplateSpecs()
	// 28 renderable templates: 17 original + 10 provider-side metrics from ADR-014
	// + 1 EBVS rate (Issue #425).
	// (exposure_join is a sub-template, not directly rendered.)
	assert.Equal(t, 28, len(specs),
		"allTemplateSpecs should cover all 28 renderable templates; if you added a new template, add it to allTemplateSpecs()")
}
