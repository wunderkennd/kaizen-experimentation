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
	// Custom metric fields
	CustomSQL string // user-provided Spark SQL expression for CUSTOM metrics
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
	case "CUSTOM":
		if p.CustomSQL == "" {
			return "", fmt.Errorf("spark: CUSTOM metric %q requires non-empty custom_sql", p.MetricID)
		}
		if err := ValidateCustomSQL(p.CustomSQL); err != nil {
			return "", fmt.Errorf("spark: CUSTOM metric %q: %w", p.MetricID, err)
		}
		return r.RenderCustom(p)
	default:
		return "", fmt.Errorf("spark: unsupported metric type %q (supported: MEAN, PROPORTION, COUNT, RATIO, CUSTOM)", metricType)
	}
}
