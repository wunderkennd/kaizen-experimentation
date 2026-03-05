package guardrail

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// TestAlertJSONDeserialization verifies that Alert can parse JSON from Agent-3's publisher.
func TestAlertJSONDeserialization(t *testing.T) {
	// This matches the JSON format produced by Agent-3's KafkaPublisher.
	raw := `{
		"experiment_id": "abc-123",
		"metric_id": "error_rate",
		"variant_id": "treatment-1",
		"current_value": 0.015,
		"threshold": 0.01,
		"consecutive_breach_count": 3,
		"detected_at": "2026-03-04T12:00:00Z"
	}`

	var alert Alert
	err := json.Unmarshal([]byte(raw), &alert)
	require.NoError(t, err)

	assert.Equal(t, "abc-123", alert.ExperimentID)
	assert.Equal(t, "error_rate", alert.MetricID)
	assert.Equal(t, "treatment-1", alert.VariantID)
	assert.InDelta(t, 0.015, alert.CurrentValue, 1e-9)
	assert.InDelta(t, 0.01, alert.Threshold, 1e-9)
	assert.Equal(t, 3, alert.ConsecutiveBreachCount)
	assert.Equal(t, time.Date(2026, 3, 4, 12, 0, 0, 0, time.UTC), alert.DetectedAt)
}

func TestAlertJSONRoundTrip(t *testing.T) {
	alert := Alert{
		ExperimentID:           "exp-456",
		MetricID:               "rebuffer_rate",
		VariantID:              "variant-7",
		CurrentValue:           0.08,
		Threshold:              0.05,
		ConsecutiveBreachCount: 5,
		DetectedAt:             time.Date(2026, 3, 4, 15, 30, 0, 0, time.UTC),
	}

	data, err := json.Marshal(alert)
	require.NoError(t, err)

	var decoded Alert
	err = json.Unmarshal(data, &decoded)
	require.NoError(t, err)
	assert.Equal(t, alert, decoded)
}

func TestResultString(t *testing.T) {
	assert.Equal(t, "skipped", resultString(ResultSkipped))
	assert.Equal(t, "alert_only", resultString(ResultAlertOnly))
	assert.Equal(t, "paused", resultString(ResultPaused))
	assert.Equal(t, "unknown", resultString(ProcessResult(99)))
}
