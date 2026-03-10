//! gRPC server for the AnalysisService (M4a).
//!
//! `GetInterferenceAnalysis` is fully wired through Delta Lake → experimentation-stats.
//! Other RPCs return `Unimplemented` until their data pipelines are connected.

use crate::config::AnalysisConfig;
use crate::delta_reader;
use experimentation_proto::experimentation::analysis::v1::analysis_service_server::{
    AnalysisService, AnalysisServiceServer,
};
use experimentation_proto::experimentation::analysis::v1::{
    AnalysisResult, GetAnalysisResultRequest, GetInterferenceAnalysisRequest,
    GetInterleavingAnalysisRequest, GetNoveltyAnalysisRequest, InterferenceAnalysisResult,
    InterleavingAnalysisResult, NoveltyAnalysisResult, RunAnalysisRequest, TitleSpillover,
};
use experimentation_stats::interference;
use tonic::{Request, Response, Status};
use tracing::info;

/// gRPC handler for the analysis service.
#[derive(Clone)]
pub struct AnalysisServiceHandler {
    config: AnalysisConfig,
}

impl AnalysisServiceHandler {
    pub fn new(config: AnalysisConfig) -> Self {
        Self { config }
    }
}

/// Convert a Rust `InterferenceAnalysisResult` to the proto equivalent.
fn to_proto_interference_result(
    experiment_id: &str,
    result: &interference::InterferenceAnalysisResult,
) -> InterferenceAnalysisResult {
    let now = chrono::Utc::now();
    let seconds = now.timestamp();
    let nanos = now.timestamp_subsec_nanos() as i32;

    InterferenceAnalysisResult {
        experiment_id: experiment_id.to_string(),
        interference_detected: result.interference_detected,
        jensen_shannon_divergence: result.jensen_shannon_divergence,
        jaccard_similarity_top_100: result.jaccard_similarity_top_100,
        treatment_gini_coefficient: result.treatment_gini_coefficient,
        control_gini_coefficient: result.control_gini_coefficient,
        treatment_catalog_coverage: result.treatment_catalog_coverage,
        control_catalog_coverage: result.control_catalog_coverage,
        spillover_titles: result
            .spillover_titles
            .iter()
            .map(|s| TitleSpillover {
                content_id: s.content_id.clone(),
                treatment_watch_rate: s.treatment_watch_rate,
                control_watch_rate: s.control_watch_rate,
                p_value: s.p_value,
            })
            .collect(),
        computed_at: Some(prost_types::Timestamp { seconds, nanos }),
    }
}

#[tonic::async_trait]
impl AnalysisService for AnalysisServiceHandler {
    async fn run_analysis(
        &self,
        _request: Request<RunAnalysisRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        Err(Status::unimplemented("not yet available"))
    }

    async fn get_analysis_result(
        &self,
        _request: Request<GetAnalysisResultRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        Err(Status::unimplemented("not yet available"))
    }

    async fn get_interleaving_analysis(
        &self,
        _request: Request<GetInterleavingAnalysisRequest>,
    ) -> Result<Response<InterleavingAnalysisResult>, Status> {
        Err(Status::unimplemented("not yet available"))
    }

    async fn get_novelty_analysis(
        &self,
        _request: Request<GetNoveltyAnalysisRequest>,
    ) -> Result<Response<NoveltyAnalysisResult>, Status> {
        Err(Status::unimplemented("not yet available"))
    }

    async fn get_interference_analysis(
        &self,
        request: Request<GetInterferenceAnalysisRequest>,
    ) -> Result<Response<InterferenceAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let input = delta_reader::read_content_consumption(
            &self.config.delta_lake_path,
            &experiment_id,
        )
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("no content_consumption data") {
                Status::not_found(msg)
            } else if msg.contains("only") && msg.contains("variant") {
                Status::failed_precondition(msg)
            } else {
                Status::internal(msg)
            }
        })?;

        let result = interference::analyze_interference(
            &input,
            self.config.default_alpha,
            self.config.default_js_threshold,
        )
        .map_err(|e| Status::internal(format!("analysis failed: {e}")))?;

        let proto_result = to_proto_interference_result(&experiment_id, &result);
        Ok(Response::new(proto_result))
    }
}

/// Start the gRPC server serving the AnalysisService.
pub async fn serve_grpc(config: AnalysisConfig) -> Result<(), String> {
    let addr = config
        .grpc_addr
        .parse()
        .map_err(|e| format!("invalid gRPC address '{}': {e}", config.grpc_addr))?;

    let handler = AnalysisServiceHandler::new(config);

    info!(%addr, "gRPC server starting");

    tonic::transport::Server::builder()
        .add_service(tonic_web::enable(AnalysisServiceServer::new(handler)))
        .serve(addr)
        .await
        .map_err(|e| format!("gRPC server error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result_no_spillover() -> interference::InterferenceAnalysisResult {
        interference::InterferenceAnalysisResult {
            interference_detected: false,
            jensen_shannon_divergence: 0.01,
            jaccard_similarity_top_100: 0.95,
            treatment_gini_coefficient: 0.3,
            control_gini_coefficient: 0.32,
            treatment_catalog_coverage: 0.8,
            control_catalog_coverage: 0.75,
            spillover_titles: vec![],
        }
    }

    fn sample_result_with_spillover() -> interference::InterferenceAnalysisResult {
        interference::InterferenceAnalysisResult {
            interference_detected: true,
            jensen_shannon_divergence: 0.12,
            jaccard_similarity_top_100: 0.65,
            treatment_gini_coefficient: 0.45,
            control_gini_coefficient: 0.30,
            treatment_catalog_coverage: 0.70,
            control_catalog_coverage: 0.85,
            spillover_titles: vec![
                interference::TitleSpillover {
                    content_id: "movie-42".into(),
                    treatment_watch_rate: 0.15,
                    control_watch_rate: 0.05,
                    p_value: 0.001,
                },
                interference::TitleSpillover {
                    content_id: "movie-99".into(),
                    treatment_watch_rate: 0.08,
                    control_watch_rate: 0.02,
                    p_value: 0.01,
                },
            ],
        }
    }

    #[test]
    fn test_to_proto_interference_no_spillover() {
        let result = sample_result_no_spillover();
        let proto = to_proto_interference_result("exp-1", &result);

        assert_eq!(proto.experiment_id, "exp-1");
        assert!(!proto.interference_detected);
        assert!(proto.spillover_titles.is_empty());
        assert!(proto.computed_at.is_some());
    }

    #[test]
    fn test_to_proto_interference_with_spillover() {
        let result = sample_result_with_spillover();
        let proto = to_proto_interference_result("exp-2", &result);

        assert!(proto.interference_detected);
        assert_eq!(proto.spillover_titles.len(), 2);
        assert_eq!(proto.spillover_titles[0].content_id, "movie-42");
        assert_eq!(proto.spillover_titles[1].content_id, "movie-99");
    }

    #[test]
    fn test_to_proto_all_fields_mapped() {
        let result = sample_result_with_spillover();
        let proto = to_proto_interference_result("exp-3", &result);

        assert_eq!(proto.jensen_shannon_divergence, 0.12);
        assert_eq!(proto.jaccard_similarity_top_100, 0.65);
        assert_eq!(proto.treatment_gini_coefficient, 0.45);
        assert_eq!(proto.control_gini_coefficient, 0.30);
        assert_eq!(proto.treatment_catalog_coverage, 0.70);
        assert_eq!(proto.control_catalog_coverage, 0.85);

        let spill = &proto.spillover_titles[0];
        assert_eq!(spill.treatment_watch_rate, 0.15);
        assert_eq!(spill.control_watch_rate, 0.05);
        assert_eq!(spill.p_value, 0.001);
    }

    #[tokio::test]
    async fn test_unimplemented_run_analysis() {
        let handler = AnalysisServiceHandler::new(AnalysisConfig {
            grpc_addr: "[::1]:0".into(),
            delta_lake_path: "/tmp/nonexistent".into(),
            default_alpha: 0.05,
            default_js_threshold: 0.05,
        });

        let err = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn test_unimplemented_get_analysis_result() {
        let handler = AnalysisServiceHandler::new(AnalysisConfig {
            grpc_addr: "[::1]:0".into(),
            delta_lake_path: "/tmp/nonexistent".into(),
            default_alpha: 0.05,
            default_js_threshold: 0.05,
        });

        let err = handler
            .get_analysis_result(Request::new(GetAnalysisResultRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn test_unimplemented_get_interleaving_analysis() {
        let handler = AnalysisServiceHandler::new(AnalysisConfig {
            grpc_addr: "[::1]:0".into(),
            delta_lake_path: "/tmp/nonexistent".into(),
            default_alpha: 0.05,
            default_js_threshold: 0.05,
        });

        let err = handler
            .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn test_unimplemented_get_novelty_analysis() {
        let handler = AnalysisServiceHandler::new(AnalysisConfig {
            grpc_addr: "[::1]:0".into(),
            delta_lake_path: "/tmp/nonexistent".into(),
            default_alpha: 0.05,
            default_js_threshold: 0.05,
        });

        let err = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn test_interference_empty_experiment_id() {
        let handler = AnalysisServiceHandler::new(AnalysisConfig {
            grpc_addr: "[::1]:0".into(),
            delta_lake_path: "/tmp/nonexistent".into(),
            default_alpha: 0.05,
            default_js_threshold: 0.05,
        });

        let err = handler
            .get_interference_analysis(Request::new(GetInterferenceAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }
}
