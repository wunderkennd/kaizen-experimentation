//! Pipeline-level Prometheus metrics.
//!
//! Counters are labeled by `event_type` (exposure, metric, reward, qoe).
//! The Kafka publish latency histogram is labeled by `topic`.

use prometheus::{Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, Opts};

/// Event type label values for counter metrics.
pub const EVENT_TYPE_EXPOSURE: &str = "exposure";
pub const EVENT_TYPE_METRIC: &str = "metric";
pub const EVENT_TYPE_REWARD: &str = "reward";
pub const EVENT_TYPE_QOE: &str = "qoe";
/// ADR-021: Model retraining event type label.
pub const EVENT_TYPE_MODEL_RETRAINING: &str = "model_retraining";

/// Pipeline metrics registered with a Prometheus registry.
#[derive(Clone)]
pub struct PipelineMetrics {
    pub events_accepted: IntCounterVec,
    pub events_rejected: IntCounterVec,
    pub events_deduplicated: IntCounterVec,
    pub events_backpressure: IntCounterVec,
    pub kafka_publish_latency: HistogramVec,
    pub event_ingest_delay: HistogramVec,
}

impl PipelineMetrics {
    /// Create and register pipeline metrics with the given Prometheus registry.
    pub fn new(registry: &prometheus::Registry) -> Self {
        let events_accepted = IntCounterVec::new(
            Opts::new(
                "events_accepted_total",
                "Total events accepted and published to Kafka",
            ),
            &["event_type"],
        )
        .unwrap();

        let events_rejected = IntCounterVec::new(
            Opts::new(
                "events_rejected_total",
                "Total events rejected due to validation failure",
            ),
            &["event_type"],
        )
        .unwrap();

        let events_deduplicated = IntCounterVec::new(
            Opts::new(
                "events_deduplicated_total",
                "Total events rejected as duplicates by Bloom filter",
            ),
            &["event_type"],
        )
        .unwrap();

        let events_backpressure = IntCounterVec::new(
            Opts::new(
                "events_backpressure_total",
                "Total events rejected due to Kafka queue backpressure",
            ),
            &["event_type"],
        )
        .unwrap();

        let kafka_publish_latency = HistogramVec::new(
            HistogramOpts::new(
                "kafka_publish_latency_seconds",
                "Kafka publish latency in seconds",
            )
            .buckets(vec![
                0.0001, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
            ]),
            &["topic"],
        )
        .unwrap();

        let event_ingest_delay = HistogramVec::new(
            HistogramOpts::new(
                "event_ingest_delay_seconds",
                "Delay between event timestamp and M2 ingest time (client clock skew + network)",
            )
            .buckets(vec![
                0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 3600.0,
            ]),
            &["event_type"],
        )
        .unwrap();

        registry
            .register(Box::new(events_accepted.clone()))
            .unwrap();
        registry
            .register(Box::new(events_rejected.clone()))
            .unwrap();
        registry
            .register(Box::new(events_deduplicated.clone()))
            .unwrap();
        registry
            .register(Box::new(events_backpressure.clone()))
            .unwrap();
        registry
            .register(Box::new(kafka_publish_latency.clone()))
            .unwrap();
        registry
            .register(Box::new(event_ingest_delay.clone()))
            .unwrap();

        Self {
            events_accepted,
            events_rejected,
            events_deduplicated,
            events_backpressure,
            kafka_publish_latency,
            event_ingest_delay,
        }
    }

    /// Create a no-op metrics instance (for testing without a real registry).
    #[cfg(test)]
    pub fn noop() -> Self {
        let registry = prometheus::Registry::new();
        Self::new(&registry)
    }

    /// Get the accepted counter for a given event type.
    pub fn accepted(&self, event_type: &str) -> IntCounter {
        self.events_accepted.with_label_values(&[event_type])
    }

    /// Get the rejected counter for a given event type.
    pub fn rejected(&self, event_type: &str) -> IntCounter {
        self.events_rejected.with_label_values(&[event_type])
    }

    /// Get the deduplicated counter for a given event type.
    pub fn deduplicated(&self, event_type: &str) -> IntCounter {
        self.events_deduplicated.with_label_values(&[event_type])
    }

    /// Get the backpressure counter for a given event type.
    pub fn backpressure(&self, event_type: &str) -> IntCounter {
        self.events_backpressure.with_label_values(&[event_type])
    }

    /// Get the publish latency histogram for a given topic.
    pub fn publish_latency(&self, topic: &str) -> Histogram {
        self.kafka_publish_latency.with_label_values(&[topic])
    }

    /// Get the ingest delay histogram for a given event type.
    /// Records the time between event timestamp and server ingest time.
    pub fn ingest_delay(&self, event_type: &str) -> Histogram {
        self.event_ingest_delay.with_label_values(&[event_type])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_register_and_increment() {
        let registry = prometheus::Registry::new();
        let metrics = PipelineMetrics::new(&registry);

        metrics.accepted(EVENT_TYPE_EXPOSURE).inc();
        metrics.accepted(EVENT_TYPE_EXPOSURE).inc();
        metrics.rejected(EVENT_TYPE_METRIC).inc();
        metrics.deduplicated(EVENT_TYPE_REWARD).inc();
        metrics.backpressure(EVENT_TYPE_QOE).inc();

        let families = registry.gather();

        let accepted = families
            .iter()
            .find(|mf| mf.get_name() == "events_accepted_total")
            .unwrap();
        let exposure_counter = accepted
            .get_metric()
            .iter()
            .find(|m| {
                m.get_label()
                    .iter()
                    .any(|l| l.get_name() == "event_type" && l.get_value() == "exposure")
            })
            .unwrap();
        assert_eq!(exposure_counter.get_counter().get_value(), 2.0);

        let rejected = families
            .iter()
            .find(|mf| mf.get_name() == "events_rejected_total")
            .unwrap();
        let metric_counter = rejected
            .get_metric()
            .iter()
            .find(|m| {
                m.get_label()
                    .iter()
                    .any(|l| l.get_name() == "event_type" && l.get_value() == "metric")
            })
            .unwrap();
        assert_eq!(metric_counter.get_counter().get_value(), 1.0);
    }

    #[test]
    fn test_histogram_observe() {
        let registry = prometheus::Registry::new();
        let metrics = PipelineMetrics::new(&registry);

        metrics.publish_latency("exposures").observe(0.005);
        metrics.publish_latency("exposures").observe(0.010);

        let families = registry.gather();
        let latency = families
            .iter()
            .find(|mf| mf.get_name() == "kafka_publish_latency_seconds")
            .unwrap();
        let exposures_hist = latency
            .get_metric()
            .iter()
            .find(|m| {
                m.get_label()
                    .iter()
                    .any(|l| l.get_name() == "topic" && l.get_value() == "exposures")
            })
            .unwrap();
        assert_eq!(exposures_hist.get_histogram().get_sample_count(), 2);
    }

    #[test]
    fn test_noop_does_not_panic() {
        let metrics = PipelineMetrics::noop();
        metrics.accepted(EVENT_TYPE_EXPOSURE).inc();
        metrics.rejected(EVENT_TYPE_METRIC).inc();
        metrics.deduplicated(EVENT_TYPE_REWARD).inc();
        metrics.backpressure(EVENT_TYPE_QOE).inc();
        metrics.publish_latency("exposures").observe(0.001);
    }
}
