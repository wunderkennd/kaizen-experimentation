//go:build integration

package alerts

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/segmentio/kafka-go"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ensureTopic creates a Kafka topic using the admin Client API, which works
// reliably with KRaft mode. The old conn.CreateTopics path is unreliable
// because it doesn't always reach the controller.
func ensureTopic(t *testing.T, brokerAddr, topic string, partitions int) {
	t.Helper()

	// Quick connectivity check — skip if Kafka isn't available.
	conn, err := kafka.Dial("tcp", brokerAddr)
	if err != nil {
		t.Skipf("Kafka not available at %s: %v", brokerAddr, err)
	}
	conn.Close()

	client := &kafka.Client{
		Addr:    kafka.TCP(brokerAddr),
		Timeout: 10 * time.Second,
	}

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()

	resp, err := client.CreateTopics(ctx, &kafka.CreateTopicsRequest{
		Topics: []kafka.TopicConfig{{
			Topic:             topic,
			NumPartitions:     partitions,
			ReplicationFactor: 1,
		}},
	})
	require.NoError(t, err)

	// Tolerate "already exists" but fail on other per-topic errors.
	for name, topicErr := range resp.Errors {
		if topicErr != nil && !strings.Contains(topicErr.Error(), "Topic with this name already exists") {
			t.Fatalf("Failed to create topic %s: %v", name, topicErr)
		}
	}
}

func TestKafkaPublisher_Integration(t *testing.T) {
	brokers := []string{"localhost:9092"}
	topic := "guardrail_alerts_test"

	ensureTopic(t, brokers[0], topic, 1)

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

	err := pub.PublishAlert(ctx, alert)
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

	ensureTopic(t, brokers[0], topic, 3)

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
