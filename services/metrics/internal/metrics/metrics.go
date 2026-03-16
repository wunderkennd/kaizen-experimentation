// Package metrics defines Prometheus metrics for the M3 metric computation service.
package metrics

import (
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promauto"
)

var (
	// RPCDuration tracks the duration of RPC handler calls.
	RPCDuration = promauto.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "m3_rpc_duration_seconds",
		Help:    "Duration of M3 RPC calls in seconds.",
		Buckets: prometheus.DefBuckets,
	}, []string{"method", "status"})

	// RPCTotal counts the total number of RPC calls.
	RPCTotal = promauto.NewCounterVec(prometheus.CounterOpts{
		Name: "m3_rpc_total",
		Help: "Total number of M3 RPC calls.",
	}, []string{"method", "status"})

	// JobDuration tracks the duration of computation jobs.
	JobDuration = promauto.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "m3_job_duration_seconds",
		Help:    "Duration of M3 computation jobs in seconds.",
		Buckets: prometheus.DefBuckets,
	}, []string{"job_type", "experiment_id"})

	// JobTotal counts the total number of computation jobs.
	JobTotal = promauto.NewCounterVec(prometheus.CounterOpts{
		Name: "m3_job_total",
		Help: "Total number of M3 computation jobs.",
	}, []string{"job_type", "status"})

	// SparkQueryDuration tracks the duration of individual Spark SQL queries.
	SparkQueryDuration = promauto.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "m3_spark_query_duration_seconds",
		Help:    "Duration of Spark SQL queries in seconds.",
		Buckets: prometheus.DefBuckets,
	}, []string{"job_type"})

	// SparkQueryRows tracks the number of rows returned by Spark SQL queries.
	SparkQueryRows = promauto.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "m3_spark_query_rows",
		Help:    "Number of rows returned by Spark SQL queries.",
		Buckets: []float64{0, 10, 50, 100, 500, 1000, 5000, 10000, 50000, 100000},
	}, []string{"job_type"})

	// GuardrailBreaches counts the total number of guardrail breach alerts published.
	GuardrailBreaches = promauto.NewCounterVec(prometheus.CounterOpts{
		Name: "m3_guardrail_breaches_total",
		Help: "Total number of guardrail breach alerts published.",
	}, []string{"experiment_id", "metric_id", "action"})
)
