// Package surrogate provides Kafka publishing for surrogate model recalibration requests.
package surrogate

import (
	"context"
	"encoding/json"
	"sync"
	"time"

	"github.com/segmentio/kafka-go"
)

// Topic is the Kafka topic for surrogate recalibration requests.
const Topic = "surrogate_recalibration_requests"

// RecalibrationRequest is the envelope published to Kafka so M3 can begin
// recalibration without querying M5 back.
type RecalibrationRequest struct {
	ModelID               string   `json:"model_id"`
	TargetMetricID        string   `json:"target_metric_id"`
	InputMetricIDs        []string `json:"input_metric_ids"`
	ModelType             string   `json:"model_type"`
	ObservationWindowDays int32    `json:"observation_window_days"`
	PredictionHorizonDays int32    `json:"prediction_horizon_days"`
	RequestedBy           string   `json:"requested_by"`
	RequestedAt           string   `json:"requested_at"`
}

// Publisher publishes surrogate recalibration requests.
type Publisher interface {
	Publish(ctx context.Context, req RecalibrationRequest) error
}

// KafkaPublisher publishes recalibration requests to Kafka.
type KafkaPublisher struct {
	writer *kafka.Writer
}

// NewKafkaPublisher creates a publisher targeting the surrogate_recalibration_requests topic.
func NewKafkaPublisher(brokers []string) *KafkaPublisher {
	w := &kafka.Writer{
		Addr:         kafka.TCP(brokers...),
		Topic:        Topic,
		Balancer:     &kafka.Hash{},
		RequiredAcks: kafka.RequireOne,
		BatchTimeout: 10 * time.Millisecond,
	}
	return &KafkaPublisher{writer: w}
}

// Publish sends a recalibration request to Kafka, keyed by model_id.
func (p *KafkaPublisher) Publish(ctx context.Context, req RecalibrationRequest) error {
	value, err := json.Marshal(req)
	if err != nil {
		return err
	}

	return p.writer.WriteMessages(ctx, kafka.Message{
		Key:   []byte(req.ModelID),
		Value: value,
	})
}

// Close closes the underlying Kafka writer.
func (p *KafkaPublisher) Close() error {
	if p == nil {
		return nil
	}
	return p.writer.Close()
}

// MemPublisher is an in-memory publisher for testing.
type MemPublisher struct {
	mu       sync.Mutex
	requests []RecalibrationRequest
}

// NewMemPublisher creates a new in-memory publisher.
func NewMemPublisher() *MemPublisher {
	return &MemPublisher{}
}

// Publish stores the request in memory.
func (p *MemPublisher) Publish(_ context.Context, req RecalibrationRequest) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.requests = append(p.requests, req)
	return nil
}

// Requests returns all published requests.
func (p *MemPublisher) Requests() []RecalibrationRequest {
	p.mu.Lock()
	defer p.mu.Unlock()
	out := make([]RecalibrationRequest, len(p.requests))
	copy(out, p.requests)
	return out
}

// Reset clears all published requests.
func (p *MemPublisher) Reset() {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.requests = nil
}
