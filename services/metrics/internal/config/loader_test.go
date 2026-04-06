package config

import (
	"testing"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestLoadFromFile(t *testing.T) {
	cs, err := LoadFromFile("testdata/seed_config.json")
	require.NoError(t, err)

	t.Run("experiments loaded", func(t *testing.T) {
		exp, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Equal(t, "homepage_recs_v2", exp.Name)
		assert.Equal(t, "RUNNING", exp.State)
		assert.Len(t, exp.Variants, 2)
	})

	t.Run("metrics loaded", func(t *testing.T) {
		m, err := cs.GetMetric("watch_time_minutes")
		require.NoError(t, err)
		assert.Equal(t, "MEAN", m.Type)
	})

	t.Run("metrics for experiment", func(t *testing.T) {
		metrics, err := cs.GetMetricsForExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Len(t, metrics, 4)
	})

	t.Run("ratio metric", func(t *testing.T) {
		m, err := cs.GetMetric("rebuffer_rate")
		require.NoError(t, err)
		assert.Equal(t, "RATIO", m.Type)
		assert.Equal(t, "rebuffer_event", m.NumeratorEventType)
		assert.Equal(t, "playback_minute", m.DenominatorEventType)
	})

	t.Run("cuped covariate", func(t *testing.T) {
		m, err := cs.GetMetric("watch_time_minutes")
		require.NoError(t, err)
		assert.Equal(t, "watch_time_minutes", m.CupedCovariateMetricID)
	})

	t.Run("guardrail configs", func(t *testing.T) {
		g, err := cs.GetGuardrailsForExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Len(t, g, 2)
		assert.Equal(t, "rebuffer_rate", g[0].MetricID)
		assert.Equal(t, 0.05, g[0].Threshold)
		assert.Equal(t, 3, g[0].ConsecutiveBreachesRequired)
		assert.Equal(t, "error_rate", g[1].MetricID)
		assert.Equal(t, 0.01, g[1].Threshold)
		assert.Equal(t, 2, g[1].ConsecutiveBreachesRequired)
	})

	t.Run("guardrail action", func(t *testing.T) {
		exp, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Equal(t, "AUTO_PAUSE", exp.GuardrailAction)
	})

	t.Run("no guardrails for search", func(t *testing.T) {
		g, err := cs.GetGuardrailsForExperiment("e0000000-0000-0000-0000-000000000003")
		require.NoError(t, err)
		assert.Len(t, g, 0)
	})

	t.Run("lower_is_better", func(t *testing.T) {
		m, _ := cs.GetMetric("rebuffer_rate")
		assert.True(t, m.LowerIsBetter)
		m2, _ := cs.GetMetric("error_rate")
		assert.True(t, m2.LowerIsBetter)
		m3, _ := cs.GetMetric("ctr_recommendation")
		assert.False(t, m3.LowerIsBetter)
	})

	t.Run("percentile metric", func(t *testing.T) {
		m, err := cs.GetMetric("latency_p50_ms")
		require.NoError(t, err)
		assert.Equal(t, "PERCENTILE", m.Type)
		assert.Equal(t, 0.50, m.Percentile)
		assert.True(t, m.LowerIsBetter)
	})

	t.Run("qoe metric", func(t *testing.T) {
		m, err := cs.GetMetric("ttff_mean")
		require.NoError(t, err)
		assert.True(t, m.IsQoEMetric)
		assert.Equal(t, "time_to_first_frame_ms", m.QoEField)
		assert.True(t, m.LowerIsBetter)
	})

	t.Run("lifecycle stratification", func(t *testing.T) {
		exp, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000004")
		require.NoError(t, err)
		assert.True(t, exp.LifecycleStratificationEnabled)
		assert.Len(t, exp.LifecycleSegments, 6)
		assert.Contains(t, exp.LifecycleSegments, "TRIAL")
		assert.Contains(t, exp.LifecycleSegments, "WINBACK")
	})

	t.Run("control variant id", func(t *testing.T) {
		exp, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Equal(t, "f0000000-0000-0000-0000-000000000001", exp.ControlVariantID())
	})

	t.Run("running experiments", func(t *testing.T) {
		ids := cs.RunningExperimentIDs()
		assert.Len(t, ids, 7)
	})

	t.Run("not found", func(t *testing.T) {
		_, err := cs.GetExperiment("nonexistent")
		assert.Error(t, err)
		_, err = cs.GetMetric("nonexistent")
		assert.Error(t, err)
	})
}

func TestLoadFromFile_InvalidPath(t *testing.T) {
	_, err := LoadFromFile("nonexistent.json")
	assert.Error(t, err)
}

func TestLoadFromFile_InvalidJSON(t *testing.T) {
	_, err := LoadFromFile("loader.go")
	assert.Error(t, err)
}
