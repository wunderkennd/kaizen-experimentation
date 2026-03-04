//! gRPC EventIngestionService implementation.
//!
//! Pattern: validate → dedup → serialize → publish to Kafka.
//! Crash-only: no graceful shutdown. Bloom filter resets on restart (brief dedup gap accepted).

use std::sync::Mutex;

use prost::Message;
use tonic::{Request, Response, Status};
use tracing::{debug, warn};

use experimentation_ingest::dedup::EventDedup;
use experimentation_ingest::validation;
use experimentation_proto::pipeline::event_ingestion_service_server::EventIngestionService;
use experimentation_proto::pipeline::{
    IngestBatchResponse, IngestExposureBatchRequest, IngestExposureRequest,
    IngestExposureResponse, IngestMetricEventBatchRequest, IngestMetricEventRequest,
    IngestMetricEventResponse, IngestQoEEventBatchRequest, IngestQoEEventRequest,
    IngestQoEEventResponse, IngestRewardEventRequest, IngestRewardEventResponse,
};

use crate::kafka::{
    EventProducer, ProduceError, TOPIC_EXPOSURES, TOPIC_METRIC_EVENTS, TOPIC_QOE_EVENTS,
    TOPIC_REWARD_EVENTS,
};

pub struct IngestionServiceImpl {
    producer: EventProducer,
    dedup: Mutex<EventDedup>,
}

impl IngestionServiceImpl {
    pub fn new(producer: EventProducer, dedup: EventDedup) -> Self {
        Self {
            producer,
            dedup: Mutex::new(dedup),
        }
    }

    /// Check dedup filter. Returns true if duplicate.
    fn is_duplicate(&self, event_id: &str) -> bool {
        self.dedup.lock().unwrap().is_duplicate(event_id)
    }
}

fn map_produce_error(e: ProduceError) -> Status {
    match e {
        ProduceError::QueueFull => {
            warn!("Kafka queue full, returning RESOURCE_EXHAUSTED");
            Status::resource_exhausted("Kafka producer queue full, retry later")
        }
        ProduceError::Kafka(msg) => {
            warn!(error = %msg, "Kafka produce failed");
            Status::internal(format!("Kafka error: {msg}"))
        }
    }
}

fn map_validation_error(e: experimentation_core::Error) -> Status {
    Status::invalid_argument(e.to_string())
}

/// Process a single event through the validate → dedup → publish pipeline.
/// Returns Ok(true) if accepted, Ok(false) if duplicate.
async fn process_event<E: Message>(
    svc: &IngestionServiceImpl,
    event_id: &str,
    topic: &str,
    key: &str,
    event: &E,
    validate: impl FnOnce() -> experimentation_core::Result<()>,
) -> Result<bool, Status> {
    validate().map_err(map_validation_error)?;

    if svc.is_duplicate(event_id) {
        debug!(event_id, "Duplicate event rejected by Bloom filter");
        return Ok(false);
    }

    let payload = event.encode_to_vec();
    svc.producer
        .publish(topic, key, &payload)
        .await
        .map_err(map_produce_error)?;

    Ok(true)
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

        let accepted = process_event(
            self,
            &event.event_id,
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
            if validation::validate_exposure(event).is_err() {
                invalid += 1;
                continue;
            }
            if self.is_duplicate(&event.event_id) {
                duplicate += 1;
                continue;
            }
            let payload = event.encode_to_vec();
            self.producer
                .publish(TOPIC_EXPOSURES, &event.experiment_id, &payload)
                .await
                .map_err(map_produce_error)?;
            accepted += 1;
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

        let accepted = process_event(
            self,
            &event.event_id,
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
            if validation::validate_metric_event(event).is_err() {
                invalid += 1;
                continue;
            }
            if self.is_duplicate(&event.event_id) {
                duplicate += 1;
                continue;
            }
            let payload = event.encode_to_vec();
            self.producer
                .publish(TOPIC_METRIC_EVENTS, &event.user_id, &payload)
                .await
                .map_err(map_produce_error)?;
            accepted += 1;
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

        let accepted = process_event(
            self,
            &event.event_id,
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

        let accepted = process_event(
            self,
            &event.event_id,
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
            if validation::validate_qoe_event(event).is_err() {
                invalid += 1;
                continue;
            }
            if self.is_duplicate(&event.event_id) {
                duplicate += 1;
                continue;
            }
            let payload = event.encode_to_vec();
            self.producer
                .publish(TOPIC_QOE_EVENTS, &event.session_id, &payload)
                .await
                .map_err(map_produce_error)?;
            accepted += 1;
        }

        Ok(Response::new(IngestBatchResponse {
            accepted_count: accepted,
            duplicate_count: duplicate,
            invalid_count: invalid,
        }))
    }
}
