package surrogate

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
)

func TestLinearModel_Predict(t *testing.T) {
	model := &LinearModel{
		Coefficients: map[string]float64{
			"watch_time_minutes": -0.015,
			"stream_start_rate":  -0.25,
		},
		Intercept:           0.35,
		CalibrationRSquared: 0.72,
	}

	inputs := InputMetrics{
		"control": {
			"watch_time_minutes": 10.0,
			"stream_start_rate":  0.80,
		},
		"treatment": {
			"watch_time_minutes": 12.0, // higher watch time
			"stream_start_rate":  0.85, // higher stream start
		},
	}

	projections, err := model.Predict(inputs, "control")
	require.NoError(t, err)
	require.Len(t, projections, 1)

	p := projections[0]
	assert.Equal(t, "treatment", p.VariantID)

	// Control: 0.35 + (-0.015 * 10) + (-0.25 * 0.80) = 0.35 - 0.15 - 0.20 = 0.00
	// Treatment: 0.35 + (-0.015 * 12) + (-0.25 * 0.85) = 0.35 - 0.18 - 0.2125 = -0.0425
	// Effect: -0.0425 - 0.00 = -0.0425
	assert.InDelta(t, -0.0425, p.ProjectedEffect, 1e-6)
	assert.Equal(t, 0.72, p.CalibrationRSquared)
	assert.True(t, p.ProjectionCILower < p.ProjectedEffect)
	assert.True(t, p.ProjectionCIUpper > p.ProjectedEffect)
}

func TestLinearModel_Predict_MultipleVariants(t *testing.T) {
	model := &LinearModel{
		Coefficients:        map[string]float64{"metric_a": 1.0},
		Intercept:           0.0,
		CalibrationRSquared: 0.80,
	}

	inputs := InputMetrics{
		"control":     {"metric_a": 5.0},
		"treatment_a": {"metric_a": 7.0},
		"treatment_b": {"metric_a": 3.0},
	}

	projections, err := model.Predict(inputs, "control")
	require.NoError(t, err)
	require.Len(t, projections, 2)

	effectMap := make(map[string]float64)
	for _, p := range projections {
		effectMap[p.VariantID] = p.ProjectedEffect
	}

	assert.InDelta(t, 2.0, effectMap["treatment_a"], 1e-6)  // 7 - 5
	assert.InDelta(t, -2.0, effectMap["treatment_b"], 1e-6) // 3 - 5
}

func TestLinearModel_Predict_ControlNotFound(t *testing.T) {
	model := &LinearModel{
		Coefficients:        map[string]float64{"x": 1.0},
		CalibrationRSquared: 0.8,
	}
	inputs := InputMetrics{"treatment": {"x": 5.0}}
	_, err := model.Predict(inputs, "control")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "control variant")
}

func TestMockModelLoader_Load(t *testing.T) {
	loader := NewMockModelLoader()

	cfg := &config.SurrogateModelConfig{
		ModelID:             "test-model",
		ModelType:           "LINEAR",
		CalibrationRSquared: 0.75,
		Coefficients:        map[string]float64{"a": 1.0, "b": 2.0},
		Intercept:           0.5,
	}

	model, err := loader.Load(cfg)
	require.NoError(t, err)

	inputs := InputMetrics{
		"ctrl": {"a": 1.0, "b": 1.0},
		"tx":   {"a": 2.0, "b": 2.0},
	}

	projections, err := model.Predict(inputs, "ctrl")
	require.NoError(t, err)
	require.Len(t, projections, 1)
	// ctrl: 0.5 + 1*1 + 2*1 = 3.5, tx: 0.5 + 1*2 + 2*2 = 6.5, effect: 3.0
	assert.InDelta(t, 3.0, projections[0].ProjectedEffect, 1e-6)
}

func TestMockModelLoader_Load_NonLinear(t *testing.T) {
	loader := NewMockModelLoader()
	cfg := &config.SurrogateModelConfig{
		ModelID:   "nn-model",
		ModelType: "NEURAL",
	}
	_, err := loader.Load(cfg)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "LINEAR")
}
