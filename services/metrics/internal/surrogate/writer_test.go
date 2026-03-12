package surrogate

import (
	"context"
	"sync"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
)

func TestMemProjectionWriter_Write(t *testing.T) {
	w := NewMemProjectionWriter()
	err := w.Write(context.Background(), ProjectionRecord{
		ExperimentID:        "exp-001",
		VariantID:           "treatment",
		ModelID:             "model-001",
		ProjectedEffect:     -0.05,
		ProjectionCILower:   -0.08,
		ProjectionCIUpper:   -0.02,
		CalibrationRSquared: 0.72,
		ComputedAt:          time.Now(),
	})
	require.NoError(t, err)

	records := w.AllRecords()
	require.Len(t, records, 1)
	assert.Equal(t, "exp-001", records[0].ExperimentID)
	assert.Equal(t, "treatment", records[0].VariantID)
	assert.Equal(t, -0.05, records[0].ProjectedEffect)
}

func TestMemProjectionWriter_MultipleRecords(t *testing.T) {
	w := NewMemProjectionWriter()
	for i := 0; i < 10; i++ {
		_ = w.Write(context.Background(), ProjectionRecord{
			ExperimentID: "exp-001",
			VariantID:    "treatment",
			ModelID:      "model-001",
		})
	}
	assert.Len(t, w.AllRecords(), 10)
}

func TestMemProjectionWriter_AllRecords_ReturnsCopy(t *testing.T) {
	w := NewMemProjectionWriter()
	_ = w.Write(context.Background(), ProjectionRecord{ExperimentID: "exp-001"})

	records1 := w.AllRecords()
	records1[0].ExperimentID = "MODIFIED"
	assert.Equal(t, "exp-001", w.AllRecords()[0].ExperimentID)
}

func TestMemProjectionWriter_AllRecords_Empty(t *testing.T) {
	w := NewMemProjectionWriter()
	assert.Empty(t, w.AllRecords())
}

func TestMemProjectionWriter_ConcurrentAccess(t *testing.T) {
	w := NewMemProjectionWriter()
	var wg sync.WaitGroup
	const goroutines = 50

	wg.Add(goroutines)
	for i := 0; i < goroutines; i++ {
		go func() {
			defer wg.Done()
			_ = w.Write(context.Background(), ProjectionRecord{ExperimentID: "exp-001"})
			_ = w.AllRecords()
		}()
	}
	wg.Wait()
	assert.Len(t, w.AllRecords(), goroutines)
}

func TestMemProjectionWriter_ReadForExperiment(t *testing.T) {
	w := NewMemProjectionWriter()
	ctx := context.Background()

	_ = w.Write(ctx, ProjectionRecord{ExperimentID: "exp-A", VariantID: "tx1", ModelID: "m1"})
	_ = w.Write(ctx, ProjectionRecord{ExperimentID: "exp-A", VariantID: "tx2", ModelID: "m1"})
	_ = w.Write(ctx, ProjectionRecord{ExperimentID: "exp-B", VariantID: "tx1", ModelID: "m2"})

	records, err := w.ReadForExperiment(ctx, "exp-A")
	require.NoError(t, err)
	require.Len(t, records, 2)
	for _, r := range records {
		assert.Equal(t, "exp-A", r.ExperimentID)
	}
}

func TestMemProjectionWriter_ReadForExperiment_NoMatch(t *testing.T) {
	w := NewMemProjectionWriter()
	ctx := context.Background()

	_ = w.Write(ctx, ProjectionRecord{ExperimentID: "exp-A", VariantID: "tx1"})

	records, err := w.ReadForExperiment(ctx, "nonexistent")
	require.NoError(t, err)
	assert.Empty(t, records)
}

func TestMemProjectionWriter_Reset(t *testing.T) {
	w := NewMemProjectionWriter()
	ctx := context.Background()

	_ = w.Write(ctx, ProjectionRecord{ExperimentID: "exp-A"})
	_ = w.Write(ctx, ProjectionRecord{ExperimentID: "exp-B"})
	require.Len(t, w.AllRecords(), 2)

	w.Reset()
	assert.Empty(t, w.AllRecords(), "Reset should clear all records")
}

func TestMemCalibrationUpdater_UpdateCalibration(t *testing.T) {
	u := NewMemCalibrationUpdater()
	ctx := context.Background()

	err := u.UpdateCalibration(ctx, "m1", 0.85)
	require.NoError(t, err)
	assert.Equal(t, 0.85, u.Updates["m1"])
}

func TestMemCalibrationUpdater_UpdateCalibration_Overwrite(t *testing.T) {
	u := NewMemCalibrationUpdater()
	ctx := context.Background()

	_ = u.UpdateCalibration(ctx, "m1", 0.70)
	_ = u.UpdateCalibration(ctx, "m1", 0.85)
	assert.Equal(t, 0.85, u.Updates["m1"], "latest value should win")
}

func TestMemCalibrationUpdater_Reset(t *testing.T) {
	u := NewMemCalibrationUpdater()
	ctx := context.Background()

	_ = u.UpdateCalibration(ctx, "m1", 0.85)
	_ = u.UpdateCalibration(ctx, "m2", 0.72)
	require.Len(t, u.Updates, 2)

	u.Reset()
	assert.Empty(t, u.Updates, "Reset should clear all updates")
}

func TestLinearModel_EstimateSE_EdgeCases(t *testing.T) {
	t.Run("R²=0 uses fallback", func(t *testing.T) {
		model := &LinearModel{
			Coefficients:        map[string]float64{"x": 1.0},
			CalibrationRSquared: 0.0,
		}
		inputs := InputMetrics{
			"ctrl": {"x": 5.0},
			"tx":   {"x": 10.0},
		}
		projections, err := model.Predict(inputs, "ctrl")
		require.NoError(t, err)
		require.Len(t, projections, 1)
		// Effect = 5.0, fallback SE = |5.0| * 0.5 = 2.5
		assert.InDelta(t, 5.0-1.96*2.5, projections[0].ProjectionCILower, 1e-6)
		assert.InDelta(t, 5.0+1.96*2.5, projections[0].ProjectionCIUpper, 1e-6)
	})

	t.Run("R²=1 uses fallback", func(t *testing.T) {
		model := &LinearModel{
			Coefficients:        map[string]float64{"x": 1.0},
			CalibrationRSquared: 1.0,
		}
		inputs := InputMetrics{
			"ctrl": {"x": 5.0},
			"tx":   {"x": 10.0},
		}
		projections, err := model.Predict(inputs, "ctrl")
		require.NoError(t, err)
		require.Len(t, projections, 1)
		// Effect = 5.0, fallback SE = |5.0| * 0.5 = 2.5
		assert.InDelta(t, 5.0-1.96*2.5, projections[0].ProjectionCILower, 1e-6)
	})

	t.Run("negative R² uses fallback", func(t *testing.T) {
		model := &LinearModel{
			Coefficients:        map[string]float64{"x": 1.0},
			CalibrationRSquared: -0.5,
		}
		inputs := InputMetrics{
			"ctrl": {"x": 5.0},
			"tx":   {"x": 10.0},
		}
		projections, err := model.Predict(inputs, "ctrl")
		require.NoError(t, err)
		require.Len(t, projections, 1)
		// Fallback SE
		assert.InDelta(t, 5.0-1.96*2.5, projections[0].ProjectionCILower, 1e-6)
	})
}

func TestLinearModel_Predict_MissingInputMetric(t *testing.T) {
	model := &LinearModel{
		Coefficients:        map[string]float64{"x": 1.0, "y": 2.0},
		Intercept:           0.0,
		CalibrationRSquared: 0.8,
	}
	// "y" is missing from treatment — should still work (treated as 0 contribution)
	inputs := InputMetrics{
		"ctrl": {"x": 5.0, "y": 3.0},
		"tx":   {"x": 5.0}, // y missing
	}
	projections, err := model.Predict(inputs, "ctrl")
	require.NoError(t, err)
	require.Len(t, projections, 1)
	// ctrl = 1*5 + 2*3 = 11, tx = 1*5 + 0 = 5, effect = -6
	assert.InDelta(t, -6.0, projections[0].ProjectedEffect, 1e-6)
}

func TestMockModelLoader_Load_NoCoefficients(t *testing.T) {
	loader := NewMockModelLoader()
	cfg := &config.SurrogateModelConfig{
		ModelID:   "empty-model",
		ModelType: "LINEAR",
	}
	_, err := loader.Load(cfg)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "no coefficients")
}
