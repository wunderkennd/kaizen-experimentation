package recalconsumer

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"time"

	"github.com/segmentio/kafka-go"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
)

const (
	// Topic is the Kafka topic for surrogate recalibration requests from M5.
	Topic = "surrogate_recalibration_requests"
	// ConsumerGroup is the Kafka consumer group for M3 recalibration.
	ConsumerGroup = "m3-recalibration"
)

// RecalibrationRequest is the JSON message published by M5 TriggerSurrogateRecalibration.
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

// Consumer reads RecalibrationRequest messages from Kafka and triggers recalibration jobs.
type Consumer struct {
	reader *kafka.Reader
	job    *jobs.RecalibrationJob
	config *config.ConfigStore
	cancel context.CancelFunc
	done   chan struct{}
}

// NewConsumer creates a Kafka consumer for surrogate recalibration requests.
func NewConsumer(brokers []string, job *jobs.RecalibrationJob, cfg *config.ConfigStore) *Consumer {
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
		reader: reader,
		job:    job,
		config: cfg,
		done:   make(chan struct{}),
	}
}

// Start begins consuming recalibration requests in a background goroutine.
func (c *Consumer) Start(ctx context.Context) {
	ctx, c.cancel = context.WithCancel(ctx)
	go c.consume(ctx)
}

func (c *Consumer) consume(ctx context.Context) {
	defer close(c.done)
	slog.Info("recalibration consumer: started", "topic", Topic, "group", ConsumerGroup)

	for {
		msg, err := c.reader.FetchMessage(ctx)
		if err != nil {
			if ctx.Err() != nil || err == io.EOF {
				slog.Info("recalibration consumer: shutting down")
				return
			}
			slog.Error("recalibration consumer: fetch error, retrying", "error", err)
			select {
			case <-ctx.Done():
				return
			case <-time.After(2 * time.Second):
			}
			continue
		}

		if err := c.processMessage(ctx, msg); err != nil {
			slog.Error("recalibration consumer: process error",
				"error", err, "offset", msg.Offset, "partition", msg.Partition)
			// Don't commit on job failure — message will be redelivered (at-least-once).
			select {
			case <-ctx.Done():
				return
			case <-time.After(time.Second):
			}
			continue
		}

		if err := c.reader.CommitMessages(ctx, msg); err != nil {
			slog.Error("recalibration consumer: commit error", "error", err)
		}
	}
}

// processMessage handles a single Kafka message. Extracted for testability.
func (c *Consumer) processMessage(ctx context.Context, msg kafka.Message) error {
	var req RecalibrationRequest
	if err := json.Unmarshal(msg.Value, &req); err != nil {
		slog.Error("recalibration consumer: invalid JSON, skipping",
			"error", err, "offset", msg.Offset)
		return nil // Return nil to commit offset — no point retrying malformed messages.
	}

	if req.ModelID == "" {
		slog.Warn("recalibration consumer: empty model_id, skipping", "offset", msg.Offset)
		return nil
	}

	experimentIDs := c.config.GetExperimentsByModelID(req.ModelID)
	if len(experimentIDs) == 0 {
		slog.Warn("recalibration consumer: no experiments for model, skipping",
			"model_id", req.ModelID)
		return nil
	}

	for _, expID := range experimentIDs {
		result, err := c.job.Run(ctx, expID)
		if err != nil {
			return err
		}
		if result != nil {
			slog.Info("recalibration consumer: completed recalibration",
				"experiment_id", expID,
				"model_id", result.ModelID,
				"new_r_squared", result.NewRSquared,
				"data_points", result.DataPoints)
		}
	}

	return nil
}

// Close signals shutdown and waits for the consumer goroutine to exit.
func (c *Consumer) Close() {
	if c.cancel != nil {
		c.cancel()
	}
	<-c.done
	if err := c.reader.Close(); err != nil {
		slog.Error("recalibration consumer: close error", "error", err)
	}
}
