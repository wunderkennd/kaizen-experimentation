//go:build integration

package alerts

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	"github.com/segmentio/kafka-go"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// TestM3M5_GuardrailAlertKafkaRoundTrip verifies end-to-end: M3's KafkaPublisher
// produces to Kafka, and the message deserializes into M5's Alert contract struct.
func TestM3M5_GuardrailAlertKafkaRoundTrip(t *testing.T) {
	brokers := []string{"localhost:9092"}
	topic := "guardrail_alerts_m3m5_contract"

	conn, err := kafka.Dial("tcp", brokers[0])
	if err != nil {
		t.Skipf("Kafka not available at %s: %v", brokers[0], err)
	}
	defer conn.Close()

	_ = conn.CreateTopics(kafka.TopicConfig{
		Topic:             topic,
		NumPartitions:     1,
		ReplicationFactor: 1,
	})

	pub := NewKafkaPublisher(brokers, topic)
	defer pub.Close()

	now := time.Now().Truncate(time.Millisecond)
	m3 := GuardrailAlert{
		ExperimentID:           "exp-guardrail-001",
		MetricID:               "error_rate",
		VariantID:              "variant-B",
		CurrentValue:           0.053,
		Threshold:              0.01,
		ConsecutiveBreachCount: 3,
		DetectedAt:             now,
	}

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	err = pub.PublishAlert(ctx, m3)
	require.NoError(t, err, "M3 publisher should produce to Kafka")

	reader := kafka.NewReader(kafka.ReaderConfig{
		Brokers:   brokers,
		Topic:     topic,
		Partition: 0,
		MinBytes:  1,
		MaxBytes:  1e6,
	})
	defer reader.Close()
	reader.SetOffset(kafka.LastOffset - 1)

	msg, err := reader.ReadMessage(ctx)
	require.NoError(t, err, "should read message from Kafka")

	assert.Equal(t, "exp-guardrail-001", string(msg.Key),
		"message key should be experiment_id for partition ordering")

	// Deserialize into M5's contract struct.
	var m5 m5Alert
	err = json.Unmarshal(msg.Value, &m5)
	require.NoError(t, err, "M3 alert JSON must deserialize into M5's Alert struct")

	assert.Equal(t, m3.ExperimentID, m5.ExperimentID)
	assert.Equal(t, m3.MetricID, m5.MetricID)
	assert.Equal(t, m3.VariantID, m5.VariantID)
	assert.InDelta(t, m3.CurrentValue, m5.CurrentValue, 1e-9)
	assert.InDelta(t, m3.Threshold, m5.Threshold, 1e-9)
	assert.Equal(t, m3.ConsecutiveBreachCount, m5.ConsecutiveBreachCount)
	assert.Equal(t, m3.DetectedAt.UnixMilli(), m5.DetectedAt.UnixMilli(),
		"DetectedAt should survive JSON roundtrip through Kafka")
}
