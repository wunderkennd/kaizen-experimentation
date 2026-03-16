package sequential

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
	// Topic is the Kafka topic for sequential boundary alerts from M4a.
	Topic = "sequential_boundary_alerts"
	// ConsumerGroup is the Kafka consumer group for M5 auto-conclude.
	ConsumerGroup = "management-sequential"
)

// Consumer reads sequential boundary alerts from Kafka and delegates to the Processor.
type Consumer struct {
	reader    *kafka.Reader
	processor *Processor
	cancel    context.CancelFunc
	done      chan struct{}
}

// NewConsumer creates a Kafka consumer for sequential boundary alerts.
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
	slog.Info("sequential consumer: started", "topic", Topic, "group", ConsumerGroup)

	const consumer = "sequential"

	for {
		msg, err := c.reader.FetchMessage(ctx)
		if err != nil {
			if ctx.Err() != nil || err == io.EOF {
				slog.Info("sequential consumer: shutting down")
				return
			}
			metrics.FetchErrors.WithLabelValues(consumer).Inc()
			slog.Error("sequential consumer: fetch error, retrying", "error", err)
			select {
			case <-ctx.Done():
				return
			case <-time.After(2 * time.Second):
			}
			continue
		}

		processStart := time.Now()
		var alert BoundaryAlert
		if err := json.Unmarshal(msg.Value, &alert); err != nil {
			metrics.AlertsProcessed.WithLabelValues(consumer, "invalid_message").Inc()
			slog.Error("sequential consumer: invalid message payload",
				"error", err, "offset", msg.Offset, "partition", msg.Partition)
			_ = c.reader.CommitMessages(ctx, msg)
			continue
		}

		result, err := c.processor.ProcessAlert(ctx, alert)
		if err != nil {
			metrics.AlertsProcessed.WithLabelValues(consumer, "error").Inc()
			slog.Error("sequential consumer: process error",
				"error", err,
				"experiment_id", alert.ExperimentID,
				"metric_id", alert.MetricID)
			select {
			case <-ctx.Done():
				return
			case <-time.After(time.Second):
			}
			continue
		}

		rs := resultString(result)
		metrics.AlertsProcessed.WithLabelValues(consumer, rs).Inc()
		metrics.AlertProcessingDuration.WithLabelValues(consumer).Observe(time.Since(processStart).Seconds())
		slog.Info("sequential consumer: processed alert",
			"experiment_id", alert.ExperimentID,
			"metric_id", alert.MetricID,
			"result", rs)

		if err := c.reader.CommitMessages(ctx, msg); err != nil {
			slog.Error("sequential consumer: commit error", "error", err)
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
		slog.Error("sequential consumer: close error", "error", err)
	}
}

func resultString(r ProcessResult) string {
	switch r {
	case ResultSkipped:
		return "skipped"
	case ResultConcluded:
		return "concluded"
	default:
		return "unknown"
	}
}
