package mlrate_test

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/management/internal/mlrate"
)

// ---------------------------------------------------------------------------
// MemPublisher tests
// ---------------------------------------------------------------------------

func TestMemPublisher_Publish(t *testing.T) {
	pub := mlrate.NewMemPublisher()

	req := mlrate.ModelTrainingRequest{
		ExperimentID:      "exp-001",
		MetricID:          "play_start_rate",
		CovariateMetricID: "7d_play_rate",
		TrainingDataStart: "2026-02-22T00:00:00Z",
		TrainingDataEnd:   "2026-03-24T00:00:00Z",
	}

	err := pub.Publish(context.Background(), req)
	require.NoError(t, err)

	reqs := pub.Requests()
	require.Len(t, reqs, 1)
	assert.Equal(t, "exp-001", reqs[0].ExperimentID)
	assert.Equal(t, "play_start_rate", reqs[0].MetricID)
	assert.Equal(t, "7d_play_rate", reqs[0].CovariateMetricID)
}

func TestMemPublisher_Reset(t *testing.T) {
	pub := mlrate.NewMemPublisher()

	require.NoError(t, pub.Publish(context.Background(), mlrate.ModelTrainingRequest{ExperimentID: "exp-1"}))
	require.Len(t, pub.Requests(), 1)

	pub.Reset()
	assert.Empty(t, pub.Requests())
}

// ---------------------------------------------------------------------------
// JSON round-trip
// ---------------------------------------------------------------------------

func TestModelTrainingRequest_JSONRoundTrip(t *testing.T) {
	req := mlrate.ModelTrainingRequest{
		ExperimentID:      "exp-abc",
		MetricID:          "play_start_rate",
		CovariateMetricID: "7d_play_rate",
		TrainingDataStart: "2026-02-22T00:00:00Z",
		TrainingDataEnd:   "2026-03-24T00:00:00Z",
	}

	data, err := json.Marshal(req)
	require.NoError(t, err)

	// Verify snake_case JSON field names.
	raw := make(map[string]json.RawMessage)
	require.NoError(t, json.Unmarshal(data, &raw))

	for _, key := range []string{"experiment_id", "metric_id", "covariate_metric_id", "training_data_start", "training_data_end"} {
		assert.Contains(t, raw, key, "missing expected JSON key: %s", key)
	}

	var decoded mlrate.ModelTrainingRequest
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.Equal(t, req, decoded)
}

// ---------------------------------------------------------------------------
// ShouldTrigger — AVLM+surrogate triggers; others do not
// ---------------------------------------------------------------------------

func TestShouldTrigger(t *testing.T) {
	cases := []struct {
		name             string
		sequentialMethod string
		surrogateModelID string
		wantTrigger      bool
	}{
		{
			name:             "AVLM + surrogate configured → triggers",
			sequentialMethod: "AVLM",
			surrogateModelID: "model-123",
			wantTrigger:      true,
		},
		{
			name:             "MSPRT + surrogate → no trigger",
			sequentialMethod: "MSPRT",
			surrogateModelID: "model-123",
			wantTrigger:      false,
		},
		{
			name:             "GST_OBF + surrogate → no trigger",
			sequentialMethod: "GST_OBF",
			surrogateModelID: "model-123",
			wantTrigger:      false,
		},
		{
			name:             "GST_POCOCK + surrogate → no trigger",
			sequentialMethod: "GST_POCOCK",
			surrogateModelID: "model-123",
			wantTrigger:      false,
		},
		{
			name:             "AVLM + no surrogate → no trigger",
			sequentialMethod: "AVLM",
			surrogateModelID: "",
			wantTrigger:      false,
		},
		{
			name:             "empty method → no trigger",
			sequentialMethod: "",
			surrogateModelID: "model-123",
			wantTrigger:      false,
		},
		{
			name:             "both empty → no trigger",
			sequentialMethod: "",
			surrogateModelID: "",
			wantTrigger:      false,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := mlrate.ShouldTrigger(tc.sequentialMethod, tc.surrogateModelID)
			assert.Equal(t, tc.wantTrigger, got)
		})
	}
}

// ---------------------------------------------------------------------------
// Emit — end-to-end trigger + publish
// ---------------------------------------------------------------------------

func TestEmit_AVLMWithSurrogate_PublishesEvent(t *testing.T) {
	pub := mlrate.NewMemPublisher()
	now := time.Date(2026, 3, 24, 12, 0, 0, 0, time.UTC)

	emitted := mlrate.Emit(
		context.Background(), pub,
		"exp-avlm-1", "AVLM", "model-456",
		"play_start_rate", "7d_play_rate",
		now,
	)

	assert.True(t, emitted, "expected Emit to return true for AVLM+surrogate")

	reqs := pub.Requests()
	require.Len(t, reqs, 1)
	assert.Equal(t, "exp-avlm-1", reqs[0].ExperimentID)
	assert.Equal(t, "play_start_rate", reqs[0].MetricID)
	assert.Equal(t, "7d_play_rate", reqs[0].CovariateMetricID)
	// training_data_end == now
	assert.Equal(t, "2026-03-24T12:00:00Z", reqs[0].TrainingDataEnd)
	// training_data_start == now - 30 days
	assert.Equal(t, "2026-02-22T12:00:00Z", reqs[0].TrainingDataStart)
}

func TestEmit_NonAVLM_DoesNotPublish(t *testing.T) {
	for _, method := range []string{"MSPRT", "GST_OBF", "GST_POCOCK", ""} {
		t.Run(method, func(t *testing.T) {
			pub := mlrate.NewMemPublisher()

			emitted := mlrate.Emit(
				context.Background(), pub,
				"exp-1", method, "model-789",
				"play_start_rate", "7d_play_rate",
				time.Now(),
			)

			assert.False(t, emitted, "Emit should return false for method=%q", method)
			assert.Empty(t, pub.Requests())
		})
	}
}

func TestEmit_AVLMWithoutSurrogate_DoesNotPublish(t *testing.T) {
	pub := mlrate.NewMemPublisher()

	emitted := mlrate.Emit(
		context.Background(), pub,
		"exp-1", "AVLM", "", // no surrogate
		"play_start_rate", "",
		time.Now(),
	)

	assert.False(t, emitted)
	assert.Empty(t, pub.Requests())
}
