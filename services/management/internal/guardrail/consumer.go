package guardrail

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"time"

	"github.com/segmentio/kafka-go"

	"github.com/org/experimentation-platform/services/management/internal/metrics"
)

const (
	// Topic is the Kafka topic for guardrail alerts.
	Topic = "guardrail_alerts"
	// ConsumerGroup is the Kafka consumer group for M5 auto-pause.
	ConsumerGroup = "management-guardrail"
)

// Consumer reads guardrail alerts from Kafka and delegates to the Processor.
type Consumer struct {
	reader    *kafka.Reader
	processor *Processor
	cancel    context.CancelFunc
	done      chan struct{}
}

// NewConsumer creates a Kafka consumer for guardrail alerts.
// brokers is a list of Kafka broker addresses (e.g., ["localhost:9092"]).
func NewConsumer(brokers []string, processor *Processor) *Consumer {
	reader := kafka.NewReader(kafka.ReaderConfig{
		Brokers:        brokers,
		Topic:          Topic,
		GroupID:        ConsumerGroup,
		MinBytes:       1,
		MaxBytes:       1e6, // 1MB
		CommitInterval: time.Second,
		StartOffset:    kafka.LastOffset,
	})

	return &Consumer{
		reader:    reader,
		processor: processor,
		done:      make(chan struct{}),
	}
}

// Start begins consuming alerts in a background goroutine.
func (c *Consumer) Start(ctx context.Context) {
	ctx, c.cancel = context.WithCancel(ctx)
	go c.consume(ctx)
}

func (c *Consumer) consume(ctx context.Context) {
	defer close(c.done)
	slog.Info("guardrail consumer: started", "topic", Topic, "group", ConsumerGroup)

	const consumer = "guardrail"

	for {
		fetchStart := time.Now()
		msg, err := c.reader.FetchMessage(ctx)
		if err != nil {
			if ctx.Err() != nil || err == io.EOF {
				slog.Info("guardrail consumer: shutting down")
				return
			}
			metrics.FetchErrors.WithLabelValues(consumer).Inc()
			slog.Error("guardrail consumer: fetch error, retrying", "error", err)
			select {
			case <-ctx.Done():
				return
			case <-time.After(2 * time.Second):
			}
			continue
		}

		var alert Alert
		if err := json.Unmarshal(msg.Value, &alert); err != nil {
			metrics.AlertsProcessed.WithLabelValues(consumer, "invalid_message").Inc()
			slog.Error("guardrail consumer: invalid message payload",
				"error", err, "offset", msg.Offset, "partition", msg.Partition)
			// Commit and skip malformed messages.
			_ = c.reader.CommitMessages(ctx, msg)
			continue
		}

		result, err := c.processor.ProcessAlert(ctx, alert)
		if err != nil {
			metrics.AlertsProcessed.WithLabelValues(consumer, "error").Inc()
			slog.Error("guardrail consumer: process error",
				"error", err,
				"experiment_id", alert.ExperimentID,
				"metric_id", alert.MetricID)
			// Don't commit — will retry on next fetch.
			select {
			case <-ctx.Done():
				return
			case <-time.After(time.Second):
			}
			continue
		}

		rs := resultString(result)
		metrics.AlertsProcessed.WithLabelValues(consumer, rs).Inc()
		metrics.AlertProcessingDuration.WithLabelValues(consumer).Observe(time.Since(fetchStart).Seconds())
		slog.Info("guardrail consumer: processed alert",
			"experiment_id", alert.ExperimentID,
			"metric_id", alert.MetricID,
			"result", rs)

		if err := c.reader.CommitMessages(ctx, msg); err != nil {
			slog.Error("guardrail consumer: commit error", "error", err)
		} else {
			metrics.LastProcessedTimestamp.WithLabelValues(consumer).SetToCurrentTime()
		}
	}
}

// Stop shuts down the consumer and waits for the goroutine to exit.
func (c *Consumer) Stop() {
	if c.cancel != nil {
		c.cancel()
	}
	<-c.done
	if err := c.reader.Close(); err != nil {
		slog.Error("guardrail consumer: close error", "error", err)
	}
}

func resultString(r ProcessResult) string {
	switch r {
	case ResultSkipped:
		return "skipped"
	case ResultAlertOnly:
		return "alert_only"
	case ResultPaused:
		return "paused"
	default:
		return "unknown"
	}
}
