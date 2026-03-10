//! gRPC server for the AnalysisService (M4a).
//!
//! All 5 RPCs are wired through Delta Lake → experimentation-stats → proto conversion.

use crate::config::AnalysisConfig;
use crate::delta_reader;
use experimentation_proto::experimentation::analysis::v1::analysis_service_server::{
    AnalysisService, AnalysisServiceServer,
};
use experimentation_proto::experimentation::analysis::v1::{
    AlgorithmStrength as ProtoAlgorithmStrength, AnalysisResult, GetAnalysisResultRequest,
    GetInterferenceAnalysisRequest, GetInterleavingAnalysisRequest, GetNoveltyAnalysisRequest,
    InterferenceAnalysisResult, InterleavingAnalysisResult, MetricResult, NoveltyAnalysisResult,
    PositionAnalysis as ProtoPositionAnalysis, RunAnalysisRequest, SrmResult as ProtoSrmResult,
    TitleSpillover,
};
use experimentation_stats::{cuped, interference, interleaving, novelty, srm, ttest};
use std::collections::HashMap;
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

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn now_timestamp() -> prost_types::Timestamp {
    let now = chrono::Utc::now();
    prost_types::Timestamp {
        seconds: now.timestamp(),
        nanos: now.timestamp_subsec_nanos() as i32,
    }
}

fn map_reader_error(e: anyhow::Error) -> Status {
    let msg = e.to_string();
    if msg.contains("not found") || msg.contains("data found") {
        Status::not_found(msg)
    } else if msg.contains("only") && msg.contains("variant") {
        Status::failed_precondition(msg)
    } else {
        Status::internal(msg)
    }
}

fn map_stats_error(e: impl std::fmt::Display) -> Status {
    let msg = e.to_string();
    if msg.contains("must") || msg.contains("need at least") || msg.contains("alpha") {
        Status::failed_precondition(msg)
    } else {
        Status::internal(format!("analysis failed: {msg}"))
    }
}

// ---------------------------------------------------------------------------
// Proto converters
// ---------------------------------------------------------------------------

fn to_proto_interference_result(
    experiment_id: &str,
    result: &interference::InterferenceAnalysisResult,
) -> InterferenceAnalysisResult {
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
        computed_at: Some(now_timestamp()),
    }
}

fn to_proto_srm_result(srm: &srm::SrmResult) -> ProtoSrmResult {
    let total: u64 = srm.observed.values().sum();
    let total_f = total as f64;

    ProtoSrmResult {
        chi_squared: srm.chi_squared,
        p_value: srm.p_value,
        is_mismatch: srm.is_mismatch,
        observed_counts: srm
            .observed
            .iter()
            .map(|(k, &v)| (k.clone(), v as i64))
            .collect(),
        expected_counts: srm
            .expected
            .iter()
            .map(|(k, &frac)| (k.clone(), (frac * total_f).round() as i64))
            .collect(),
    }
}

fn to_proto_interleaving_result(
    experiment_id: &str,
    result: &interleaving::InterleavingAnalysisResult,
) -> InterleavingAnalysisResult {
    InterleavingAnalysisResult {
        experiment_id: experiment_id.to_string(),
        algorithm_win_rates: result.algorithm_win_rates.clone(),
        sign_test_p_value: result.sign_test_p_value,
        algorithm_strengths: result
            .algorithm_strengths
            .iter()
            .map(|s| ProtoAlgorithmStrength {
                algorithm_id: s.algorithm_id.clone(),
                strength: s.strength,
                ci_lower: s.ci_lower,
                ci_upper: s.ci_upper,
            })
            .collect(),
        position_analyses: result
            .position_analyses
            .iter()
            .map(|p| ProtoPositionAnalysis {
                position: p.position as i32,
                algorithm_engagement_rates: p.algorithm_engagement_rates.clone(),
            })
            .collect(),
        computed_at: Some(now_timestamp()),
    }
}

fn to_proto_novelty_result(
    experiment_id: &str,
    metric_id: &str,
    result: &novelty::NoveltyAnalysisResult,
) -> NoveltyAnalysisResult {
    NoveltyAnalysisResult {
        experiment_id: experiment_id.to_string(),
        metric_id: metric_id.to_string(),
        novelty_detected: result.novelty_detected,
        raw_treatment_effect: result.raw_treatment_effect,
        projected_steady_state_effect: result.projected_steady_state_effect,
        novelty_amplitude: result.novelty_amplitude,
        decay_constant_days: result.decay_constant_days,
        is_stabilized: result.is_stabilized,
        days_until_projected_stability: result.days_until_projected_stability,
        computed_at: Some(now_timestamp()),
    }
}

// ---------------------------------------------------------------------------
// Core analysis computation (shared by run_analysis and get_analysis_result)
// ---------------------------------------------------------------------------

async fn compute_analysis(
    config: &AnalysisConfig,
    experiment_id: &str,
) -> Result<AnalysisResult, Status> {
    let data = delta_reader::read_metric_summaries(&config.delta_lake_path, experiment_id)
        .await
        .map_err(map_reader_error)?;

    // Identify control variant: one named "control", or first alphabetically.
    let control_variant = if data.variant_user_counts.contains_key("control") {
        "control".to_string()
    } else {
        let mut variants: Vec<&String> = data.variant_user_counts.keys().collect();
        variants.sort();
        variants[0].clone()
    };

    // SRM check: uniform expected fractions.
    let n_variants = data.variant_user_counts.len() as f64;
    let expected_fractions: HashMap<String, f64> = data
        .variant_user_counts
        .keys()
        .map(|k| (k.clone(), 1.0 / n_variants))
        .collect();

    let srm_result = srm::srm_check(&data.variant_user_counts, &expected_fractions, 0.001)
        .map_err(map_stats_error)?;

    let alpha = config.default_alpha;

    // Per-metric, per-treatment-variant analysis.
    let mut metric_results = Vec::new();

    let mut metric_ids: Vec<&String> = data.metrics.keys().collect();
    metric_ids.sort();

    for metric_id in metric_ids {
        let variant_data = &data.metrics[metric_id];

        let control_tuples = match variant_data.get(&control_variant) {
            Some(v) => v,
            None => continue,
        };

        let control_values: Vec<f64> = control_tuples.iter().map(|(v, _)| *v).collect();

        let mut variant_ids: Vec<&String> = variant_data.keys().collect();
        variant_ids.sort();

        for variant_id in variant_ids {
            if *variant_id == control_variant {
                continue;
            }

            let treatment_tuples = &variant_data[variant_id];
            let treatment_values: Vec<f64> = treatment_tuples.iter().map(|(v, _)| *v).collect();

            let ttest_result =
                ttest::welch_ttest(&control_values, &treatment_values, alpha)
                    .map_err(map_stats_error)?;

            let relative_effect = if ttest_result.control_mean.abs() > 1e-10 {
                ttest_result.effect / ttest_result.control_mean.abs()
            } else {
                0.0
            };

            // CUPED: only if all observations in both groups have non-null covariates.
            let control_covs: Vec<f64> = control_tuples
                .iter()
                .filter_map(|(_, c)| *c)
                .collect();
            let treatment_covs: Vec<f64> = treatment_tuples
                .iter()
                .filter_map(|(_, c)| *c)
                .collect();

            let (cuped_effect, cuped_ci_lower, cuped_ci_upper, variance_reduction_pct) =
                if control_covs.len() == control_values.len()
                    && treatment_covs.len() == treatment_values.len()
                    && control_covs.len() >= 2
                    && treatment_covs.len() >= 2
                {
                    match cuped::cuped_adjust(
                        &control_values,
                        &treatment_values,
                        &control_covs,
                        &treatment_covs,
                        alpha,
                    ) {
                        Ok(cr) => (
                            cr.adjusted_effect,
                            cr.ci_lower,
                            cr.ci_upper,
                            cr.variance_reduction * 100.0,
                        ),
                        Err(_) => (0.0, 0.0, 0.0, 0.0),
                    }
                } else {
                    (0.0, 0.0, 0.0, 0.0)
                };

            metric_results.push(MetricResult {
                metric_id: metric_id.clone(),
                variant_id: variant_id.clone(),
                control_mean: ttest_result.control_mean,
                treatment_mean: ttest_result.treatment_mean,
                absolute_effect: ttest_result.effect,
                relative_effect,
                ci_lower: ttest_result.ci_lower,
                ci_upper: ttest_result.ci_upper,
                p_value: ttest_result.p_value,
                is_significant: ttest_result.is_significant,
                cuped_adjusted_effect: cuped_effect,
                cuped_ci_lower,
                cuped_ci_upper,
                variance_reduction_pct,
                sequential_result: None,
                segment_results: vec![],
                session_level_result: None,
            });
        }
    }

    Ok(AnalysisResult {
        experiment_id: experiment_id.to_string(),
        metric_results,
        srm_result: Some(to_proto_srm_result(&srm_result)),
        surrogate_projections: vec![],
        cochran_q_p_value: 0.0,
        computed_at: Some(now_timestamp()),
    })
}

// ---------------------------------------------------------------------------
// gRPC trait implementation
// ---------------------------------------------------------------------------

#[tonic::async_trait]
impl AnalysisService for AnalysisServiceHandler {
    async fn run_analysis(
        &self,
        request: Request<RunAnalysisRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let result = compute_analysis(&self.config, &experiment_id).await?;
        Ok(Response::new(result))
    }

    async fn get_analysis_result(
        &self,
        request: Request<GetAnalysisResultRequest>,
    ) -> Result<Response<AnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }
        let result = compute_analysis(&self.config, &experiment_id).await?;
        Ok(Response::new(result))
    }

    async fn get_interleaving_analysis(
        &self,
        request: Request<GetInterleavingAnalysisRequest>,
    ) -> Result<Response<InterleavingAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let scores = delta_reader::read_interleaving_scores(
            &self.config.delta_lake_path,
            &experiment_id,
        )
        .await
        .map_err(map_reader_error)?;

        let result = interleaving::analyze_interleaving(&scores, self.config.default_alpha)
            .map_err(map_stats_error)?;

        Ok(Response::new(to_proto_interleaving_result(
            &experiment_id,
            &result,
        )))
    }

    async fn get_novelty_analysis(
        &self,
        request: Request<GetNoveltyAnalysisRequest>,
    ) -> Result<Response<NoveltyAnalysisResult>, Status> {
        let experiment_id = request.into_inner().experiment_id;
        if experiment_id.is_empty() {
            return Err(Status::invalid_argument("experiment_id is required"));
        }

        let (metric_id, effects) = delta_reader::read_daily_treatment_effects(
            &self.config.delta_lake_path,
            &experiment_id,
        )
        .await
        .map_err(map_reader_error)?;

        let result = novelty::analyze_novelty(&effects, self.config.default_alpha)
            .map_err(map_stats_error)?;

        Ok(Response::new(to_proto_novelty_result(
            &experiment_id,
            &metric_id,
            &result,
        )))
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
        .map_err(map_reader_error)?;

        let result = interference::analyze_interference(
            &input,
            self.config.default_alpha,
            self.config.default_js_threshold,
        )
        .map_err(|e| Status::internal(format!("analysis failed: {e}")))?;

        Ok(Response::new(to_proto_interference_result(
            &experiment_id,
            &result,
        )))
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
    use deltalake::arrow::array::{
        builder::{Float64Builder, MapBuilder, StringBuilder},
        Array, Date32Array, Float64Array, Int64Array, StringArray,
    };
    use deltalake::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
    use deltalake::arrow::record_batch::RecordBatch;
    use deltalake::DeltaOps;
    use std::sync::Arc;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Test data helpers
    // -----------------------------------------------------------------------

    fn test_config(path: &str) -> AnalysisConfig {
        AnalysisConfig {
            grpc_addr: "[::1]:0".into(),
            delta_lake_path: path.into(),
            default_alpha: 0.05,
            default_js_threshold: 0.05,
        }
    }

    async fn write_table(dir: &std::path::Path, name: &str, batch: RecordBatch) {
        let table_path = dir.join(name);
        std::fs::create_dir_all(&table_path).unwrap();
        let ops = DeltaOps::try_from_uri(table_path.to_str().unwrap())
            .await
            .unwrap();
        ops.write(vec![batch]).await.unwrap();
    }

    fn metric_summaries_schema() -> Arc<ArrowSchema> {
        Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("variant_id", DataType::Utf8, false),
            Field::new("metric_id", DataType::Utf8, false),
            Field::new("metric_value", DataType::Float64, false),
            Field::new("cuped_covariate", DataType::Float64, true),
        ]))
    }

    fn make_analysis_data(
        exp_ids: &[&str],
        user_ids: &[&str],
        variant_ids: &[&str],
        metric_ids: &[&str],
        values: &[f64],
        covariates: &[Option<f64>],
    ) -> RecordBatch {
        let cov_arr: Float64Array = covariates.iter().copied().collect();
        RecordBatch::try_new(
            metric_summaries_schema(),
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(StringArray::from(variant_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Float64Array::from(values.to_vec())),
                Arc::new(cov_arr),
            ],
        )
        .unwrap()
    }

    fn make_interleaving_data(
        exp_ids: &[&str],
        user_ids: &[&str],
        algo_scores: &[Vec<(&str, f64)>],
        winners: &[Option<&str>],
        engagements: &[i64],
    ) -> RecordBatch {
        let mut map_builder = MapBuilder::new(None, StringBuilder::new(), Float64Builder::new());
        for row_scores in algo_scores {
            for &(k, v) in row_scores {
                map_builder.keys().append_value(k);
                map_builder.values().append_value(v);
            }
            map_builder.append(true).unwrap();
        }
        let map_arr = map_builder.finish();
        let winner_arr: StringArray = winners.iter().copied().collect();
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("algorithm_scores", map_arr.data_type().clone(), false),
            Field::new("winning_algorithm_id", DataType::Utf8, true),
            Field::new("total_engagements", DataType::Int64, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(map_arr),
                Arc::new(winner_arr),
                Arc::new(Int64Array::from(engagements.to_vec())),
            ],
        )
        .unwrap()
    }

    fn make_daily_effects_data(
        exp_ids: &[&str],
        metric_ids: &[&str],
        dates: &[i32],
        effects: &[f64],
        sizes: &[i64],
    ) -> RecordBatch {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("metric_id", DataType::Utf8, false),
            Field::new("effect_date", DataType::Date32, false),
            Field::new("absolute_effect", DataType::Float64, false),
            Field::new("sample_size", DataType::Int64, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(exp_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Date32Array::from(dates.to_vec())),
                Arc::new(Float64Array::from(effects.to_vec())),
                Arc::new(Int64Array::from(sizes.to_vec())),
            ],
        )
        .unwrap()
    }

    // -----------------------------------------------------------------------
    // Proto converter tests (kept from original)
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // RunAnalysis / GetAnalysisResult tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_analysis_basic() {
        let tmp = TempDir::new().unwrap();
        // 5 control users + 5 treatment users, metric "ctr", clear effect
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control", "control", "control", "control", "control",
            "treatment", "treatment", "treatment", "treatment", "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates);
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert_eq!(result.metric_results.len(), 1);

        let mr = &result.metric_results[0];
        assert_eq!(mr.metric_id, "ctr");
        assert_eq!(mr.variant_id, "treatment");
        assert!((mr.control_mean - 3.0).abs() < 1e-10);
        assert!((mr.treatment_mean - 13.0).abs() < 1e-10);
        assert!((mr.absolute_effect - 10.0).abs() < 1e-10);
        assert!(mr.is_significant); // large effect
        assert!(mr.p_value < 0.05);
        assert!(result.srm_result.is_some());
    }

    #[tokio::test]
    async fn test_run_analysis_with_cuped() {
        let tmp = TempDir::new().unwrap();
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control", "control", "control", "control", "control",
            "treatment", "treatment", "treatment", "treatment", "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        // Highly correlated covariate and outcome
        let values: Vec<f64> = (0..10).map(|i| (i as f64) * 2.0 + if i >= 5 { 3.0 } else { 1.0 }).collect();
        let covariates: Vec<Option<f64>> = (0..10).map(|i| Some(i as f64)).collect();

        let batch = make_analysis_data(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates);
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let mr = &resp.into_inner().metric_results[0];
        // CUPED should be populated since all covariates are present
        assert!(mr.variance_reduction_pct > 0.0, "CUPED should reduce variance, got {}", mr.variance_reduction_pct);
    }

    #[tokio::test]
    async fn test_run_analysis_srm() {
        let tmp = TempDir::new().unwrap();
        // 50/50 split → no SRM
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control", "control", "control", "control", "control",
            "treatment", "treatment", "treatment", "treatment", "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 1.1, 2.1, 3.1, 4.1, 5.1];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates);
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let resp = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let srm = resp.into_inner().srm_result.unwrap();
        assert!(!srm.is_mismatch, "50/50 split should not trigger SRM");
        assert_eq!(srm.observed_counts.len(), 2);
        assert_eq!(srm.expected_counts.len(), 2);
    }

    #[tokio::test]
    async fn test_run_analysis_empty_id() {
        let handler = AnalysisServiceHandler::new(test_config("/tmp/nonexistent"));
        let err = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_run_analysis_not_found() {
        let tmp = TempDir::new().unwrap();
        let batch = make_analysis_data(
            &["exp-1"], &["u1"], &["control"], &["ctr"], &[1.0], &[None],
        );
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let err = handler
            .run_analysis(Request::new(RunAnalysisRequest {
                experiment_id: "exp-999".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_get_analysis_result_delegates() {
        let tmp = TempDir::new().unwrap();
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
        let variant_ids: Vec<&str> = vec![
            "control", "control", "control", "control", "control",
            "treatment", "treatment", "treatment", "treatment", "treatment",
        ];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let covariates: Vec<Option<f64>> = vec![None; n];

        let batch = make_analysis_data(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates);
        write_table(tmp.path(), "metric_summaries", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let resp = handler
            .get_analysis_result(Request::new(GetAnalysisResultRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert_eq!(result.metric_results.len(), 1);
        assert!(result.srm_result.is_some());
    }

    // -----------------------------------------------------------------------
    // GetInterleavingAnalysis tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_interleaving_basic() {
        let tmp = TempDir::new().unwrap();
        // 10 scores: 7 wins for algo_a, 3 for algo_b → sign test should detect
        let n = 10;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let user_ids: Vec<&str> = (0..n).map(|i| match i {
            0 => "u0", 1 => "u1", 2 => "u2", 3 => "u3", 4 => "u4",
            5 => "u5", 6 => "u6", 7 => "u7", 8 => "u8", _ => "u9",
        }).collect();
        let algo_scores: Vec<Vec<(&str, f64)>> = (0..n)
            .map(|i| {
                if i < 7 {
                    vec![("algo_a", 3.0), ("algo_b", 1.0)]
                } else {
                    vec![("algo_a", 1.0), ("algo_b", 3.0)]
                }
            })
            .collect();
        let winners: Vec<Option<&str>> = (0..n)
            .map(|i| if i < 7 { Some("algo_a") } else { Some("algo_b") })
            .collect();
        let engagements: Vec<i64> = vec![4; n];

        let batch = make_interleaving_data(&exp_ids, &user_ids, &algo_scores, &winners, &engagements);
        write_table(tmp.path(), "interleaving_scores", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let resp = handler
            .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert!(result.algorithm_win_rates.contains_key("algo_a"));
        assert!(result.algorithm_win_rates.contains_key("algo_b"));
        assert!(result.algorithm_win_rates["algo_a"] > result.algorithm_win_rates["algo_b"]);
        assert!(!result.algorithm_strengths.is_empty());
        assert!(!result.position_analyses.is_empty());
        assert!(result.computed_at.is_some());
    }

    #[tokio::test]
    async fn test_interleaving_empty_id() {
        let handler = AnalysisServiceHandler::new(test_config("/tmp/nonexistent"));
        let err = handler
            .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_interleaving_not_found() {
        let tmp = TempDir::new().unwrap();
        let batch = make_interleaving_data(
            &["exp-1"],
            &["u1"],
            &[vec![("algo_a", 3.0), ("algo_b", 1.0)]],
            &[Some("algo_a")],
            &[4],
        );
        write_table(tmp.path(), "interleaving_scores", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let err = handler
            .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
                experiment_id: "exp-999".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    // -----------------------------------------------------------------------
    // GetNoveltyAnalysis tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_novelty_basic() {
        let tmp = TempDir::new().unwrap();
        // 15 days of decaying effect: s=5, a=3, d=4
        let n = 15;
        let base_date = 19700i32;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let dates: Vec<i32> = (0..n as i32).map(|i| base_date + i).collect();
        let effects: Vec<f64> = (0..n).map(|i| 5.0 + 3.0 * (-(i as f64) / 4.0).exp()).collect();
        let sizes: Vec<i64> = vec![1000; n];

        let batch = make_daily_effects_data(&exp_ids, &metric_ids, &dates, &effects, &sizes);
        write_table(tmp.path(), "daily_treatment_effects", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let resp = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap();

        let result = resp.into_inner();
        assert_eq!(result.experiment_id, "exp-1");
        assert_eq!(result.metric_id, "ctr");
        assert!(result.novelty_detected);
        assert!(result.decay_constant_days > 0.0);
        assert!(result.computed_at.is_some());
    }

    #[tokio::test]
    async fn test_novelty_empty_id() {
        let handler = AnalysisServiceHandler::new(test_config("/tmp/nonexistent"));
        let err = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_novelty_not_found() {
        let tmp = TempDir::new().unwrap();
        let batch = make_daily_effects_data(
            &["exp-1"], &["ctr"], &[19700], &[5.0], &[1000],
        );
        write_table(tmp.path(), "daily_treatment_effects", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let err = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "exp-999".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_novelty_insufficient_days() {
        let tmp = TempDir::new().unwrap();
        // Only 5 days → stats crate returns error (needs ≥ 7)
        let n = 5;
        let base_date = 19700i32;
        let exp_ids: Vec<&str> = vec!["exp-1"; n];
        let metric_ids: Vec<&str> = vec!["ctr"; n];
        let dates: Vec<i32> = (0..n as i32).map(|i| base_date + i).collect();
        let effects: Vec<f64> = vec![5.0; n];
        let sizes: Vec<i64> = vec![1000; n];

        let batch = make_daily_effects_data(&exp_ids, &metric_ids, &dates, &effects, &sizes);
        write_table(tmp.path(), "daily_treatment_effects", batch).await;

        let handler = AnalysisServiceHandler::new(test_config(tmp.path().to_str().unwrap()));
        let err = handler
            .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
                experiment_id: "exp-1".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    }

    // -----------------------------------------------------------------------
    // GetInterferenceAnalysis tests (kept from original)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_interference_empty_experiment_id() {
        let handler = AnalysisServiceHandler::new(test_config("/tmp/nonexistent"));
        let err = handler
            .get_interference_analysis(Request::new(GetInterferenceAnalysisRequest {
                experiment_id: "".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }
}
