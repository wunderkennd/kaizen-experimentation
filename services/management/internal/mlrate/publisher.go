// Package mlrate handles MLRATE model training request publishing (ADR-015 Phase 2).
//
// When an experiment transitions to STARTING state with SEQUENTIAL_METHOD_AVLM
// and a configured surrogate model, M5 emits a ModelTrainingRequest to the
// model_training_requests Kafka topic so M3 can train an ML control variate.
package mlrate

import (
	"context"
	"encoding/json"
	"sync"
	"time"

	"github.com/segmentio/kafka-go"
)

// Topic is the Kafka topic for MLRATE model training requests.
const Topic = "model_training_requests"

// avlmMethod is the string stored in experiments.sequential_method for AVLM.
const avlmMethod = "AVLM"

// trainingWindowDays is the number of days of historical data used to train
// the MLRATE control variate model.
const trainingWindowDays = 30

// ModelTrainingRequest is the envelope published to Kafka so M3 can train an
// ML-predicted control variate for AVLM experiments (MLRATE framework).
//
// M3 trains a LightGBM/XGBoost model predicting MetricID from
// pre-experiment features in the [TrainingDataStart, TrainingDataEnd] window,
// then stores cross-fitted predictions for M4a to use as the control variate.
type ModelTrainingRequest struct {
	ExperimentID      string `json:"experiment_id"`
	MetricID          string `json:"metric_id"`
	CovariateMetricID string `json:"covariate_metric_id"`
	TrainingDataStart string `json:"training_data_start"` // RFC3339
	TrainingDataEnd   string `json:"training_data_end"`   // RFC3339
}

// Publisher publishes model training requests to Kafka.
type Publisher interface {
	Publish(ctx context.Context, req ModelTrainingRequest) error
}

// KafkaPublisher publishes model training requests to the model_training_requests topic.
type KafkaPublisher struct {
	writer *kafka.Writer
}

// NewKafkaPublisher creates a KafkaPublisher targeting the model_training_requests topic.
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

// Publish sends a model training request to Kafka, keyed by experiment_id.
func (p *KafkaPublisher) Publish(ctx context.Context, req ModelTrainingRequest) error {
	value, err := json.Marshal(req)
	if err != nil {
		return err
	}
	return p.writer.WriteMessages(ctx, kafka.Message{
		Key:   []byte(req.ExperimentID),
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

// MemPublisher is an in-memory Publisher for testing.
type MemPublisher struct {
	mu       sync.Mutex
	requests []ModelTrainingRequest
}

// NewMemPublisher creates a new in-memory publisher.
func NewMemPublisher() *MemPublisher {
	return &MemPublisher{}
}

// Publish stores the request in memory.
func (p *MemPublisher) Publish(_ context.Context, req ModelTrainingRequest) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.requests = append(p.requests, req)
	return nil
}

// Requests returns all published requests.
func (p *MemPublisher) Requests() []ModelTrainingRequest {
	p.mu.Lock()
	defer p.mu.Unlock()
	out := make([]ModelTrainingRequest, len(p.requests))
	copy(out, p.requests)
	return out
}

// Reset clears all published requests.
func (p *MemPublisher) Reset() {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.requests = nil
}

// ShouldTrigger returns true when the given sequential method and surrogate
// model ID combination warrants emitting a model training request.
//
// Criteria: sequential_method == "AVLM" AND surrogateModelID != "".
func ShouldTrigger(sequentialMethod, surrogateModelID string) bool {
	return sequentialMethod == avlmMethod && surrogateModelID != ""
}

// Emit publishes a ModelTrainingRequest if the experiment qualifies for MLRATE
// model training. Returns true if the event was successfully published.
//
// The training window is [now - 30 days, now], providing M3 with a 30-day
// pre-experiment lookback to train the ML control variate.
func Emit(
	ctx context.Context,
	pub Publisher,
	experimentID, sequentialMethod, surrogateModelID,
	primaryMetricID, covariateMetricID string,
	now time.Time,
) bool {
	if !ShouldTrigger(sequentialMethod, surrogateModelID) {
		return false
	}
	req := ModelTrainingRequest{
		ExperimentID:      experimentID,
		MetricID:          primaryMetricID,
		CovariateMetricID: covariateMetricID,
		TrainingDataStart: now.UTC().Add(-trainingWindowDays * 24 * time.Hour).Format(time.RFC3339),
		TrainingDataEnd:   now.UTC().Format(time.RFC3339),
	}
	return pub.Publish(ctx, req) == nil
}
