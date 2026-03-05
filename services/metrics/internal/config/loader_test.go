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
		assert.Equal(t, "ctr_recommendation", exp.PrimaryMetricID)
		assert.Len(t, exp.Variants, 2)
		assert.True(t, exp.Variants[0].IsControl)
	})

	t.Run("metrics loaded", func(t *testing.T) {
		m, err := cs.GetMetric("watch_time_minutes")
		require.NoError(t, err)
		assert.Equal(t, "MEAN", m.Type)
		assert.Equal(t, "heartbeat", m.SourceEventType)
	})

	t.Run("metrics for experiment", func(t *testing.T) {
		metrics, err := cs.GetMetricsForExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		// primary (ctr_recommendation) + secondary (watch_time_minutes, stream_start_rate, rebuffer_rate)
		assert.Len(t, metrics, 4)
		assert.Equal(t, "ctr_recommendation", metrics[0].MetricID)
	})

	t.Run("ratio metric has numerator and denominator", func(t *testing.T) {
		m, err := cs.GetMetric("rebuffer_rate")
		require.NoError(t, err)
		assert.Equal(t, "RATIO", m.Type)
		assert.Equal(t, "rebuffer_event", m.NumeratorEventType)
		assert.Equal(t, "playback_minute", m.DenominatorEventType)
	})

	t.Run("experiment has started_at", func(t *testing.T) {
		exp, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Equal(t, "2024-01-08", exp.StartedAt)
	})

	t.Run("metric has cuped_covariate_metric_id", func(t *testing.T) {
		m, err := cs.GetMetric("watch_time_minutes")
		require.NoError(t, err)
		assert.Equal(t, "watch_time_minutes", m.CupedCovariateMetricID)

		// stream_start_rate has no CUPED covariate
		m2, err := cs.GetMetric("stream_start_rate")
		require.NoError(t, err)
		assert.Empty(t, m2.CupedCovariateMetricID)
	})

	t.Run("running experiments", func(t *testing.T) {
		ids := cs.RunningExperimentIDs()
		assert.Len(t, ids, 2)
	})

	t.Run("not found errors", func(t *testing.T) {
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
	// Write a temporary invalid file.
	_, err := LoadFromFile("loader.go") // valid file, invalid JSON
	assert.Error(t, err)
}
