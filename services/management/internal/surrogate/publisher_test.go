package surrogate

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMemPublisher_Publish(t *testing.T) {
	pub := NewMemPublisher()

	req := RecalibrationRequest{
		ModelID:               "model-1",
		TargetMetricID:        "90_day_churn_rate",
		InputMetricIDs:        []string{"7d_watch_time", "7d_session_freq"},
		ModelType:             "LINEAR",
		ObservationWindowDays: 7,
		PredictionHorizonDays: 90,
		RequestedBy:           "alice@example.com",
		RequestedAt:           "2026-03-09T12:00:00Z",
	}

	err := pub.Publish(context.Background(), req)
	require.NoError(t, err)

	reqs := pub.Requests()
	require.Len(t, reqs, 1)
	assert.Equal(t, "model-1", reqs[0].ModelID)
	assert.Equal(t, "90_day_churn_rate", reqs[0].TargetMetricID)
	assert.Equal(t, []string{"7d_watch_time", "7d_session_freq"}, reqs[0].InputMetricIDs)
	assert.Equal(t, "LINEAR", reqs[0].ModelType)
	assert.Equal(t, int32(7), reqs[0].ObservationWindowDays)
	assert.Equal(t, int32(90), reqs[0].PredictionHorizonDays)
	assert.Equal(t, "alice@example.com", reqs[0].RequestedBy)
}

func TestMemPublisher_Reset(t *testing.T) {
	pub := NewMemPublisher()

	err := pub.Publish(context.Background(), RecalibrationRequest{ModelID: "m1"})
	require.NoError(t, err)
	require.Len(t, pub.Requests(), 1)

	pub.Reset()
	assert.Empty(t, pub.Requests())
}

func TestRecalibrationRequest_JSONRoundTrip(t *testing.T) {
	req := RecalibrationRequest{
		ModelID:               "model-abc",
		TargetMetricID:        "ltv_180d",
		InputMetricIDs:        []string{"7d_revenue", "14d_sessions"},
		ModelType:             "GRADIENT_BOOSTED",
		ObservationWindowDays: 14,
		PredictionHorizonDays: 180,
		RequestedBy:           "bob@example.com",
		RequestedAt:           "2026-03-09T15:30:00Z",
	}

	data, err := json.Marshal(req)
	require.NoError(t, err)

	// Verify snake_case JSON field names.
	raw := make(map[string]json.RawMessage)
	require.NoError(t, json.Unmarshal(data, &raw))
	expectedKeys := []string{
		"model_id", "target_metric_id", "input_metric_ids", "model_type",
		"observation_window_days", "prediction_horizon_days", "requested_by", "requested_at",
	}
	for _, key := range expectedKeys {
		assert.Contains(t, raw, key, "missing expected JSON key: %s", key)
	}

	var decoded RecalibrationRequest
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.Equal(t, req, decoded)
}
