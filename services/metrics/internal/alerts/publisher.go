package alerts

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"sync"
	"time"

	"github.com/segmentio/kafka-go"
)

type GuardrailAlert struct {
	ExperimentID           string    `json:"experiment_id"`
	MetricID               string    `json:"metric_id"`
	VariantID              string    `json:"variant_id"`
	CurrentValue           float64   `json:"current_value"`
	Threshold              float64   `json:"threshold"`
	ConsecutiveBreachCount int       `json:"consecutive_breach_count"`
	DetectedAt             time.Time `json:"detected_at"`
}

type Publisher interface {
	PublishAlert(ctx context.Context, alert GuardrailAlert) error
}

// KafkaPublisher writes guardrail alerts to a Kafka topic using kafka-go.
// Messages are keyed by experiment_id to ensure ordering per experiment.
type KafkaPublisher struct {
	writer *kafka.Writer
}

// NewKafkaPublisher creates a publisher that writes to the given Kafka topic.
func NewKafkaPublisher(brokers []string, topic string) *KafkaPublisher {
	w := &kafka.Writer{
		Addr:         kafka.TCP(brokers...),
		Topic:        topic,
		Balancer:     &kafka.Hash{},
		RequiredAcks: kafka.RequireOne,
		MaxAttempts:  3,
		BatchTimeout: 10 * time.Millisecond,
	}
	return &KafkaPublisher{writer: w}
}

func (p *KafkaPublisher) PublishAlert(ctx context.Context, alert GuardrailAlert) error {
	value, err := json.Marshal(alert)
	if err != nil {
		return fmt.Errorf("alerts: marshal alert: %w", err)
	}
	msg := kafka.Message{
		Key:   []byte(alert.ExperimentID),
		Value: value,
	}
	if err := p.writer.WriteMessages(ctx, msg); err != nil {
		return fmt.Errorf("alerts: publish to kafka: %w", err)
	}
	slog.Info("guardrail alert published",
		"experiment_id", alert.ExperimentID,
		"metric_id", alert.MetricID,
		"variant_id", alert.VariantID,
		"breach_count", alert.ConsecutiveBreachCount)
	return nil
}

// Close flushes pending writes and closes the Kafka writer.
func (p *KafkaPublisher) Close() error {
	return p.writer.Close()
}

type MemPublisher struct {
	mu     sync.Mutex
	alerts []GuardrailAlert
}

func NewMemPublisher() *MemPublisher {
	return &MemPublisher{}
}

func (p *MemPublisher) PublishAlert(_ context.Context, alert GuardrailAlert) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.alerts = append(p.alerts, alert)
	return nil
}

func (p *MemPublisher) Alerts() []GuardrailAlert {
	p.mu.Lock()
	defer p.mu.Unlock()
	out := make([]GuardrailAlert, len(p.alerts))
	copy(out, p.alerts)
	return out
}

func (p *MemPublisher) Reset() {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.alerts = nil
}
