//! gRPC EventIngestionService implementation.
//!
//! Pattern: validate → dedup → serialize → publish to Kafka.
//! If Kafka is unreachable (not queue-full), buffer to local disk.
//! Crash-only: no graceful shutdown. Bloom filter resets on restart (brief dedup gap accepted).

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use prost::Message;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use experimentation_ingest::dedup::EventDedup;
use experimentation_ingest::validation;
use experimentation_proto::pipeline::event_ingestion_service_server::EventIngestionService;
use experimentation_proto::pipeline::{
    IngestBatchResponse, IngestExposureBatchRequest, IngestExposureRequest,
    IngestExposureResponse, IngestMetricEventBatchRequest, IngestMetricEventRequest,
    IngestMetricEventResponse, IngestModelRetrainingEventRequest,
    IngestModelRetrainingEventResponse, IngestQoEEventBatchRequest, IngestQoEEventRequest,
    IngestQoEEventResponse, IngestRewardEventRequest, IngestRewardEventResponse,
};

use crate::buffer::{BufferedEvent, DiskBuffer};
use crate::kafka::{
    ProduceError, Producer, HEADER_TRACEPARENT, TOPIC_EXPOSURES, TOPIC_METRIC_EVENTS,
    TOPIC_MODEL_RETRAINING_EVENTS, TOPIC_QOE_EVENTS, TOPIC_REWARD_EVENTS,
};
use crate::metrics::PipelineMetrics;

/// Extract W3C traceparent header from gRPC request metadata.
fn extract_traceparent<T>(request: &Request<T>) -> Option<String> {
    request
        .metadata()
        .get(HEADER_TRACEPARENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

pub struct IngestionServiceImpl {
    producer: Box<dyn Producer>,
    dedup: Mutex<EventDedup>,
    metrics: PipelineMetrics,
    buffer: Mutex<DiskBuffer>,
}

impl IngestionServiceImpl {
    pub fn new(
        producer: impl Producer + 'static,
        dedup: EventDedup,
        metrics: PipelineMetrics,
        buffer: DiskBuffer,
    ) -> Self {
        Self {
            producer: Box::new(producer),
            dedup: Mutex::new(dedup),
            metrics,
            buffer: Mutex::new(buffer),
        }
    }

    /// Replay any buffered events from a previous crash. Call once at startup.
    pub async fn replay_buffer(&self) {
        let events = {
            let buf = self.buffer.lock().unwrap();
            if !buf.has_pending() {
                return;
            }
            match buf.read_all() {
                Ok(events) => events,
                Err(e) => {
                    warn!(error = %e, "Failed to read buffer file for replay");
                    return;
                }
            }
        };

        info!(count = events.len(), "Replaying buffered events to Kafka");
        let mut replayed = 0;
        let mut failed = 0;

        for event in &events {
            let histogram = self.metrics.publish_latency(&event.topic);
            match self
                .producer
                .publish(&event.topic, &event.key, &event.payload, Some(&histogram))
                .await
            {
                Ok(()) => replayed += 1,
                Err(e) => {
                    warn!(error = %e, topic = %event.topic, "Failed to replay buffered event");
                    failed += 1;
                    // If Kafka is still down, stop replay — events stay buffered
                    if e.is_broker_unreachable() {
                        warn!("Kafka still unreachable during replay, aborting. Events remain buffered.");
                        return;
                    }
                }
            }
        }

        if failed == 0 {
            if let Err(e) = self.buffer.lock().unwrap().clear() {
                warn!(error = %e, "Failed to clear buffer after replay");
            }
        }

        info!(replayed, failed, "Buffer replay complete");
    }

    /// Check dedup filter. Returns true if duplicate.
    fn is_duplicate(&self, event_id: &str) -> bool {
        self.dedup.lock().unwrap().is_duplicate(event_id)
    }

    /// Buffer an event to disk when Kafka is unreachable.
    fn buffer_event(&self, topic: &str, key: &str, payload: &[u8]) {
        let event = BufferedEvent {
            topic: topic.to_string(),
            key: key.to_string(),
            payload: payload.to_vec(),
        };
        if let Err(e) = self.buffer.lock().unwrap().append(&event) {
            warn!(error = %e, "Failed to buffer event to disk");
        }
    }
}

fn map_validation_error(e: experimentation_core::Error) -> Status {
    Status::invalid_argument(e.to_string())
}

/// Observe the delay between an event's timestamp and the current server time.
/// Skips observation if the timestamp is missing or in the future (clock skew).
fn observe_ingest_delay(metrics: &PipelineMetrics, event_type: &str, ts: Option<&prost_types::Timestamp>) {
    if let Some(ts) = ts {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let event_secs = ts.seconds as f64 + ts.nanos as f64 / 1_000_000_000.0;
        let delay = now_secs - event_secs;
        if delay >= 0.0 {
            metrics.ingest_delay(event_type).observe(delay);
        }
    }
}

/// Process a single event through the validate → dedup → publish pipeline.
/// Returns Ok(true) if accepted, Ok(false) if duplicate.
/// Forwards the W3C traceparent header (if present) to Kafka for distributed tracing.
#[allow(clippy::too_many_arguments)]
async fn process_event<E: Message>(
    svc: &IngestionServiceImpl,
    event_id: &str,
    event_type: &str,
    topic: &str,
    key: &str,
    event: &E,
    validate: impl FnOnce() -> experimentation_core::Result<()>,
    traceparent: Option<&str>,
) -> Result<bool, Status> {
    // Validate
    if let Err(e) = validate() {
        svc.metrics.rejected(event_type).inc();
        return Err(map_validation_error(e));
    }

    // Dedup
    if svc.is_duplicate(event_id) {
        debug!(event_id, "Duplicate event rejected by Bloom filter");
        svc.metrics.deduplicated(event_type).inc();
        return Ok(false);
    }

    // Publish with event type + traceparent headers for downstream tracing
    let payload = event.encode_to_vec();
    let histogram = svc.metrics.publish_latency(topic);
    match svc
        .producer
        .publish_with_headers(topic, key, &payload, Some(&histogram), Some(event_type), traceparent)
        .await
    {
        Ok(()) => {
            svc.metrics.accepted(event_type).inc();
            Ok(true)
        }
        Err(ProduceError::QueueFull) => {
            warn!("Kafka queue full, returning RESOURCE_EXHAUSTED");
            svc.metrics.backpressure(event_type).inc();
            Err(Status::resource_exhausted(
                "Kafka producer queue full, retry later",
            ))
        }
        Err(ProduceError::Kafka(msg)) => {
            warn!(error = %msg, "Kafka produce failed, buffering to disk");
            svc.buffer_event(topic, key, &payload);
            // Event is buffered — tell the client it was accepted.
            // It will be replayed to Kafka when the broker comes back.
            svc.metrics.accepted(event_type).inc();
            Ok(true)
        }
    }
}

/// Process a batch event through validate → dedup → publish.
/// Returns (accepted_delta, duplicate_delta, invalid_delta).
#[allow(clippy::too_many_arguments)]
async fn process_batch_event<E: Message>(
    svc: &IngestionServiceImpl,
    event_id: &str,
    event_type: &str,
    topic: &str,
    key: &str,
    event: &E,
    validate_result: Result<(), experimentation_core::Error>,
    traceparent: Option<&str>,
) -> Result<(i32, i32, i32), Status> {
    if validate_result.is_err() {
        svc.metrics.rejected(event_type).inc();
        return Ok((0, 0, 1));
    }
    if svc.is_duplicate(event_id) {
        svc.metrics.deduplicated(event_type).inc();
        return Ok((0, 1, 0));
    }

    let payload = event.encode_to_vec();
    let histogram = svc.metrics.publish_latency(topic);
    match svc
        .producer
        .publish_with_headers(topic, key, &payload, Some(&histogram), Some(event_type), traceparent)
        .await
    {
        Ok(()) => {
            svc.metrics.accepted(event_type).inc();
            Ok((1, 0, 0))
        }
        Err(ProduceError::QueueFull) => {
            svc.metrics.backpressure(event_type).inc();
            Err(Status::resource_exhausted(
                "Kafka producer queue full, retry later",
            ))
        }
        Err(ProduceError::Kafka(msg)) => {
            warn!(error = %msg, "Kafka produce failed in batch, buffering");
            svc.buffer_event(topic, key, &payload);
            svc.metrics.accepted(event_type).inc();
            Ok((1, 0, 0))
        }
    }
}

#[tonic::async_trait]
impl EventIngestionService for IngestionServiceImpl {
    async fn ingest_exposure(
        &self,
        request: Request<IngestExposureRequest>,
    ) -> Result<Response<IngestExposureResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let event = request
            .into_inner()
            .event
            .ok_or_else(|| Status::invalid_argument("event is required"))?;

        observe_ingest_delay(&self.metrics, crate::metrics::EVENT_TYPE_EXPOSURE, event.timestamp.as_ref());

        let accepted = process_event(
            self,
            &event.event_id,
            crate::metrics::EVENT_TYPE_EXPOSURE,
            TOPIC_EXPOSURES,
            &event.experiment_id,
            &event,
            || validation::validate_exposure(&event),
            traceparent.as_deref(),
        )
        .await?;

        Ok(Response::new(IngestExposureResponse { accepted }))
    }

    async fn ingest_exposure_batch(
        &self,
        request: Request<IngestExposureBatchRequest>,
    ) -> Result<Response<IngestBatchResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let events = request.into_inner().events;
        let mut accepted = 0i32;
        let mut duplicate = 0i32;
        let mut invalid = 0i32;

        for event in &events {
            let (a, d, i) = process_batch_event(
                self,
                &event.event_id,
                crate::metrics::EVENT_TYPE_EXPOSURE,
                TOPIC_EXPOSURES,
                &event.experiment_id,
                event,
                validation::validate_exposure(event),
                traceparent.as_deref(),
            )
            .await?;
            accepted += a;
            duplicate += d;
            invalid += i;
        }

        Ok(Response::new(IngestBatchResponse {
            accepted_count: accepted,
            duplicate_count: duplicate,
            invalid_count: invalid,
        }))
    }

    async fn ingest_metric_event(
        &self,
        request: Request<IngestMetricEventRequest>,
    ) -> Result<Response<IngestMetricEventResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let event = request
            .into_inner()
            .event
            .ok_or_else(|| Status::invalid_argument("event is required"))?;

        observe_ingest_delay(&self.metrics, crate::metrics::EVENT_TYPE_METRIC, event.timestamp.as_ref());

        let accepted = process_event(
            self,
            &event.event_id,
            crate::metrics::EVENT_TYPE_METRIC,
            TOPIC_METRIC_EVENTS,
            &event.user_id,
            &event,
            || validation::validate_metric_event(&event),
            traceparent.as_deref(),
        )
        .await?;

        Ok(Response::new(IngestMetricEventResponse { accepted }))
    }

    async fn ingest_metric_event_batch(
        &self,
        request: Request<IngestMetricEventBatchRequest>,
    ) -> Result<Response<IngestBatchResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let events = request.into_inner().events;
        let mut accepted = 0i32;
        let mut duplicate = 0i32;
        let mut invalid = 0i32;

        for event in &events {
            let (a, d, i) = process_batch_event(
                self,
                &event.event_id,
                crate::metrics::EVENT_TYPE_METRIC,
                TOPIC_METRIC_EVENTS,
                &event.user_id,
                event,
                validation::validate_metric_event(event),
                traceparent.as_deref(),
            )
            .await?;
            accepted += a;
            duplicate += d;
            invalid += i;
        }

        Ok(Response::new(IngestBatchResponse {
            accepted_count: accepted,
            duplicate_count: duplicate,
            invalid_count: invalid,
        }))
    }

    async fn ingest_reward_event(
        &self,
        request: Request<IngestRewardEventRequest>,
    ) -> Result<Response<IngestRewardEventResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let event = request
            .into_inner()
            .event
            .ok_or_else(|| Status::invalid_argument("event is required"))?;

        observe_ingest_delay(&self.metrics, crate::metrics::EVENT_TYPE_REWARD, event.timestamp.as_ref());

        let accepted = process_event(
            self,
            &event.event_id,
            crate::metrics::EVENT_TYPE_REWARD,
            TOPIC_REWARD_EVENTS,
            &event.experiment_id,
            &event,
            || validation::validate_reward_event(&event),
            traceparent.as_deref(),
        )
        .await?;

        Ok(Response::new(IngestRewardEventResponse { accepted }))
    }

    async fn ingest_qo_e_event(
        &self,
        request: Request<IngestQoEEventRequest>,
    ) -> Result<Response<IngestQoEEventResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let event = request
            .into_inner()
            .event
            .ok_or_else(|| Status::invalid_argument("event is required"))?;

        observe_ingest_delay(&self.metrics, crate::metrics::EVENT_TYPE_QOE, event.timestamp.as_ref());

        let accepted = process_event(
            self,
            &event.event_id,
            crate::metrics::EVENT_TYPE_QOE,
            TOPIC_QOE_EVENTS,
            &event.session_id,
            &event,
            || validation::validate_qoe_event(&event),
            traceparent.as_deref(),
        )
        .await?;

        Ok(Response::new(IngestQoEEventResponse { accepted }))
    }

    async fn ingest_qo_e_event_batch(
        &self,
        request: Request<IngestQoEEventBatchRequest>,
    ) -> Result<Response<IngestBatchResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let events = request.into_inner().events;
        let mut accepted = 0i32;
        let mut duplicate = 0i32;
        let mut invalid = 0i32;

        for event in &events {
            let (a, d, i) = process_batch_event(
                self,
                &event.event_id,
                crate::metrics::EVENT_TYPE_QOE,
                TOPIC_QOE_EVENTS,
                &event.session_id,
                event,
                validation::validate_qoe_event(event),
                traceparent.as_deref(),
            )
            .await?;
            accepted += a;
            duplicate += d;
            invalid += i;
        }

        Ok(Response::new(IngestBatchResponse {
            accepted_count: accepted,
            duplicate_count: duplicate,
            invalid_count: invalid,
        }))
    }

    /// ADR-021: Ingest a ModelRetrainingEvent for feedback loop interference detection.
    ///
    /// Required: event_id, model_id, training_data_start, training_data_end.
    /// Published to model_retraining_events topic. Consumed by M3 for contamination analysis.
    async fn ingest_model_retraining_event(
        &self,
        request: Request<IngestModelRetrainingEventRequest>,
    ) -> Result<Response<IngestModelRetrainingEventResponse>, Status> {
        let traceparent = extract_traceparent(&request);
        let event = request
            .into_inner()
            .event
            .ok_or_else(|| Status::invalid_argument("event is required"))?;

        // ADR-021: Composite dedup key — same model retrained on the same data
        // window is semantically duplicate regardless of caller-supplied event_id.
        let dedup_key = validation::model_retraining_dedup_key(&event)
            .unwrap_or_else(|| event.event_id.clone());

        let accepted = process_event(
            self,
            &dedup_key,
            crate::metrics::EVENT_TYPE_MODEL_RETRAINING,
            TOPIC_MODEL_RETRAINING_EVENTS,
            &event.model_id,
            &event,
            || validation::validate_model_retraining_event(&event),
            traceparent.as_deref(),
        )
        .await?;

        Ok(Response::new(IngestModelRetrainingEventResponse {
            accepted,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferConfig;
    use crate::kafka::ProduceError;
    use crate::metrics::PipelineMetrics;
    use chrono::{Duration, Utc};
    use experimentation_ingest::dedup::{DedupConfig, DedupMetrics, EventDedup};
    use experimentation_proto::common::{
        ExposureEvent, MetricEvent, ModelRetrainingEvent, PlaybackMetrics, QoEEvent, RewardEvent,
    };
    use experimentation_proto::pipeline::{
        IngestExposureBatchRequest, IngestExposureRequest, IngestMetricEventBatchRequest,
        IngestMetricEventRequest, IngestModelRetrainingEventRequest, IngestQoEEventBatchRequest,
        IngestQoEEventRequest, IngestRewardEventRequest,
    };
    use prost::Message;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ---- Mock producer ----

    /// Controls what the mock producer returns on publish calls.
    #[derive(Clone)]
    enum MockBehavior {
        /// Always succeed.
        Ok,
        /// Return QueueFull on every call.
        QueueFull,
        /// Return a Kafka broker error on every call.
        BrokerError,
    }

    struct MockProducer {
        behavior: MockBehavior,
        publish_count: AtomicUsize,
        /// Stores (topic, key) pairs for each successful publish call.
        published: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl MockProducer {
        fn new(behavior: MockBehavior) -> Self {
            Self {
                behavior,
                publish_count: AtomicUsize::new(0),
                published: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.publish_count.load(Ordering::SeqCst)
        }

        fn published_events(&self) -> Vec<(String, String)> {
            self.published.lock().unwrap().clone()
        }
    }

    #[tonic::async_trait]
    impl Producer for MockProducer {
        async fn publish(
            &self,
            topic: &str,
            key: &str,
            _payload: &[u8],
            _latency_histogram: Option<&prometheus::Histogram>,
        ) -> Result<(), ProduceError> {
            self.publish_with_headers(topic, key, _payload, _latency_histogram, None, None)
                .await
        }

        async fn publish_with_headers(
            &self,
            topic: &str,
            key: &str,
            _payload: &[u8],
            _latency_histogram: Option<&prometheus::Histogram>,
            _event_type: Option<&str>,
            _traceparent: Option<&str>,
        ) -> Result<(), ProduceError> {
            self.publish_count.fetch_add(1, Ordering::SeqCst);
            match &self.behavior {
                MockBehavior::Ok => {
                    self.published
                        .lock()
                        .unwrap()
                        .push((topic.to_string(), key.to_string()));
                    Ok(())
                }
                MockBehavior::QueueFull => Err(ProduceError::QueueFull),
                MockBehavior::BrokerError => {
                    Err(ProduceError::Kafka("broker unreachable".to_string()))
                }
            }
        }
    }

    // MockProducer wrapped in Arc so the test can inspect it after handing to the service.
    struct MockProducerHandle {
        inner: Arc<MockProducer>,
    }

    impl MockProducerHandle {
        fn new(behavior: MockBehavior) -> Self {
            Self {
                inner: Arc::new(MockProducer::new(behavior)),
            }
        }

        fn call_count(&self) -> usize {
            self.inner.call_count()
        }

        fn published_events(&self) -> Vec<(String, String)> {
            self.inner.published_events()
        }
    }

    /// A wrapper that delegates to the Arc'd inner producer. This lets us
    /// pass ownership to `IngestionServiceImpl::new` while retaining a handle.
    struct ArcProducer(Arc<MockProducer>);

    #[tonic::async_trait]
    impl Producer for ArcProducer {
        async fn publish(
            &self,
            topic: &str,
            key: &str,
            payload: &[u8],
            hist: Option<&prometheus::Histogram>,
        ) -> Result<(), ProduceError> {
            self.0.publish(topic, key, payload, hist).await
        }

        async fn publish_with_headers(
            &self,
            topic: &str,
            key: &str,
            payload: &[u8],
            hist: Option<&prometheus::Histogram>,
            event_type: Option<&str>,
            traceparent: Option<&str>,
        ) -> Result<(), ProduceError> {
            self.0
                .publish_with_headers(topic, key, payload, hist, event_type, traceparent)
                .await
        }
    }

    // ---- Test fixtures ----

    fn test_dedup() -> EventDedup {
        let config = DedupConfig {
            items_per_interval: 1000,
            fp_rate: 0.001,
            rotation_interval_secs: 3600,
        };
        EventDedup::with_config(config, DedupMetrics::noop())
    }

    fn test_buffer(dir: &std::path::Path) -> DiskBuffer {
        DiskBuffer::new(BufferConfig {
            dir: dir.to_path_buf(),
            max_size_bytes: 1024 * 1024,
        })
        .unwrap()
    }

    fn build_service(handle: &MockProducerHandle, dir: &std::path::Path) -> IngestionServiceImpl {
        IngestionServiceImpl::new(
            ArcProducer(Arc::clone(&handle.inner)),
            test_dedup(),
            PipelineMetrics::noop(),
            test_buffer(dir),
        )
    }

    fn now_proto() -> Option<prost_types::Timestamp> {
        let now = Utc::now();
        Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        })
    }

    fn valid_exposure() -> ExposureEvent {
        ExposureEvent {
            event_id: "evt-1".into(),
            experiment_id: "exp-1".into(),
            user_id: "user-1".into(),
            variant_id: "control".into(),
            timestamp: now_proto(),
            assignment_probability: 0.5,
            ..Default::default()
        }
    }

    fn valid_metric_event() -> MetricEvent {
        MetricEvent {
            event_id: "met-1".into(),
            user_id: "user-1".into(),
            event_type: "play_start".into(),
            value: 42.0,
            timestamp: now_proto(),
            ..Default::default()
        }
    }

    fn valid_reward_event() -> RewardEvent {
        RewardEvent {
            event_id: "rew-1".into(),
            experiment_id: "exp-1".into(),
            user_id: "user-1".into(),
            arm_id: "arm-a".into(),
            reward: 0.85,
            timestamp: now_proto(),
            ..Default::default()
        }
    }

    fn valid_qoe_event() -> QoEEvent {
        QoEEvent {
            event_id: "qoe-1".into(),
            session_id: "sess-1".into(),
            content_id: "movie-1".into(),
            user_id: "user-1".into(),
            metrics: Some(PlaybackMetrics {
                time_to_first_frame_ms: 250,
                rebuffer_count: 1,
                rebuffer_ratio: 0.02,
                avg_bitrate_kbps: 5000,
                resolution_switches: 2,
                peak_resolution_height: 1080,
                startup_failure_rate: 0.0,
                playback_duration_ms: 60000,
            }),
            timestamp: now_proto(),
            ..Default::default()
        }
    }

    // ---- Exposure tests ----

    #[tokio::test]
    async fn test_ingest_valid_exposure() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let resp = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(valid_exposure()),
            }))
            .await
            .unwrap();

        assert!(resp.into_inner().accepted);
        assert_eq!(handle.call_count(), 1);
        let published = handle.published_events();
        assert_eq!(published[0].0, "exposures");
        assert_eq!(published[0].1, "exp-1"); // keyed by experiment_id
    }

    #[tokio::test]
    async fn test_ingest_exposure_missing_event_field() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let result = svc
            .ingest_exposure(Request::new(IngestExposureRequest { event: None }))
            .await;

        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("event is required"));
        assert_eq!(handle.call_count(), 0);
    }

    #[tokio::test]
    async fn test_ingest_exposure_missing_experiment_id() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_exposure();
        event.experiment_id = String::new();
        let result = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
        assert_eq!(handle.call_count(), 0);
    }

    #[tokio::test]
    async fn test_ingest_exposure_duplicate_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        // First call: accepted
        let resp1 = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(valid_exposure()),
            }))
            .await
            .unwrap();
        assert!(resp1.into_inner().accepted);

        // Second call with same event_id: duplicate
        let resp2 = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(valid_exposure()),
            }))
            .await
            .unwrap();
        assert!(!resp2.into_inner().accepted);

        // Only one publish call (first event)
        assert_eq!(handle.call_count(), 1);
    }

    #[tokio::test]
    async fn test_ingest_exposure_queue_full() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::QueueFull);
        let svc = build_service(&handle, dir.path());

        let result = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(valid_exposure()),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::ResourceExhausted);
    }

    #[tokio::test]
    async fn test_ingest_exposure_broker_error_buffers_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::BrokerError);
        let svc = build_service(&handle, dir.path());

        let resp = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(valid_exposure()),
            }))
            .await
            .unwrap();

        // Accepted (buffered to disk for later replay)
        assert!(resp.into_inner().accepted);
        assert_eq!(handle.call_count(), 1);

        // Verify event was buffered
        let buffer = svc.buffer.lock().unwrap();
        assert!(buffer.has_pending());
        let events = buffer.read_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].topic, "exposures");
        assert_eq!(events[0].key, "exp-1");
    }

    // ---- Metric event tests ----

    #[tokio::test]
    async fn test_ingest_valid_metric_event() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let resp = svc
            .ingest_metric_event(Request::new(IngestMetricEventRequest {
                event: Some(valid_metric_event()),
            }))
            .await
            .unwrap();

        assert!(resp.into_inner().accepted);
        let published = handle.published_events();
        assert_eq!(published[0].0, "metric_events");
        assert_eq!(published[0].1, "user-1"); // keyed by user_id
    }

    #[tokio::test]
    async fn test_ingest_metric_event_missing_event_type() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_metric_event();
        event.event_type = String::new();
        let result = svc
            .ingest_metric_event(Request::new(IngestMetricEventRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    // ---- Reward event tests ----

    #[tokio::test]
    async fn test_ingest_valid_reward_event() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let resp = svc
            .ingest_reward_event(Request::new(IngestRewardEventRequest {
                event: Some(valid_reward_event()),
            }))
            .await
            .unwrap();

        assert!(resp.into_inner().accepted);
        let published = handle.published_events();
        assert_eq!(published[0].0, "reward_events");
        assert_eq!(published[0].1, "exp-1"); // keyed by experiment_id
    }

    #[tokio::test]
    async fn test_ingest_reward_event_missing_arm_id() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_reward_event();
        event.arm_id = String::new();
        let result = svc
            .ingest_reward_event(Request::new(IngestRewardEventRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    // ---- QoE event tests ----

    #[tokio::test]
    async fn test_ingest_valid_qoe_event() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let resp = svc
            .ingest_qo_e_event(Request::new(IngestQoEEventRequest {
                event: Some(valid_qoe_event()),
            }))
            .await
            .unwrap();

        assert!(resp.into_inner().accepted);
        let published = handle.published_events();
        assert_eq!(published[0].0, "qoe_events");
        assert_eq!(published[0].1, "sess-1"); // keyed by session_id
    }

    #[tokio::test]
    async fn test_ingest_qoe_event_missing_metrics() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_qoe_event();
        event.metrics = None;
        let result = svc
            .ingest_qo_e_event(Request::new(IngestQoEEventRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    // ---- Batch tests ----

    #[tokio::test]
    async fn test_ingest_exposure_batch_mixed() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let valid1 = valid_exposure();
        let mut valid2 = valid_exposure();
        valid2.event_id = "evt-2".into();
        valid2.experiment_id = "exp-2".into();

        let mut invalid = valid_exposure();
        invalid.experiment_id = String::new(); // invalid

        let resp = svc
            .ingest_exposure_batch(Request::new(IngestExposureBatchRequest {
                events: vec![valid1.clone(), invalid, valid2],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.accepted_count, 2);
        assert_eq!(resp.invalid_count, 1);
        assert_eq!(resp.duplicate_count, 0);
    }

    #[tokio::test]
    async fn test_ingest_exposure_batch_with_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let event = valid_exposure();
        let mut different = valid_exposure();
        different.event_id = "evt-2".into();

        // Batch with 3 events: 2 unique + 1 duplicate of first
        let resp = svc
            .ingest_exposure_batch(Request::new(IngestExposureBatchRequest {
                events: vec![event.clone(), different, event],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.accepted_count, 2);
        assert_eq!(resp.duplicate_count, 1);
        assert_eq!(resp.invalid_count, 0);
    }

    #[tokio::test]
    async fn test_ingest_metric_event_batch() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event1 = valid_metric_event();
        event1.event_id = "met-1".into();
        let mut event2 = valid_metric_event();
        event2.event_id = "met-2".into();

        let resp = svc
            .ingest_metric_event_batch(Request::new(IngestMetricEventBatchRequest {
                events: vec![event1, event2],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.accepted_count, 2);
        assert_eq!(handle.call_count(), 2);
    }

    #[tokio::test]
    async fn test_ingest_qoe_event_batch_mixed() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let valid = valid_qoe_event();
        let mut invalid = valid_qoe_event();
        invalid.event_id = "qoe-2".into();
        invalid.metrics = None; // invalid

        let resp = svc
            .ingest_qo_e_event_batch(Request::new(IngestQoEEventBatchRequest {
                events: vec![valid, invalid],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.accepted_count, 1);
        assert_eq!(resp.invalid_count, 1);
    }

    // ---- Cross-event dedup tests ----

    #[tokio::test]
    async fn test_dedup_works_across_event_types() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        // First: ingest exposure with event_id "shared-id"
        let mut exposure = valid_exposure();
        exposure.event_id = "shared-id".into();
        let resp = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(exposure),
            }))
            .await
            .unwrap();
        assert!(resp.into_inner().accepted);

        // Second: try to ingest metric event with same event_id
        let mut metric = valid_metric_event();
        metric.event_id = "shared-id".into();
        let resp = svc
            .ingest_metric_event(Request::new(IngestMetricEventRequest {
                event: Some(metric),
            }))
            .await
            .unwrap();
        // Should be rejected as duplicate (Bloom filter is global)
        assert!(!resp.into_inner().accepted);

        assert_eq!(handle.call_count(), 1); // only the exposure was published
    }

    // ---- Timestamp validation through service ----

    #[tokio::test]
    async fn test_ingest_exposure_old_timestamp_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_exposure();
        let old = Utc::now() - Duration::hours(25);
        event.timestamp = Some(prost_types::Timestamp {
            seconds: old.timestamp(),
            nanos: 0,
        });

        let result = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
        assert_eq!(handle.call_count(), 0);
    }

    // ---- Buffer replay tests ----

    #[tokio::test]
    async fn test_replay_buffer_publishes_buffered_events() {
        let dir = tempfile::tempdir().unwrap();

        // Pre-populate buffer with events from a "previous crash"
        {
            let mut buffer = DiskBuffer::new(BufferConfig {
                dir: dir.path().to_path_buf(),
                max_size_bytes: 1024 * 1024,
            })
            .unwrap();
            buffer
                .append(&BufferedEvent {
                    topic: "exposures".into(),
                    key: "exp-1".into(),
                    payload: vec![1, 2, 3],
                })
                .unwrap();
            buffer
                .append(&BufferedEvent {
                    topic: "metric_events".into(),
                    key: "user-1".into(),
                    payload: vec![4, 5, 6],
                })
                .unwrap();
        }

        // Create service with the same buffer dir (simulating restart)
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        // Buffer should have pending events from "previous crash"
        assert!(svc.buffer.lock().unwrap().has_pending());

        svc.replay_buffer().await;

        // Both events should have been replayed
        assert_eq!(handle.call_count(), 2);

        // Buffer should be cleared after successful replay
        assert!(!svc.buffer.lock().unwrap().has_pending());
    }

    #[tokio::test]
    async fn test_replay_buffer_stops_on_broker_unreachable() {
        let dir = tempfile::tempdir().unwrap();

        // Pre-populate buffer
        {
            let mut buffer = DiskBuffer::new(BufferConfig {
                dir: dir.path().to_path_buf(),
                max_size_bytes: 1024 * 1024,
            })
            .unwrap();
            for i in 0..5 {
                buffer
                    .append(&BufferedEvent {
                        topic: "exposures".into(),
                        key: format!("key-{i}"),
                        payload: vec![i as u8],
                    })
                    .unwrap();
            }
        }

        // Broker is unreachable — replay should abort and keep buffer
        let handle = MockProducerHandle::new(MockBehavior::BrokerError);
        let svc = build_service(&handle, dir.path());
        svc.replay_buffer().await;

        // First event attempted, then aborted (broker unreachable)
        assert_eq!(handle.call_count(), 1);

        // Buffer should still have pending events
        assert!(svc.buffer.lock().unwrap().has_pending());
    }

    // ---- Metrics verification ----

    #[tokio::test]
    async fn test_metrics_updated_on_ingest() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let registry = prometheus::Registry::new();
        let metrics = PipelineMetrics::new(&registry);

        let svc = IngestionServiceImpl::new(
            ArcProducer(Arc::clone(&handle.inner)),
            test_dedup(),
            metrics.clone(),
            test_buffer(dir.path()),
        );

        // Ingest a valid exposure
        svc.ingest_exposure(Request::new(IngestExposureRequest {
            event: Some(valid_exposure()),
        }))
        .await
        .unwrap();

        assert_eq!(
            metrics.accepted("exposure").get(),
            1,
            "accepted counter should increment"
        );

        // Ingest an invalid exposure
        let mut invalid = valid_exposure();
        invalid.event_id = "evt-bad".into();
        invalid.experiment_id = String::new();
        let _ = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(invalid),
            }))
            .await;

        assert_eq!(
            metrics.rejected("exposure").get(),
            1,
            "rejected counter should increment"
        );

        // Ingest a duplicate
        let _ = svc
            .ingest_exposure(Request::new(IngestExposureRequest {
                event: Some(valid_exposure()),
            }))
            .await;

        assert_eq!(
            metrics.deduplicated("exposure").get(),
            1,
            "deduplicated counter should increment"
        );
    }

    // ---- All four event types publish to correct topics ----

    #[tokio::test]
    async fn test_all_event_types_route_to_correct_topics() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        // Exposure → exposures topic
        svc.ingest_exposure(Request::new(IngestExposureRequest {
            event: Some(valid_exposure()),
        }))
        .await
        .unwrap();

        // Metric → metric_events topic
        svc.ingest_metric_event(Request::new(IngestMetricEventRequest {
            event: Some(valid_metric_event()),
        }))
        .await
        .unwrap();

        // Reward → reward_events topic
        svc.ingest_reward_event(Request::new(IngestRewardEventRequest {
            event: Some(valid_reward_event()),
        }))
        .await
        .unwrap();

        // QoE → qoe_events topic
        svc.ingest_qo_e_event(Request::new(IngestQoEEventRequest {
            event: Some(valid_qoe_event()),
        }))
        .await
        .unwrap();

        let published = handle.published_events();
        assert_eq!(published.len(), 4);
        assert_eq!(published[0].0, "exposures");
        assert_eq!(published[1].0, "metric_events");
        assert_eq!(published[2].0, "reward_events");
        assert_eq!(published[3].0, "qoe_events");
    }

    // ---- ModelRetrainingEvent tests (ADR-021) ----

    fn past_proto(offset_hours: i64) -> Option<prost_types::Timestamp> {
        let t = Utc::now() - Duration::hours(offset_hours);
        Some(prost_types::Timestamp {
            seconds: t.timestamp(),
            nanos: 0,
        })
    }

    fn valid_model_retraining_event() -> ModelRetrainingEvent {
        ModelRetrainingEvent {
            event_id: "mre-1".into(),
            model_id: "rec-model-v2".into(),
            training_data_start: past_proto(48),
            training_data_end: past_proto(24),
            active_experiment_ids: vec!["exp-1".into(), "exp-2".into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_ingest_valid_model_retraining_event() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let resp = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(valid_model_retraining_event()),
            }))
            .await
            .unwrap();

        assert!(resp.into_inner().accepted);
        assert_eq!(handle.call_count(), 1);
        let published = handle.published_events();
        assert_eq!(published[0].0, "model_retraining_events");
        assert_eq!(published[0].1, "rec-model-v2"); // keyed by model_id
    }

    #[tokio::test]
    async fn test_ingest_model_retraining_missing_model_id() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_model_retraining_event();
        event.model_id = String::new();
        let result = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
        assert_eq!(handle.call_count(), 0);
    }

    #[tokio::test]
    async fn test_ingest_model_retraining_missing_training_data_start() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_model_retraining_event();
        event.training_data_start = None;
        let result = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_ingest_model_retraining_missing_training_data_end() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let mut event = valid_model_retraining_event();
        event.training_data_end = None;
        let result = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(event),
            }))
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_ingest_model_retraining_duplicate_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        // First call: accepted
        let resp1 = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(valid_model_retraining_event()),
            }))
            .await
            .unwrap();
        assert!(resp1.into_inner().accepted);

        // Second call with same model_id+training_data_start: duplicate (composite key).
        let resp2 = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(valid_model_retraining_event()),
            }))
            .await
            .unwrap();
        assert!(!resp2.into_inner().accepted);
        assert_eq!(handle.call_count(), 1);
    }

    /// Composite key dedup: a different event_id for the same model+window is still a duplicate.
    #[tokio::test]
    async fn test_ingest_model_retraining_composite_key_dedup() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        let first = valid_model_retraining_event(); // event_id = "mre-1"
        let mut second = valid_model_retraining_event();
        second.event_id = "mre-different-id".into(); // Different event_id, same model+window

        // First: accepted
        let resp1 = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(first),
            }))
            .await
            .unwrap();
        assert!(resp1.into_inner().accepted);

        // Second: same model_id + training_data_start → rejected even though event_id differs
        let resp2 = svc
            .ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
                event: Some(second),
            }))
            .await
            .unwrap();
        assert!(
            !resp2.into_inner().accepted,
            "same model+window with different event_id must be deduplicated (ADR-021 composite key)"
        );
        assert_eq!(handle.call_count(), 1, "only one event must reach Kafka");
    }

    /// Contract test: Kafka roundtrip serialization (M2 producer → M3 consumer wire format).
    ///
    /// Verifies that a ModelRetrainingEvent can be serialized to protobuf bytes (as published
    /// to the model_retraining_events Kafka topic by M2) and deserialized back faithfully
    /// by a consumer (M3 feedback loop contamination pipeline). This is the wire-format
    /// contract between M2 and M3 per ADR-021.
    #[test]
    fn test_model_retraining_event_kafka_roundtrip_serialization() {
        let original = valid_model_retraining_event();

        // Simulate M2 producer: encode to protobuf bytes (what goes onto Kafka)
        let bytes = original.encode_to_vec();
        assert!(!bytes.is_empty(), "serialized payload must not be empty");

        // Simulate M3 consumer: decode from protobuf bytes
        let decoded = ModelRetrainingEvent::decode(bytes.as_slice())
            .expect("M3 must be able to decode ModelRetrainingEvent from Kafka payload");

        // Wire format contract assertions
        assert_eq!(decoded.event_id, original.event_id);
        assert_eq!(decoded.model_id, original.model_id);
        assert_eq!(
            decoded.training_data_start,
            original.training_data_start,
            "training_data_start must survive Kafka roundtrip"
        );
        assert_eq!(
            decoded.training_data_end,
            original.training_data_end,
            "training_data_end must survive Kafka roundtrip"
        );
        assert_eq!(
            decoded.active_experiment_ids,
            original.active_experiment_ids,
            "active_experiment_ids must survive Kafka roundtrip"
        );
    }

    /// Contract test: model_retraining_events routes to the correct Kafka topic.
    #[tokio::test]
    async fn test_model_retraining_event_routes_to_correct_topic() {
        let dir = tempfile::tempdir().unwrap();
        let handle = MockProducerHandle::new(MockBehavior::Ok);
        let svc = build_service(&handle, dir.path());

        svc.ingest_model_retraining_event(Request::new(IngestModelRetrainingEventRequest {
            event: Some(valid_model_retraining_event()),
        }))
        .await
        .unwrap();

        let published = handle.published_events();
        assert_eq!(
            published[0].0, "model_retraining_events",
            "ModelRetrainingEvent must be published to model_retraining_events topic (ADR-021)"
        );
    }
}
