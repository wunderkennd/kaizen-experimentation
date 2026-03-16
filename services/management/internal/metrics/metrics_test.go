package metrics

import (
	"testing"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/stretchr/testify/assert"
)

func TestAllMetricsRegistered(t *testing.T) {
	// Use Describe to verify promauto registration (Gather only returns metrics with observations).
	collectors := []prometheus.Collector{
		AlertsProcessed,
		AlertProcessingDuration,
		FetchErrors,
		LastProcessedTimestamp,
	}

	for _, c := range collectors {
		ch := make(chan *prometheus.Desc, 10)
		c.Describe(ch)
		close(ch)
		count := 0
		for range ch {
			count++
		}
		assert.Greater(t, count, 0, "collector should emit at least one Desc")
	}
}

func TestAlertsProcessedLabelCardinality(t *testing.T) {
	consumers := []string{"guardrail", "sequential"}
	results := []string{"skipped", "paused", "alert_only", "concluded", "error", "invalid_message"}

	for _, c := range consumers {
		for _, r := range results {
			assert.NotPanics(t, func() {
				AlertsProcessed.WithLabelValues(c, r).Add(0)
			}, "should not panic for consumer=%s result=%s", c, r)
		}
	}
}

func TestAlertProcessingDurationBuckets(t *testing.T) {
	expectedBuckets := []float64{0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10}

	// Observe a value to make the metric gatherable.
	AlertProcessingDuration.WithLabelValues("guardrail").Observe(0.042)

	metrics, err := prometheus.DefaultGatherer.Gather()
	assert.NoError(t, err)

	var found bool
	for _, mf := range metrics {
		if mf.GetName() == "m5_alert_processing_duration_seconds" {
			found = true
			for _, m := range mf.GetMetric() {
				buckets := m.GetHistogram().GetBucket()
				assert.Len(t, buckets, len(expectedBuckets), "bucket count mismatch")
				for i, b := range buckets {
					assert.InDelta(t, expectedBuckets[i], b.GetUpperBound(), 1e-9,
						"bucket %d mismatch", i)
				}
			}
		}
	}
	assert.True(t, found, "m5_alert_processing_duration_seconds metric not found")
}

func TestLastProcessedTimestampDoesNotPanic(t *testing.T) {
	for _, consumer := range []string{"guardrail", "sequential"} {
		assert.NotPanics(t, func() {
			LastProcessedTimestamp.WithLabelValues(consumer).SetToCurrentTime()
		}, "SetToCurrentTime should not panic for consumer=%s", consumer)
	}
}

func TestFetchErrorsDoNotPanic(t *testing.T) {
	for _, consumer := range []string{"guardrail", "sequential"} {
		assert.NotPanics(t, func() {
			FetchErrors.WithLabelValues(consumer).Inc()
		}, "Inc should not panic for consumer=%s", consumer)
	}
}
