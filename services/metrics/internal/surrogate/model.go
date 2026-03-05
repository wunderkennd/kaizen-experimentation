// Package surrogate provides model loading and prediction for surrogate metrics.
// A surrogate model projects long-horizon outcomes (e.g., 90-day churn) from
// short-term input signals (e.g., 7-day watch time, session frequency).
package surrogate

import (
	"fmt"
	"math"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
)

// InputMetrics maps metric_id → per-variant mean value.
// Outer key is variant_id, inner key is metric_id.
type InputMetrics map[string]map[string]float64

// Projection is the result of applying a surrogate model to input metrics.
type Projection struct {
	VariantID           string
	ProjectedEffect     float64 // treatment effect on target metric
	ProjectionCILower   float64
	ProjectionCIUpper   float64
	CalibrationRSquared float64
}

// Model predicts long-term effects from short-term input metrics.
type Model interface {
	// Predict computes projected treatment effects for each variant relative to control.
	Predict(inputs InputMetrics, controlVariantID string) ([]Projection, error)
}

// ModelLoader loads a trained model from a registry (MLflow, local mock, etc.).
type ModelLoader interface {
	Load(cfg *config.SurrogateModelConfig) (Model, error)
}

// LinearModel is a simple linear surrogate: y = intercept + sum(coeff_i * x_i).
// Used for testing and as a baseline before real MLflow models are available.
type LinearModel struct {
	Coefficients        map[string]float64
	Intercept           float64
	CalibrationRSquared float64
}

// Predict computes the treatment effect for each non-control variant by applying
// the linear model to both control and treatment inputs and taking the difference.
func (m *LinearModel) Predict(inputs InputMetrics, controlVariantID string) ([]Projection, error) {
	controlInputs, ok := inputs[controlVariantID]
	if !ok {
		return nil, fmt.Errorf("surrogate: control variant %q not found in inputs", controlVariantID)
	}

	controlPrediction := m.predict(controlInputs)

	var projections []Projection
	for variantID, variantInputs := range inputs {
		if variantID == controlVariantID {
			continue
		}

		treatmentPrediction := m.predict(variantInputs)
		effect := treatmentPrediction - controlPrediction

		// Analytic CI approximation: use R² to scale uncertainty.
		// Higher R² → tighter CI. SE ≈ |effect| * sqrt((1 - R²) / R²) when R² > 0.
		se := m.estimateSE(effect)

		projections = append(projections, Projection{
			VariantID:           variantID,
			ProjectedEffect:     effect,
			ProjectionCILower:   effect - 1.96*se,
			ProjectionCIUpper:   effect + 1.96*se,
			CalibrationRSquared: m.CalibrationRSquared,
		})
	}

	return projections, nil
}

func (m *LinearModel) predict(metricValues map[string]float64) float64 {
	y := m.Intercept
	for metricID, coeff := range m.Coefficients {
		if val, ok := metricValues[metricID]; ok {
			y += coeff * val
		}
	}
	return y
}

func (m *LinearModel) estimateSE(effect float64) float64 {
	if m.CalibrationRSquared <= 0 || m.CalibrationRSquared >= 1 {
		return math.Abs(effect) * 0.5 // fallback: 50% relative SE
	}
	return math.Abs(effect) * math.Sqrt((1-m.CalibrationRSquared)/m.CalibrationRSquared)
}

// MockModelLoader returns LinearModel instances from config coefficients.
type MockModelLoader struct{}

func NewMockModelLoader() *MockModelLoader {
	return &MockModelLoader{}
}

func (l *MockModelLoader) Load(cfg *config.SurrogateModelConfig) (Model, error) {
	if cfg.ModelType != "LINEAR" {
		return nil, fmt.Errorf("surrogate: mock loader only supports LINEAR models, got %q", cfg.ModelType)
	}
	if len(cfg.Coefficients) == 0 {
		return nil, fmt.Errorf("surrogate: LINEAR model %q has no coefficients", cfg.ModelID)
	}
	return &LinearModel{
		Coefficients:        cfg.Coefficients,
		Intercept:           cfg.Intercept,
		CalibrationRSquared: cfg.CalibrationRSquared,
	}, nil
}
