// Package spark provides SQL template rendering and execution for Spark SQL jobs.
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

// TemplateParams holds the values substituted into SQL templates.
type TemplateParams struct {
	ExperimentID    string
	MetricID        string
	SourceEventType string
	ComputationDate string // YYYY-MM-DD
	// NumeratorEventType and DenominatorEventType are used for RATIO metrics.
	NumeratorEventType   string
	DenominatorEventType string
}

// SQLRenderer renders Spark SQL from embedded templates.
type SQLRenderer struct {
	templates *template.Template
}

// NewSQLRenderer parses all embedded SQL templates and returns a renderer.
func NewSQLRenderer() (*SQLRenderer, error) {
	tmpl, err := template.ParseFS(templateFS, "templates/*.sql.tmpl")
	if err != nil {
		return nil, fmt.Errorf("spark: parse templates: %w", err)
	}
	return &SQLRenderer{templates: tmpl}, nil
}

// Render renders a named template (e.g., "mean.sql.tmpl") with the given params.
func (r *SQLRenderer) Render(templateName string, p TemplateParams) (string, error) {
	var buf bytes.Buffer
	if err := r.templates.ExecuteTemplate(&buf, templateName, p); err != nil {
		return "", fmt.Errorf("spark: render %s: %w", templateName, err)
	}
	return strings.TrimSpace(buf.String()), nil
}

// RenderMean renders the MEAN metric SQL template.
func (r *SQLRenderer) RenderMean(p TemplateParams) (string, error) {
	return r.Render("mean.sql.tmpl", p)
}

// RenderProportion renders the PROPORTION metric SQL template.
func (r *SQLRenderer) RenderProportion(p TemplateParams) (string, error) {
	return r.Render("proportion.sql.tmpl", p)
}

// RenderCount renders the COUNT metric SQL template.
func (r *SQLRenderer) RenderCount(p TemplateParams) (string, error) {
	return r.Render("count.sql.tmpl", p)
}

// RenderRatio renders the RATIO metric SQL template (per-user ratio value).
func (r *SQLRenderer) RenderRatio(p TemplateParams) (string, error) {
	return r.Render("ratio.sql.tmpl", p)
}

// RenderRatioDeltaMethod renders the delta method variance components SQL for RATIO metrics.
// This produces per-variant Var(N), Var(D), Cov(N,D) needed by M4a for delta method CI.
func (r *SQLRenderer) RenderRatioDeltaMethod(p TemplateParams) (string, error) {
	return r.Render("ratio_delta_method.sql.tmpl", p)
}

// RenderForType dispatches to the correct template based on metric type string.
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
	default:
		return "", fmt.Errorf("spark: unsupported metric type %q (supported: MEAN, PROPORTION, COUNT, RATIO)", metricType)
	}
}
