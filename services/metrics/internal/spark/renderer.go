package spark

import (
	"bytes"
	"embed"
	"fmt"
	"strings"
	"text/template"
)

//go:embed templates/*.sql.tmpl
var templateFS embed.FS

type TemplateParams struct {
	ExperimentID         string
	MetricID             string
	SourceEventType      string
	ComputationDate      string
	NumeratorEventType   string
	DenominatorEventType string
	CupedEnabled             bool
	CupedCovariateEventType  string
	ExperimentStartDate      string
	CupedLookbackDays        int
	// SVOD-specific fields
	QoEField         string // maps to PlaybackMetrics field (e.g. "time_to_first_frame_ms")
	ControlVariantID string // variant_id of the control variant
	LifecycleEnabled bool   // whether to include lifecycle_segment in GROUP BY
	ContentIDField   string // field name for content identifier (default: "content_id")
	// Surrogate metric fields
	InputMetricIDs        []string // list of metric_ids to aggregate for surrogate input
	ObservationWindowDays int      // how many days of recent data to aggregate
	// Interleaving-specific fields
	CreditAssignment    string // "binary_win", "proportional", or "weighted"
	EngagementEventType string // event_type for engagement events to join with provenance
	// Session-level fields
	SessionLevel bool // whether to aggregate by session_id instead of user_id
	// QoE-engagement correlation fields
	QoEFieldA            string // first QoE field for correlation (e.g. "time_to_first_frame_ms")
	QoEFieldB            string // engagement field for correlation (e.g. "watch_time")
	EngagementSourceType string // event_type for engagement metric
	// Percentile metric fields
	Percentile float64 // percentile value in (0,1), e.g. 0.50 for p50, 0.95 for p95
	// Custom metric fields
	CustomSQL string // user-provided Spark SQL expression for CUSTOM metrics
	// Provider-side metric fields (ADR-014)
	LongtailThreshold float64 // PERCENT_RANK threshold for longtail classification (e.g. 0.80)
	ProviderField     string  // provider column name in content_catalog (default "provider_id")
	GenreField        string  // genre column name in content_catalog (default "genre")
}

type SQLRenderer struct {
	templates *template.Template
}

func NewSQLRenderer() (*SQLRenderer, error) {
	tmpl, err := template.ParseFS(templateFS, "templates/*.sql.tmpl")
	if err != nil {
		return nil, fmt.Errorf("spark: parse templates: %w", err)
	}
	return &SQLRenderer{templates: tmpl}, nil
}

func (r *SQLRenderer) Render(templateName string, p TemplateParams) (string, error) {
	var buf bytes.Buffer
	if err := r.templates.ExecuteTemplate(&buf, templateName, p); err != nil {
		return "", fmt.Errorf("spark: render %s: %w", templateName, err)
	}
	return strings.TrimSpace(buf.String()), nil
}

func (r *SQLRenderer) RenderMean(p TemplateParams) (string, error)            { return r.Render("mean.sql.tmpl", p) }
func (r *SQLRenderer) RenderProportion(p TemplateParams) (string, error)      { return r.Render("proportion.sql.tmpl", p) }
func (r *SQLRenderer) RenderCount(p TemplateParams) (string, error)           { return r.Render("count.sql.tmpl", p) }
func (r *SQLRenderer) RenderRatio(p TemplateParams) (string, error)           { return r.Render("ratio.sql.tmpl", p) }
func (r *SQLRenderer) RenderRatioDeltaMethod(p TemplateParams) (string, error) { return r.Render("ratio_delta_method.sql.tmpl", p) }
func (r *SQLRenderer) RenderCupedCovariate(p TemplateParams) (string, error)  { return r.Render("cuped_covariate.sql.tmpl", p) }
func (r *SQLRenderer) RenderGuardrailMetric(p TemplateParams) (string, error) { return r.Render("guardrail_metric.sql.tmpl", p) }
func (r *SQLRenderer) RenderQoEMetric(p TemplateParams) (string, error)      { return r.Render("qoe_metric.sql.tmpl", p) }
func (r *SQLRenderer) RenderContentConsumption(p TemplateParams) (string, error) { return r.Render("content_consumption.sql.tmpl", p) }
func (r *SQLRenderer) RenderDailyTreatmentEffect(p TemplateParams) (string, error) { return r.Render("daily_treatment_effect.sql.tmpl", p) }
func (r *SQLRenderer) RenderLifecycleMean(p TemplateParams) (string, error)  { return r.Render("lifecycle_mean.sql.tmpl", p) }
func (r *SQLRenderer) RenderSurrogateInput(p TemplateParams) (string, error) { return r.Render("surrogate_input.sql.tmpl", p) }
func (r *SQLRenderer) RenderInterleavingScore(p TemplateParams) (string, error) { return r.Render("interleaving_score.sql.tmpl", p) }
func (r *SQLRenderer) RenderSessionLevelMean(p TemplateParams) (string, error) { return r.Render("session_level_mean.sql.tmpl", p) }
func (r *SQLRenderer) RenderQoEEngagementCorrelation(p TemplateParams) (string, error) { return r.Render("qoe_engagement_correlation.sql.tmpl", p) }
func (r *SQLRenderer) RenderCustom(p TemplateParams) (string, error)               { return r.Render("custom.sql.tmpl", p) }
func (r *SQLRenderer) RenderPercentile(p TemplateParams) (string, error)           { return r.Render("percentile.sql.tmpl", p) }

// Provider-side metric renderers (ADR-014).
// Experiment-level metrics — results go to delta.experiment_level_metrics.
func (r *SQLRenderer) RenderCatalogCoverageRate(p TemplateParams) (string, error)    { return r.Render("catalog_coverage_rate.sql.tmpl", p) }
func (r *SQLRenderer) RenderCatalogGiniCoefficient(p TemplateParams) (string, error) { return r.Render("catalog_gini_coefficient.sql.tmpl", p) }
func (r *SQLRenderer) RenderCatalogEntropy(p TemplateParams) (string, error)         { return r.Render("catalog_entropy.sql.tmpl", p) }
func (r *SQLRenderer) RenderLongtailImpressionShare(p TemplateParams) (string, error) { return r.Render("longtail_impression_share.sql.tmpl", p) }
func (r *SQLRenderer) RenderProviderExposureGini(p TemplateParams) (string, error)   { return r.Render("provider_exposure_gini.sql.tmpl", p) }
func (r *SQLRenderer) RenderProviderExposureParity(p TemplateParams) (string, error) { return r.Render("provider_exposure_parity.sql.tmpl", p) }

// User-level provider metrics — results go to delta.metric_summaries.
func (r *SQLRenderer) RenderUserGenreEntropy(p TemplateParams) (string, error)      { return r.Render("user_genre_entropy.sql.tmpl", p) }
func (r *SQLRenderer) RenderUserDiscoveryRate(p TemplateParams) (string, error)     { return r.Render("user_discovery_rate.sql.tmpl", p) }
func (r *SQLRenderer) RenderUserProviderDiversity(p TemplateParams) (string, error) { return r.Render("user_provider_diversity.sql.tmpl", p) }
func (r *SQLRenderer) RenderIntraListDistance(p TemplateParams) (string, error)     { return r.Render("intra_list_distance.sql.tmpl", p) }

func (r *SQLRenderer) RenderForType(metricType string, p TemplateParams) (string, error) {
	switch strings.ToUpper(metricType) {
	case "MEAN":
		return r.RenderMean(p)
	case "PROPORTION":
		return r.RenderProportion(p)
	case "COUNT":
		return r.RenderCount(p)
	case "RATIO":
		return r.RenderRatio(p)
	case "PERCENTILE":
		if p.Percentile <= 0 || p.Percentile >= 1 {
			return "", fmt.Errorf("spark: PERCENTILE metric %q requires percentile in (0,1), got %g", p.MetricID, p.Percentile)
		}
		return r.RenderPercentile(p)
	case "CUSTOM":
		if p.CustomSQL == "" {
			return "", fmt.Errorf("spark: CUSTOM metric %q requires non-empty custom_sql", p.MetricID)
		}
		if err := ValidateCustomSQL(p.CustomSQL); err != nil {
			return "", fmt.Errorf("spark: CUSTOM metric %q: %w", p.MetricID, err)
		}
		return r.RenderCustom(p)
	default:
		return "", fmt.Errorf("spark: unsupported metric type %q (supported: MEAN, PROPORTION, COUNT, RATIO, PERCENTILE, CUSTOM)", metricType)
	}
}
