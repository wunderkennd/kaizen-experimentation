package config

import (
	"testing"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/encoding/protojson"
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
		assert.Equal(t, commonv1.MetricType_METRIC_TYPE_MEAN, m.Type)
	})

	t.Run("metrics for experiment", func(t *testing.T) {
		metrics, err := cs.GetMetricsForExperiment("e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Len(t, metrics, 4)
	})

	t.Run("ratio metric", func(t *testing.T) {
		m, err := cs.GetMetric("rebuffer_rate")
		require.NoError(t, err)
		assert.Equal(t, commonv1.MetricType_METRIC_TYPE_RATIO, m.Type)
		assert.Equal(t, "rebuffer_event", m.NumeratorEventType)
		assert.Equal(t, "playback_minute", m.DenominatorEventType)
	})

	t.Run("cuped covariate", func(t *testing.T) {
		m, err := cs.GetMetric("watch_time_minutes")
		require.NoError(t, err)
		assert.Equal(t, "watch_time_minutes", m.CupedCovariateMetricId)
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
		assert.Equal(t, commonv1.MetricType_METRIC_TYPE_PERCENTILE, m.Type)
		assert.Equal(t, 0.50, m.Percentile)
		assert.True(t, m.LowerIsBetter)
	})

	t.Run("qoe metric", func(t *testing.T) {
		m, err := cs.GetMetric("ttff_mean")
		require.NoError(t, err)
		assert.True(t, m.IsQoeMetric)
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

func TestMetricConfig_ADR026Phase1_RoundTrip(t *testing.T) {
	// Now that MetricConfig embeds *commonv1.MetricDefinition, raw protojson
	// drives the proto half; the four M3-only sibling fields are no longer
	// exercised here.  Tier-1 (FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT)
	// round-trip via protojson's oneof handling.
	protoOpts := protojson.UnmarshalOptions{DiscardUnknown: true}

	t.Run("filtered_mean", func(t *testing.T) {
		raw := []byte(`{
            "metric_id": "mobile_avg_watch_time",
            "name": "Mobile avg watch time",
            "type": "METRIC_TYPE_FILTERED_MEAN",
            "source_event_type": "heartbeat",
            "filteredMean": {
                "filter_sql": "platform = 'mobile'",
                "value_column": "duration_ms"
            }
        }`)
		md := &commonv1.MetricDefinition{}
		require.NoError(t, protoOpts.Unmarshal(raw, md))
		m := MetricConfig{MetricDefinition: md}
		assert.Equal(t, commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN, m.Type)
		assert.Equal(t, "platform = 'mobile'", m.GetFilteredMean().GetFilterSql())
		assert.Equal(t, "duration_ms", m.GetFilteredMean().GetValueColumn())
	})

	t.Run("composite", func(t *testing.T) {
		raw := []byte(`{
            "metric_id": "engagement_score",
            "name": "Composite engagement",
            "type": "METRIC_TYPE_COMPOSITE",
            "composite": {
                "operator": "COMPOSITE_OPERATOR_WEIGHTED_SUM",
                "operands": [
                    {"metric_id": "watch_time_minutes", "weight": 0.7},
                    {"metric_id": "stream_start_rate", "weight": 0.3}
                ]
            }
        }`)
		md := &commonv1.MetricDefinition{}
		require.NoError(t, protoOpts.Unmarshal(raw, md))
		m := MetricConfig{MetricDefinition: md}
		assert.Equal(t, commonv1.MetricType_METRIC_TYPE_COMPOSITE, m.Type)
		assert.Equal(t, commonv1.CompositeOperator_COMPOSITE_OPERATOR_WEIGHTED_SUM, m.GetComposite().GetOperator())
		operands := m.GetComposite().GetOperands()
		require.Len(t, operands, 2)
		assert.Equal(t, "watch_time_minutes", operands[0].GetMetricId())
		assert.InDelta(t, 0.7, operands[0].GetWeight(), 1e-9)
		assert.Equal(t, "stream_start_rate", operands[1].GetMetricId())
		assert.InDelta(t, 0.3, operands[1].GetWeight(), 1e-9)
	})

	t.Run("windowed_count", func(t *testing.T) {
		raw := []byte(`{
            "metric_id": "stream_starts_24h",
            "name": "Stream starts within 24h",
            "type": "METRIC_TYPE_WINDOWED_COUNT",
            "windowedCount": {
                "event_type": "stream_start",
                "window_hours": 24
            }
        }`)
		md := &commonv1.MetricDefinition{}
		require.NoError(t, protoOpts.Unmarshal(raw, md))
		m := MetricConfig{MetricDefinition: md}
		assert.Equal(t, commonv1.MetricType_METRIC_TYPE_WINDOWED_COUNT, m.Type)
		assert.Equal(t, "stream_start", m.GetWindowedCount().GetEventType())
		assert.Equal(t, int32(24), m.GetWindowedCount().GetWindowHours())
	})

	t.Run("omitempty preserves backward compatibility", func(t *testing.T) {
		// A pre-ADR-026 metric (e.g. MEAN) must still unmarshal cleanly with
		// no JSON keys for the new fields.
		raw := []byte(`{
            "metric_id": "watch_time_minutes",
            "name": "Watch time",
            "type": "METRIC_TYPE_MEAN",
            "source_event_type": "heartbeat"
        }`)
		md := &commonv1.MetricDefinition{}
		require.NoError(t, protoOpts.Unmarshal(raw, md))
		m := MetricConfig{MetricDefinition: md}
		assert.Nil(t, m.GetFilteredMean())
		assert.Nil(t, m.GetComposite())
		assert.Nil(t, m.GetWindowedCount())
	})
}

func TestCompositeOperatorShortName(t *testing.T) {
	cases := []struct {
		name string
		op   commonv1.CompositeOperator
		want string
	}{
		{"unspecified is empty", commonv1.CompositeOperator_COMPOSITE_OPERATOR_UNSPECIFIED, ""},
		{"add", commonv1.CompositeOperator_COMPOSITE_OPERATOR_ADD, "ADD"},
		{"subtract", commonv1.CompositeOperator_COMPOSITE_OPERATOR_SUBTRACT, "SUBTRACT"},
		{"multiply", commonv1.CompositeOperator_COMPOSITE_OPERATOR_MULTIPLY, "MULTIPLY"},
		{"divide", commonv1.CompositeOperator_COMPOSITE_OPERATOR_DIVIDE, "DIVIDE"},
		{"weighted_sum", commonv1.CompositeOperator_COMPOSITE_OPERATOR_WEIGHTED_SUM, "WEIGHTED_SUM"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			assert.Equal(t, tc.want, CompositeOperatorShortName(tc.op))
		})
	}
}
