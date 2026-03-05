package alerts

import (
	"context"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMemPublisher_PublishAlert(t *testing.T) {
	pub := NewMemPublisher()
	err := pub.PublishAlert(context.Background(), GuardrailAlert{
		ExperimentID: "exp-001", MetricID: "error_rate", VariantID: "variant-A",
		CurrentValue: 0.05, Threshold: 0.01, ConsecutiveBreachCount: 3, DetectedAt: time.Now(),
	})
	require.NoError(t, err)
	alerts := pub.Alerts()
	require.Len(t, alerts, 1)
	assert.Equal(t, "exp-001", alerts[0].ExperimentID)
	assert.Equal(t, 0.05, alerts[0].CurrentValue)
}

func TestMemPublisher_MultipleAlerts(t *testing.T) {
	pub := NewMemPublisher()
	for i := 0; i < 5; i++ {
		_ = pub.PublishAlert(context.Background(), GuardrailAlert{ExperimentID: "exp-001"})
	}
	assert.Len(t, pub.Alerts(), 5)
}

func TestMemPublisher_Reset(t *testing.T) {
	pub := NewMemPublisher()
	_ = pub.PublishAlert(context.Background(), GuardrailAlert{ExperimentID: "exp-001"})
	assert.Len(t, pub.Alerts(), 1)
	pub.Reset()
	assert.Len(t, pub.Alerts(), 0)
}

func TestKafkaPublisher_PublishAlert(t *testing.T) {
	pub := NewKafkaPublisher("guardrail_alerts")
	err := pub.PublishAlert(context.Background(), GuardrailAlert{ExperimentID: "exp-001", MetricID: "error_rate"})
	assert.NoError(t, err)
}
