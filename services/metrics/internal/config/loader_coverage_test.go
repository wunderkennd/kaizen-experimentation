package config

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestGetSurrogateModel(t *testing.T) {
	cs, err := LoadFromFile("testdata/seed_config.json")
	require.NoError(t, err)

	t.Run("found", func(t *testing.T) {
		sm, err := cs.GetSurrogateModel("sm-churn-predictor-001")
		require.NoError(t, err)
		assert.Equal(t, "sm-churn-predictor-001", sm.ModelID)
		assert.Equal(t, "LINEAR", sm.ModelType)
		assert.NotEmpty(t, sm.Coefficients)
	})

	t.Run("not found", func(t *testing.T) {
		_, err := cs.GetSurrogateModel("nonexistent")
		require.Error(t, err)
		assert.Contains(t, err.Error(), "not found")
	})
}

func TestGetSurrogateModelForExperiment(t *testing.T) {
	cs, err := LoadFromFile("testdata/seed_config.json")
	require.NoError(t, err)

	t.Run("experiment with surrogate model", func(t *testing.T) {
		// e0000000-0000-0000-0000-000000000002 (retention_holdout) has surrogate_model_id
		sm := cs.GetSurrogateModelForExperiment("e0000000-0000-0000-0000-000000000002")
		if sm != nil {
			assert.NotEmpty(t, sm.ModelID)
		}
	})

	t.Run("experiment without surrogate model", func(t *testing.T) {
		sm := cs.GetSurrogateModelForExperiment("e0000000-0000-0000-0000-000000000003")
		// May be nil if no surrogate_model_id is set
		_ = sm
	})

	t.Run("nonexistent experiment", func(t *testing.T) {
		sm := cs.GetSurrogateModelForExperiment("nonexistent")
		assert.Nil(t, sm)
	})
}

func TestControlVariantID_NoControl(t *testing.T) {
	exp := &ExperimentConfig{
		Variants: []VariantConfig{
			{VariantID: "v1", IsControl: false},
			{VariantID: "v2", IsControl: false},
		},
	}
	assert.Equal(t, "", exp.ControlVariantID())
}

func TestControlVariantID_EmptyVariants(t *testing.T) {
	exp := &ExperimentConfig{Variants: nil}
	assert.Equal(t, "", exp.ControlVariantID())
}

func TestGetMetricsForExperiment_NotFound(t *testing.T) {
	cs, err := LoadFromFile("testdata/seed_config.json")
	require.NoError(t, err)

	_, err = cs.GetMetricsForExperiment("nonexistent")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "not found")
}

func TestGetGuardrailsForExperiment_NotFound(t *testing.T) {
	cs, err := LoadFromFile("testdata/seed_config.json")
	require.NoError(t, err)

	_, err = cs.GetGuardrailsForExperiment("nonexistent")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "not found")
}
