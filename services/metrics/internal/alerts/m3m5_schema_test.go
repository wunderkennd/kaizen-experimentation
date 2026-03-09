package alerts

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// m5Alert mirrors M5's guardrail.Alert struct (management/internal/guardrail/processor.go).
// This is the "contract snapshot" — if M5 changes their struct, the Kafka contract
// breaks, and this test must be updated to match.
type m5Alert struct {
	ExperimentID           string    `json:"experiment_id"`
	MetricID               string    `json:"metric_id"`
	VariantID              string    `json:"variant_id"`
	CurrentValue           float64   `json:"current_value"`
	Threshold              float64   `json:"threshold"`
	ConsecutiveBreachCount int       `json:"consecutive_breach_count"`
	DetectedAt             time.Time `json:"detected_at"`
}

// TestM3M5_FieldSchemaSymmetry validates that M3's GuardrailAlert and M5's
// Alert have identical JSON field names. This catches field renames that
// would silently break the Kafka guardrail_alerts contract.
func TestM3M5_FieldSchemaSymmetry(t *testing.T) {
	m3 := GuardrailAlert{
		ExperimentID:           "exp-001",
		MetricID:               "m-001",
		VariantID:              "v-001",
		CurrentValue:           1.5,
		Threshold:              1.0,
		ConsecutiveBreachCount: 2,
		DetectedAt:             time.Now(),
	}
	m5 := m5Alert{
		ExperimentID:           "exp-001",
		MetricID:               "m-001",
		VariantID:              "v-001",
		CurrentValue:           1.5,
		Threshold:              1.0,
		ConsecutiveBreachCount: 2,
		DetectedAt:             time.Now(),
	}

	m3JSON, err := json.Marshal(m3)
	require.NoError(t, err)
	m5JSON, err := json.Marshal(m5)
	require.NoError(t, err)

	var m3Fields, m5Fields map[string]interface{}
	require.NoError(t, json.Unmarshal(m3JSON, &m3Fields))
	require.NoError(t, json.Unmarshal(m5JSON, &m5Fields))

	m3Keys := sortedKeys(m3Fields)
	m5Keys := sortedKeys(m5Fields)
	assert.Equal(t, m3Keys, m5Keys,
		"M3 GuardrailAlert and M5 Alert must have identical JSON field names")

	// Verify the exact expected field set.
	expected := []string{
		"consecutive_breach_count", "current_value", "detected_at",
		"experiment_id", "metric_id", "threshold", "variant_id",
	}
	assert.Equal(t, expected, m3Keys, "M3 JSON fields must match guardrail contract")
}

// TestM3M5_BidirectionalDeserialization verifies alerts deserialize in both
// directions (M3 → M5 and M5 → M3) with no data loss.
func TestM3M5_BidirectionalDeserialization(t *testing.T) {
	now := time.Now().Truncate(time.Millisecond)

	// M3 → M5 direction.
	m3 := GuardrailAlert{
		ExperimentID:           "exp-bidir",
		MetricID:               "latency_p99",
		VariantID:              "variant-A",
		CurrentValue:           250.0,
		Threshold:              200.0,
		ConsecutiveBreachCount: 5,
		DetectedAt:             now,
	}
	m3JSON, err := json.Marshal(m3)
	require.NoError(t, err)

	var m5 m5Alert
	err = json.Unmarshal(m3JSON, &m5)
	require.NoError(t, err)
	assert.Equal(t, m3.ExperimentID, m5.ExperimentID)
	assert.Equal(t, m3.MetricID, m5.MetricID)
	assert.Equal(t, m3.VariantID, m5.VariantID)
	assert.InDelta(t, m3.CurrentValue, m5.CurrentValue, 1e-9)
	assert.InDelta(t, m3.Threshold, m5.Threshold, 1e-9)
	assert.Equal(t, m3.ConsecutiveBreachCount, m5.ConsecutiveBreachCount)
	assert.Equal(t, m3.DetectedAt.UnixMilli(), m5.DetectedAt.UnixMilli())

	// M5 → M3 direction.
	m5JSON, err := json.Marshal(m5)
	require.NoError(t, err)

	var roundTripped GuardrailAlert
	err = json.Unmarshal(m5JSON, &roundTripped)
	require.NoError(t, err)
	assert.Equal(t, m3.ExperimentID, roundTripped.ExperimentID)
	assert.Equal(t, m3.CurrentValue, roundTripped.CurrentValue)
	assert.Equal(t, m3.DetectedAt.UnixMilli(), roundTripped.DetectedAt.UnixMilli())
}

// TestM3M5_ZeroValueHandling verifies that zero-value alerts survive the
// JSON roundtrip (e.g., zero breach count, zero threshold).
func TestM3M5_ZeroValueHandling(t *testing.T) {
	m3 := GuardrailAlert{
		ExperimentID:           "exp-zero",
		MetricID:               "metric-zero",
		VariantID:              "",
		CurrentValue:           0.0,
		Threshold:              0.0,
		ConsecutiveBreachCount: 0,
		DetectedAt:             time.Time{},
	}
	data, err := json.Marshal(m3)
	require.NoError(t, err)

	var m5 m5Alert
	err = json.Unmarshal(data, &m5)
	require.NoError(t, err)
	assert.Equal(t, "exp-zero", m5.ExperimentID)
	assert.Equal(t, "", m5.VariantID)
	assert.Equal(t, 0.0, m5.CurrentValue)
	assert.Equal(t, 0, m5.ConsecutiveBreachCount)
}

func sortedKeys(m map[string]interface{}) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	for i := 0; i < len(keys); i++ {
		for j := i + 1; j < len(keys); j++ {
			if keys[i] > keys[j] {
				keys[i], keys[j] = keys[j], keys[i]
			}
		}
	}
	return keys
}
