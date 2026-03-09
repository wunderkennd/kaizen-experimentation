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

func TestKafkaPublisher_Integration(t *testing.T) {
	brokers := []string{"localhost:9092"}
	topic := "guardrail_alerts_test"

	// Create the topic if it doesn't exist.
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
	alert := GuardrailAlert{
		ExperimentID:           "exp-integration-001",
		MetricID:               "error_rate",
		VariantID:              "variant-B",
		CurrentValue:           0.05,
		Threshold:              0.01,
		ConsecutiveBreachCount: 3,
		DetectedAt:             now,
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	err = pub.PublishAlert(ctx, alert)
	require.NoError(t, err)

	// Read the message back and verify it matches.
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
	require.NoError(t, err)

	assert.Equal(t, "exp-integration-001", string(msg.Key))

	var received GuardrailAlert
	err = json.Unmarshal(msg.Value, &received)
	require.NoError(t, err)
	assert.Equal(t, alert.ExperimentID, received.ExperimentID)
	assert.Equal(t, alert.MetricID, received.MetricID)
	assert.Equal(t, alert.VariantID, received.VariantID)
	assert.Equal(t, alert.CurrentValue, received.CurrentValue)
	assert.Equal(t, alert.Threshold, received.Threshold)
	assert.Equal(t, alert.ConsecutiveBreachCount, received.ConsecutiveBreachCount)
	assert.Equal(t, alert.DetectedAt.UnixMilli(), received.DetectedAt.UnixMilli())
}

func TestKafkaPublisher_MultipleAlerts_Integration(t *testing.T) {
	brokers := []string{"localhost:9092"}
	topic := "guardrail_alerts_multi_test"

	conn, err := kafka.Dial("tcp", brokers[0])
	if err != nil {
		t.Skipf("Kafka not available at %s: %v", brokers[0], err)
	}
	defer conn.Close()

	_ = conn.CreateTopics(kafka.TopicConfig{
		Topic:             topic,
		NumPartitions:     3,
		ReplicationFactor: 1,
	})

	pub := NewKafkaPublisher(brokers, topic)
	defer pub.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Publish alerts for different experiments.
	experiments := []string{"exp-A", "exp-B", "exp-C"}
	for _, expID := range experiments {
		err := pub.PublishAlert(ctx, GuardrailAlert{
			ExperimentID:           expID,
			MetricID:               "latency_p99",
			VariantID:              "variant-1",
			CurrentValue:           500.0,
			Threshold:              200.0,
			ConsecutiveBreachCount: 1,
			DetectedAt:             time.Now(),
		})
		require.NoError(t, err)
	}

	// Verify all three were produced by reading back.
	reader := kafka.NewReader(kafka.ReaderConfig{
		Brokers:  brokers,
		Topic:    topic,
		GroupID:  "test-multi-alert-" + time.Now().Format("20060102150405"),
		MinBytes: 1,
		MaxBytes: 1e6,
	})
	defer reader.Close()

	received := make(map[string]bool)
	for i := 0; i < 3; i++ {
		msg, err := reader.ReadMessage(ctx)
		require.NoError(t, err)
		received[string(msg.Key)] = true
	}

	for _, expID := range experiments {
		assert.True(t, received[expID], "missing alert for experiment %s", expID)
	}
}
