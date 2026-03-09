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
    IngestMetricEventResponse, IngestQoEEventBatchRequest, IngestQoEEventRequest,
    IngestQoEEventResponse, IngestRewardEventRequest, IngestRewardEventResponse,
};

use crate::buffer::{BufferedEvent, DiskBuffer};
use crate::kafka::{
    EventProducer, ProduceError, TOPIC_EXPOSURES, TOPIC_METRIC_EVENTS, TOPIC_QOE_EVENTS,
    TOPIC_REWARD_EVENTS,
};
use crate::metrics::PipelineMetrics;

pub struct IngestionServiceImpl {
    producer: EventProducer,
    dedup: Mutex<EventDedup>,
    metrics: PipelineMetrics,
    buffer: Mutex<DiskBuffer>,
}

impl IngestionServiceImpl {
    pub fn new(
        producer: EventProducer,
        dedup: EventDedup,
        metrics: PipelineMetrics,
        buffer: DiskBuffer,
    ) -> Self {
        Self {
            producer,
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
async fn process_event<E: Message>(
    svc: &IngestionServiceImpl,
    event_id: &str,
    event_type: &str,
    topic: &str,
    key: &str,
    event: &E,
    validate: impl FnOnce() -> experimentation_core::Result<()>,
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

    // Publish with event type header for downstream latency tracing
    let payload = event.encode_to_vec();
    let histogram = svc.metrics.publish_latency(topic);
    match svc
        .producer
        .publish_with_event_type(topic, key, &payload, Some(&histogram), Some(event_type))
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
async fn process_batch_event<E: Message>(
    svc: &IngestionServiceImpl,
    event_id: &str,
    event_type: &str,
    topic: &str,
    key: &str,
    event: &E,
    validate_result: Result<(), experimentation_core::Error>,
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
        .publish_with_event_type(topic, key, &payload, Some(&histogram), Some(event_type))
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
        )
        .await?;

        Ok(Response::new(IngestExposureResponse { accepted }))
    }

    async fn ingest_exposure_batch(
        &self,
        request: Request<IngestExposureBatchRequest>,
    ) -> Result<Response<IngestBatchResponse>, Status> {
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
        )
        .await?;

        Ok(Response::new(IngestMetricEventResponse { accepted }))
    }

    async fn ingest_metric_event_batch(
        &self,
        request: Request<IngestMetricEventBatchRequest>,
    ) -> Result<Response<IngestBatchResponse>, Status> {
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
        )
        .await?;

        Ok(Response::new(IngestRewardEventResponse { accepted }))
    }

    async fn ingest_qo_e_event(
        &self,
        request: Request<IngestQoEEventRequest>,
    ) -> Result<Response<IngestQoEEventResponse>, Status> {
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
        )
        .await?;

        Ok(Response::new(IngestQoEEventResponse { accepted }))
    }

    async fn ingest_qo_e_event_batch(
        &self,
        request: Request<IngestQoEEventBatchRequest>,
    ) -> Result<Response<IngestBatchResponse>, Status> {
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
}
