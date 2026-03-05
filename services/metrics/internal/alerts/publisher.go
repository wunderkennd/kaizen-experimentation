package alerts

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"time"
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

type KafkaPublisher struct {
	topic string
}

func NewKafkaPublisher(topic string) *KafkaPublisher {
	return &KafkaPublisher{topic: topic}
}

func (p *KafkaPublisher) PublishAlert(_ context.Context, alert GuardrailAlert) error {
	_, err := json.Marshal(alert)
	if err != nil {
		return fmt.Errorf("alerts: marshal alert: %w", err)
	}
	return nil
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
