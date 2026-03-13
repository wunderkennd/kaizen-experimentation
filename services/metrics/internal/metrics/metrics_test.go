package metrics

import (
	"testing"

	"github.com/prometheus/client_golang/prometheus"
	dto "github.com/prometheus/client_model/go"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestAllMetricsRegistered(t *testing.T) {
	// Use Describe to verify promauto registration (Gather only returns metrics with observations).
	collectors := []prometheus.Collector{
		RPCDuration, RPCTotal, JobDuration, JobTotal,
		SparkQueryDuration, SparkQueryRows, GuardrailBreaches,
	}

	expected := []string{
		"m3_rpc_duration_seconds",
		"m3_rpc_total",
		"m3_job_duration_seconds",
		"m3_job_total",
		"m3_spark_query_duration_seconds",
		"m3_spark_query_rows",
		"m3_guardrail_breaches_total",
	}

	for i, c := range collectors {
		ch := make(chan *prometheus.Desc, 1)
		c.Describe(ch)
		desc := <-ch
		require.NotNil(t, desc, "collector %d should have a description", i)
		assert.Contains(t, desc.String(), expected[i],
			"metric %q should be registered", expected[i])
	}
}

func TestRPCDurationLabelCardinality(t *testing.T) {
	// Observe with valid labels — should not panic.
	RPCDuration.WithLabelValues("ComputeMetrics", "ok").Observe(0.5)
	RPCDuration.WithLabelValues("ComputeMetrics", "error").Observe(1.0)

	// Verify both label sets are present.
	ch := make(chan prometheus.Metric, 10)
	RPCDuration.Collect(ch)
	close(ch)

	count := 0
	for m := range ch {
		d := &dto.Metric{}
		require.NoError(t, m.Write(d))
		count++
	}
	assert.GreaterOrEqual(t, count, 2, "should have at least 2 label combinations")
}

func TestRPCTotalLabelCardinality(t *testing.T) {
	RPCTotal.WithLabelValues("ExportNotebook", "ok").Inc()
	RPCTotal.WithLabelValues("ExportNotebook", "error").Inc()

	ch := make(chan prometheus.Metric, 10)
	RPCTotal.Collect(ch)
	close(ch)

	count := 0
	for m := range ch {
		d := &dto.Metric{}
		require.NoError(t, m.Write(d))
		count++
	}
	assert.GreaterOrEqual(t, count, 2)
}

func TestJobMetricsDoNotPanic(t *testing.T) {
	assert.NotPanics(t, func() {
		JobDuration.WithLabelValues("daily_metric", "exp-001").Observe(12.5)
		JobTotal.WithLabelValues("daily_metric", "ok").Inc()
		JobTotal.WithLabelValues("daily_metric", "error").Inc()
	})
}

func TestSparkQueryMetricsDoNotPanic(t *testing.T) {
	assert.NotPanics(t, func() {
		SparkQueryDuration.WithLabelValues("daily_metric").Observe(3.14)
		SparkQueryRows.WithLabelValues("daily_metric").Observe(500)
	})
}

func TestGuardrailBreachesDoNotPanic(t *testing.T) {
	assert.NotPanics(t, func() {
		GuardrailBreaches.WithLabelValues("exp-001", "metric-001", "alert").Inc()
	})
}

func TestSparkQueryRowsBuckets(t *testing.T) {
	// Verify custom bucket boundaries are applied.
	SparkQueryRows.WithLabelValues("test_bucket_check").Observe(42)

	ch := make(chan prometheus.Metric, 10)
	SparkQueryRows.Collect(ch)
	close(ch)

	for m := range ch {
		d := &dto.Metric{}
		require.NoError(t, m.Write(d))
		if d.Histogram != nil {
			// Custom buckets: 0, 10, 50, 100, 500, 1000, 5000, 10000, 50000, 100000 = 10 boundaries
			assert.Equal(t, 10, len(d.Histogram.Bucket), "should have 10 bucket boundaries")
			break
		}
	}
}
