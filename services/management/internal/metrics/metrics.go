// Package metrics defines Prometheus metrics for the M5 management service.
package metrics

import (
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promauto"
)

var (
	// AlertsProcessed counts processed Kafka messages.
	// Labels: consumer ("guardrail" | "sequential"), result (consumer-specific).
	AlertsProcessed = promauto.NewCounterVec(prometheus.CounterOpts{
		Name: "m5_alerts_processed_total",
		Help: "Total number of processed Kafka alert messages.",
	}, []string{"consumer", "result"})

	// AlertProcessingDuration tracks end-to-end processing latency per message.
	// Labels: consumer ("guardrail" | "sequential").
	AlertProcessingDuration = promauto.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "m5_alert_processing_duration_seconds",
		Help:    "Duration from message fetch to commit, in seconds.",
		Buckets: []float64{0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10},
	}, []string{"consumer"})

	// FetchErrors counts Kafka fetch errors (retried).
	// Labels: consumer ("guardrail" | "sequential").
	FetchErrors = promauto.NewCounterVec(prometheus.CounterOpts{
		Name: "m5_kafka_fetch_errors_total",
		Help: "Total number of Kafka fetch errors.",
	}, []string{"consumer"})

	// LastProcessedTimestamp tracks when the last message was successfully processed.
	// Labels: consumer ("guardrail" | "sequential").
	LastProcessedTimestamp = promauto.NewGaugeVec(prometheus.GaugeOpts{
		Name: "m5_last_processed_timestamp_seconds",
		Help: "Unix timestamp of the last successfully processed message.",
	}, []string{"consumer"})
)

// Init pre-populates all metric label combinations so that metric families
// always appear in /metrics output, even before any Kafka messages are processed.
// Call from main.go to ensure Prometheus scrapes see all m5_* families at startup.
func Init() {
	for _, c := range []string{"guardrail", "sequential"} {
		FetchErrors.WithLabelValues(c).Add(0)
		AlertProcessingDuration.WithLabelValues(c)
		LastProcessedTimestamp.WithLabelValues(c)
		for _, r := range []string{"skipped", "paused", "alert_only", "concluded", "error", "invalid_message"} {
			AlertsProcessed.WithLabelValues(c, r).Add(0)
		}
	}
}
