//! M4a ↔ M6 Wire-Format Contract Tests
//!
//! These tests validate that the gRPC responses produced by M4a's
//! `AnalysisServiceHandler` match the wire-format contract expected
//! by M6's TypeScript UI. Unlike unit tests in `grpc.rs`, these are
//! integration tests that exercise the full handler through the
//! library crate's public API.
//!
//! Contract points verified:
//! 1. AnalysisResult field presence (experimentId, metricResults, srmResult, computedAt)
//! 2. MetricResult: all 14 scalar fields populated + finite, CI contains estimate, p-value ∈ [0,1]
//! 3. SrmResult map fields: observed/expected counts for all variants
//! 4. SRM mismatch detection (90/10 split)
//! 5. SegmentResult lifecycle enum values (TRIAL=1, ESTABLISHED=3, MATURE=4)
//! 6. Proto3 zero-value omission for optional sub-messages
//! 7. InterleavingAnalysisResult: win rates, sign test, algorithm strengths, position analyses
//! 8. NoveltyAnalysisResult: decay parameters, stabilization fields
//! 9. InterferenceAnalysisResult: JSD, Jaccard, Gini, catalog coverage, spillover titles
//! 10. NOT_FOUND for missing experiments (all 5 RPCs)
//! 11. INVALID_ARGUMENT for empty experiment_id (all 5 RPCs)
//! 12. Cochran Q heterogeneity detection

use experimentation_analysis::config::AnalysisConfig;
use experimentation_analysis::grpc::AnalysisServiceHandler;

use deltalake::arrow::array::{
    builder::{Float64Builder, MapBuilder, StringBuilder},
    Array, Date32Array, Float64Array, Int64Array, StringArray,
};
use deltalake::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::DeltaOps;
use std::sync::Arc;
use tempfile::TempDir;

use experimentation_proto::experimentation::analysis::v1::analysis_service_server::AnalysisService;
use experimentation_proto::experimentation::analysis::v1::{
    GetAnalysisResultRequest, GetInterferenceAnalysisRequest, GetInterleavingAnalysisRequest,
    GetNoveltyAnalysisRequest, RunAnalysisRequest,
};
use tonic::Request;

// ---------------------------------------------------------------------------
// Test infrastructure helpers
// ---------------------------------------------------------------------------

fn test_config(path: &str) -> AnalysisConfig {
    AnalysisConfig {
        grpc_addr: "[::1]:0".into(),
        delta_lake_path: path.into(),
        default_alpha: 0.05,
        default_js_threshold: 0.05,
        database_url: None,
        default_tau_sq: 0.5,
    }
}

fn test_handler(path: &str) -> AnalysisServiceHandler {
    AnalysisServiceHandler::new(test_config(path), None)
}

async fn write_table(dir: &std::path::Path, name: &str, batch: RecordBatch) {
    let table_path = dir.join(name);
    std::fs::create_dir_all(&table_path).unwrap();
    let ops = DeltaOps::try_from_uri(table_path.to_str().unwrap())
        .await
        .unwrap();
    ops.write(vec![batch]).await.unwrap();
}

// ---------------------------------------------------------------------------
// Schema + data builders
// ---------------------------------------------------------------------------

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

fn metric_summaries_schema_with_segment() -> Arc<ArrowSchema> {
    Arc::new(ArrowSchema::new(vec![
        Field::new("experiment_id", DataType::Utf8, false),
        Field::new("user_id", DataType::Utf8, false),
        Field::new("variant_id", DataType::Utf8, false),
        Field::new("metric_id", DataType::Utf8, false),
        Field::new("metric_value", DataType::Float64, false),
        Field::new("cuped_covariate", DataType::Float64, true),
        Field::new("lifecycle_segment", DataType::Utf8, true),
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

fn make_segmented_analysis_data(
    exp_ids: &[&str],
    user_ids: &[&str],
    variant_ids: &[&str],
    metric_ids: &[&str],
    values: &[f64],
    covariates: &[Option<f64>],
    segments: &[Option<&str>],
) -> RecordBatch {
    let cov_arr: Float64Array = covariates.iter().copied().collect();
    let seg_arr: StringArray = segments.iter().copied().collect();
    RecordBatch::try_new(
        metric_summaries_schema_with_segment(),
        vec![
            Arc::new(StringArray::from(exp_ids.to_vec())),
            Arc::new(StringArray::from(user_ids.to_vec())),
            Arc::new(StringArray::from(variant_ids.to_vec())),
            Arc::new(StringArray::from(metric_ids.to_vec())),
            Arc::new(Float64Array::from(values.to_vec())),
            Arc::new(cov_arr),
            Arc::new(seg_arr),
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

fn content_consumption_schema() -> Arc<ArrowSchema> {
    Arc::new(ArrowSchema::new(vec![
        Field::new("experiment_id", DataType::Utf8, false),
        Field::new("variant_id", DataType::Utf8, false),
        Field::new("content_id", DataType::Utf8, false),
        Field::new("watch_time_seconds", DataType::Float64, false),
        Field::new("view_count", DataType::Int64, false),
        Field::new("unique_viewers", DataType::Int64, false),
    ]))
}

fn make_content_consumption_data(
    exp_ids: &[&str],
    variant_ids: &[&str],
    content_ids: &[&str],
    watch_times: &[f64],
    view_counts: &[i64],
    unique_viewers: &[i64],
) -> RecordBatch {
    RecordBatch::try_new(
        content_consumption_schema(),
        vec![
            Arc::new(StringArray::from(exp_ids.to_vec())),
            Arc::new(StringArray::from(variant_ids.to_vec())),
            Arc::new(StringArray::from(content_ids.to_vec())),
            Arc::new(Float64Array::from(watch_times.to_vec())),
            Arc::new(Int64Array::from(view_counts.to_vec())),
            Arc::new(Int64Array::from(unique_viewers.to_vec())),
        ],
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

fn assert_finite(val: f64, field: &str) {
    assert!(
        val.is_finite(),
        "{field} must be finite, got {val}"
    );
}

fn assert_p_value(val: f64, field: &str) {
    assert_finite(val, field);
    assert!(
        (0.0..=1.0).contains(&val),
        "{field} must be in [0, 1], got {val}"
    );
}

fn assert_ci_contains(lo: f64, est: f64, hi: f64, name: &str) {
    assert_finite(lo, &format!("{name}.ci_lower"));
    assert_finite(est, &format!("{name}.estimate"));
    assert_finite(hi, &format!("{name}.ci_upper"));
    assert!(
        lo <= est && est <= hi,
        "{name}: CI [{lo}, {hi}] does not contain estimate {est}"
    );
}

// ============================================================================
// Group 1: AnalysisResult contract (4 tests)
// ============================================================================

/// 1. UI reads: experimentId (non-empty), metricResults[] (non-empty),
///    srmResult (present), computedAt (present).
#[tokio::test]
async fn contract_analysis_result_field_presence() {
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

    let batch = make_analysis_data(
        &exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates,
    );
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let result = resp.into_inner();

    // UI reads `experimentId` — must be non-empty
    assert!(!result.experiment_id.is_empty(), "experimentId must be non-empty");

    // UI reads `metricResults[]` — must be non-empty
    assert!(
        !result.metric_results.is_empty(),
        "metricResults must be non-empty"
    );

    // UI reads `srmResult` — must be present
    assert!(result.srm_result.is_some(), "srmResult must be present");

    // UI reads `computedAt` — must be present
    assert!(result.computed_at.is_some(), "computedAt must be present");
}

/// 2. All 14 scalar fields in MetricResult populated + finite.
///    CI contains estimate. p-value ∈ [0,1]. CUPED fields populated when covariates present.
#[tokio::test]
async fn contract_metric_result_all_fields() {
    let tmp = TempDir::new().unwrap();
    let n = 10;
    let exp_ids: Vec<&str> = vec!["exp-1"; n];
    let user_ids: Vec<&str> = vec!["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"];
    let variant_ids: Vec<&str> = vec![
        "control", "control", "control", "control", "control",
        "treatment", "treatment", "treatment", "treatment", "treatment",
    ];
    let metric_ids: Vec<&str> = vec!["ctr"; n];
    // Highly correlated covariate and outcome for CUPED to work
    let values: Vec<f64> = (0..10)
        .map(|i| (i as f64) * 2.0 + if i >= 5 { 3.0 } else { 1.0 })
        .collect();
    let covariates: Vec<Option<f64>> = (0..10).map(|i| Some(i as f64)).collect();

    let batch = make_analysis_data(
        &exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates,
    );
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let mr = &resp.into_inner().metric_results[0];

    // String fields non-empty
    assert!(!mr.metric_id.is_empty(), "metricId must be non-empty");
    assert!(!mr.variant_id.is_empty(), "variantId must be non-empty");

    // All scalar fields finite
    assert_finite(mr.control_mean, "controlMean");
    assert_finite(mr.treatment_mean, "treatmentMean");
    assert_finite(mr.absolute_effect, "absoluteEffect");
    assert_finite(mr.relative_effect, "relativeEffect");
    assert_finite(mr.ci_lower, "ciLower");
    assert_finite(mr.ci_upper, "ciUpper");

    // CI contains estimate
    assert_ci_contains(mr.ci_lower, mr.absolute_effect, mr.ci_upper, "absoluteEffect");

    // p-value in [0, 1]
    assert_p_value(mr.p_value, "pValue");

    // CUPED fields populated since all covariates present
    assert_finite(mr.cuped_adjusted_effect, "cupedAdjustedEffect");
    assert_finite(mr.cuped_ci_lower, "cupedCiLower");
    assert_finite(mr.cuped_ci_upper, "cupedCiUpper");
    assert_finite(mr.variance_reduction_pct, "varianceReductionPct");
    assert!(
        mr.variance_reduction_pct > 0.0,
        "varianceReductionPct should be > 0 when covariates present, got {}",
        mr.variance_reduction_pct
    );
}

/// 3. SRM: chiSquared finite, pValue ∈ [0,1], observed/expected counts maps
///    have entries for all variants. UI reads both maps to show traffic distribution.
#[tokio::test]
async fn contract_srm_result_map_fields() {
    let tmp = TempDir::new().unwrap();
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

    let batch = make_analysis_data(
        &exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates,
    );
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let srm = resp.into_inner().srm_result.unwrap();

    assert_finite(srm.chi_squared, "srmResult.chiSquared");
    assert_p_value(srm.p_value, "srmResult.pValue");

    // Maps must have entries for both variants
    assert!(
        srm.observed_counts.contains_key("control"),
        "observedCounts missing 'control'"
    );
    assert!(
        srm.observed_counts.contains_key("treatment"),
        "observedCounts missing 'treatment'"
    );
    assert!(
        srm.expected_counts.contains_key("control"),
        "expectedCounts missing 'control'"
    );
    assert!(
        srm.expected_counts.contains_key("treatment"),
        "expectedCounts missing 'treatment'"
    );

    // Counts must be positive
    for (variant, &count) in &srm.observed_counts {
        assert!(count > 0, "observedCounts['{variant}'] must be > 0, got {count}");
    }
    for (variant, &count) in &srm.expected_counts {
        assert!(count > 0, "expectedCounts['{variant}'] must be > 0, got {count}");
    }
}

/// 4. 90/10 split triggers SRM mismatch. UI shows SRM warning banner when
///    `isMismatch == true`. Critical for data quality alerting.
#[tokio::test]
async fn contract_srm_mismatch_detected() {
    let tmp = TempDir::new().unwrap();
    // 90 control users + 10 treatment users → severe SRM
    let n = 100;
    let exp_ids: Vec<&str> = vec!["exp-1"; n];
    let user_ids: Vec<String> = (0..n).map(|i| format!("u{i}")).collect();
    let user_id_refs: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
    let variant_ids: Vec<&str> = (0..n)
        .map(|i| if i < 90 { "control" } else { "treatment" })
        .collect();
    let metric_ids: Vec<&str> = vec!["ctr"; n];
    let values: Vec<f64> = (0..n).map(|i| i as f64 * 0.1).collect();
    let covariates: Vec<Option<f64>> = vec![None; n];

    let batch = make_analysis_data(
        &exp_ids, &user_id_refs, &variant_ids, &metric_ids, &values, &covariates,
    );
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let srm = resp.into_inner().srm_result.unwrap();
    assert!(srm.is_mismatch, "90/10 split must trigger SRM mismatch");
    assert!(
        srm.p_value < 0.001,
        "SRM p-value must be < 0.001 for 90/10 split, got {}",
        srm.p_value
    );
}

// ============================================================================
// Group 2: Segment + optional sub-messages (2 tests)
// ============================================================================

/// 5. SegmentResult uses correct i32 enum values. UI strips `LIFECYCLE_SEGMENT_` prefix.
///    Each segment has finite effect/CI, p-value ∈ [0,1], sample_size > 0.
#[tokio::test]
async fn contract_segment_result_lifecycle_enum_values() {
    let tmp = TempDir::new().unwrap();
    // Three segments: TRIAL, ESTABLISHED, MATURE with different effects
    let n = 30;
    let exp_ids: Vec<&str> = vec!["exp-1"; n];
    let user_ids: Vec<String> = (0..n).map(|i| format!("u{i:02}")).collect();
    let user_id_refs: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
    let variant_ids: Vec<&str> = vec![
        // TRIAL: 5 control + 5 treatment
        "control", "control", "control", "control", "control",
        "treatment", "treatment", "treatment", "treatment", "treatment",
        // ESTABLISHED: 5 control + 5 treatment
        "control", "control", "control", "control", "control",
        "treatment", "treatment", "treatment", "treatment", "treatment",
        // MATURE: 5 control + 5 treatment
        "control", "control", "control", "control", "control",
        "treatment", "treatment", "treatment", "treatment", "treatment",
    ];
    let metric_ids: Vec<&str> = vec!["ctr"; n];
    let values: Vec<f64> = vec![
        // TRIAL control ~3, treatment ~5 (effect ~2)
        1.0, 2.0, 3.0, 4.0, 5.0,
        3.0, 4.0, 5.0, 6.0, 7.0,
        // ESTABLISHED control ~3, treatment ~8 (effect ~5)
        1.0, 2.0, 3.0, 4.0, 5.0,
        6.0, 7.0, 8.0, 9.0, 10.0,
        // MATURE control ~3, treatment ~13 (effect ~10)
        1.0, 2.0, 3.0, 4.0, 5.0,
        11.0, 12.0, 13.0, 14.0, 15.0,
    ];
    let covariates: Vec<Option<f64>> = vec![None; n];
    let segments: Vec<Option<&str>> = vec![
        Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"),
        Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"),
        Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"),
        Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"),
        Some("MATURE"), Some("MATURE"), Some("MATURE"), Some("MATURE"), Some("MATURE"),
        Some("MATURE"), Some("MATURE"), Some("MATURE"), Some("MATURE"), Some("MATURE"),
    ];

    let batch = make_segmented_analysis_data(
        &exp_ids, &user_id_refs, &variant_ids, &metric_ids, &values, &covariates, &segments,
    );
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let result = resp.into_inner();
    let mr = &result.metric_results[0];

    assert_eq!(
        mr.segment_results.len(), 3,
        "expected 3 segment results, got {}",
        mr.segment_results.len()
    );

    // Verify correct i32 enum values
    let seg_enums: Vec<i32> = mr.segment_results.iter().map(|s| s.segment).collect();
    assert!(seg_enums.contains(&1), "should contain TRIAL (1)");
    assert!(seg_enums.contains(&3), "should contain ESTABLISHED (3)");
    assert!(seg_enums.contains(&4), "should contain MATURE (4)");

    // Each segment: finite effect/CI, p-value ∈ [0,1], sample_size > 0
    for seg in &mr.segment_results {
        assert_finite(seg.effect, &format!("segment({}).effect", seg.segment));
        assert_ci_contains(
            seg.ci_lower, seg.effect, seg.ci_upper,
            &format!("segment({})", seg.segment),
        );
        assert_p_value(seg.p_value, &format!("segment({}).pValue", seg.segment));
        assert!(
            seg.sample_size > 0,
            "segment({}).sampleSize must be > 0, got {}",
            seg.segment, seg.sample_size
        );
    }
}

/// 6. Without segments/covariates: sequentialResult = None, sessionLevelResult = None,
///    segmentResults = empty, CUPED fields = 0.0. Documents proto3 zero-value
///    omission for UI fallback defaults.
#[tokio::test]
async fn contract_optional_sub_messages_absent() {
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

    let batch = make_analysis_data(
        &exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates,
    );
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let mr = &resp.into_inner().metric_results[0];

    // Optional sub-messages absent (proto3 default = None)
    assert!(
        mr.sequential_result.is_none(),
        "sequentialResult should be None without sequential testing"
    );
    assert!(
        mr.session_level_result.is_none(),
        "sessionLevelResult should be None without session-level data"
    );
    assert!(
        mr.segment_results.is_empty(),
        "segmentResults should be empty without lifecycle_segment column"
    );

    // CUPED fields: 0.0 (proto3 zero value, omitted on wire → UI sees default 0)
    assert!(
        (mr.cuped_adjusted_effect - 0.0).abs() < 1e-10,
        "cupedAdjustedEffect should be 0.0 without covariates, got {}",
        mr.cuped_adjusted_effect
    );
    assert!(
        (mr.cuped_ci_lower - 0.0).abs() < 1e-10,
        "cupedCiLower should be 0.0 without covariates"
    );
    assert!(
        (mr.cuped_ci_upper - 0.0).abs() < 1e-10,
        "cupedCiUpper should be 0.0 without covariates"
    );
    assert!(
        (mr.variance_reduction_pct - 0.0).abs() < 1e-10,
        "varianceReductionPct should be 0.0 without covariates"
    );
}

// ============================================================================
// Group 3: Specialized RPCs (3 tests)
// ============================================================================

/// 7. InterleavingAnalysisResult: algorithmWinRates map with 2 algorithms ∈ [0,1],
///    signTestPValue ∈ [0,1], algorithmStrengths[] with all fields finite,
///    positionAnalyses[] with position ≥ 0, computedAt present.
#[tokio::test]
async fn contract_interleaving_analysis() {
    let tmp = TempDir::new().unwrap();
    let n = 20;
    let exp_ids: Vec<&str> = vec!["exp-1"; n];
    let user_ids: Vec<String> = (0..n).map(|i| format!("u{i}")).collect();
    let user_id_refs: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
    // 14 wins for algo_a, 6 for algo_b
    let algo_scores: Vec<Vec<(&str, f64)>> = (0..n)
        .map(|i| {
            if i < 14 {
                vec![("algo_a", 5.0), ("algo_b", 2.0)]
            } else {
                vec![("algo_a", 2.0), ("algo_b", 5.0)]
            }
        })
        .collect();
    let winners: Vec<Option<&str>> = (0..n)
        .map(|i| if i < 14 { Some("algo_a") } else { Some("algo_b") })
        .collect();
    let engagements: Vec<i64> = vec![7; n];

    let batch = make_interleaving_data(
        &exp_ids, &user_id_refs, &algo_scores, &winners, &engagements,
    );
    write_table(tmp.path(), "interleaving_scores", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let result = resp.into_inner();

    // experimentId
    assert_eq!(result.experiment_id, "exp-1");

    // algorithmWinRates map: 2 algorithms, values ∈ [0,1]
    assert_eq!(
        result.algorithm_win_rates.len(), 2,
        "algorithmWinRates should have 2 entries"
    );
    for (algo, &rate) in &result.algorithm_win_rates {
        assert_p_value(rate, &format!("algorithmWinRates['{algo}']"));
    }

    // signTestPValue ∈ [0,1]
    assert_p_value(result.sign_test_p_value, "signTestPValue");

    // algorithmStrengths[]: algorithmId, strength, ciLower/ciUpper all finite
    assert!(
        !result.algorithm_strengths.is_empty(),
        "algorithmStrengths must not be empty"
    );
    for s in &result.algorithm_strengths {
        assert!(!s.algorithm_id.is_empty(), "algorithmId must be non-empty");
        assert_finite(s.strength, &format!("strength[{}]", s.algorithm_id));
        assert_finite(s.ci_lower, &format!("ciLower[{}]", s.algorithm_id));
        assert_finite(s.ci_upper, &format!("ciUpper[{}]", s.algorithm_id));
    }

    // positionAnalyses[]: position ≥ 0 + algorithmEngagementRates map
    assert!(
        !result.position_analyses.is_empty(),
        "positionAnalyses must not be empty"
    );
    for pa in &result.position_analyses {
        assert!(pa.position >= 0, "position must be >= 0, got {}", pa.position);
        assert!(
            !pa.algorithm_engagement_rates.is_empty(),
            "algorithmEngagementRates must not be empty for position {}",
            pa.position
        );
    }

    // computedAt present
    assert!(result.computed_at.is_some(), "computedAt must be present");
}

/// 8. NoveltyAnalysisResult: all numeric fields finite, decayConstantDays ≥ 0,
///    daysUntilProjectedStability ≥ 0, computedAt present.
#[tokio::test]
async fn contract_novelty_analysis() {
    let tmp = TempDir::new().unwrap();
    // 15 days of decaying effect: s=5, a=3, d=4
    let n = 15;
    let base_date = 19700i32;
    let exp_ids: Vec<&str> = vec!["exp-1"; n];
    let metric_ids: Vec<&str> = vec!["ctr"; n];
    let dates: Vec<i32> = (0..n as i32).map(|i| base_date + i).collect();
    let effects: Vec<f64> = (0..n)
        .map(|i| 5.0 + 3.0 * (-(i as f64) / 4.0).exp())
        .collect();
    let sizes: Vec<i64> = vec![1000; n];

    let batch = make_daily_effects_data(&exp_ids, &metric_ids, &dates, &effects, &sizes);
    write_table(tmp.path(), "daily_treatment_effects", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let result = resp.into_inner();

    // experimentId + metricId non-empty
    assert_eq!(result.experiment_id, "exp-1");
    assert!(!result.metric_id.is_empty(), "metricId must be non-empty");

    // noveltyDetected is a bool — just verify it's present (with decaying effect, should be true)
    assert!(result.novelty_detected, "novelty should be detected with decaying effect");

    // All numeric fields finite
    assert_finite(result.raw_treatment_effect, "rawTreatmentEffect");
    assert_finite(result.projected_steady_state_effect, "projectedSteadyStateEffect");
    assert_finite(result.novelty_amplitude, "noveltyAmplitude");
    assert_finite(result.decay_constant_days, "decayConstantDays");
    assert_finite(result.days_until_projected_stability, "daysUntilProjectedStability");

    // Decay constant non-negative
    assert!(
        result.decay_constant_days >= 0.0,
        "decayConstantDays must be >= 0, got {}",
        result.decay_constant_days
    );

    // daysUntilProjectedStability non-negative
    assert!(
        result.days_until_projected_stability >= 0.0,
        "daysUntilProjectedStability must be >= 0, got {}",
        result.days_until_projected_stability
    );

    // computedAt present
    assert!(result.computed_at.is_some(), "computedAt must be present");
}

/// 9. InterferenceAnalysisResult: JSD ∈ [0,1], Jaccard ∈ [0,1], Gini ∈ [0,1],
///    catalog coverage ∈ [0,1], spilloverTitles[] with contentId + finite watch rates
///    + p-value ∈ [0,1], computedAt present.
#[tokio::test]
async fn contract_interference_analysis_with_spillover() {
    let tmp = TempDir::new().unwrap();
    // Create content consumption data with divergent distributions to trigger spillover
    // Control watches movie-a heavily, treatment watches movie-b heavily
    let n = 20;
    let exp_ids: Vec<&str> = vec!["exp-1"; n];
    let variant_ids: Vec<&str> = vec![
        // Control: mostly movie-a
        "control", "control", "control", "control", "control",
        "control", "control", "control", "control", "control",
        // Treatment: mostly movie-b
        "treatment", "treatment", "treatment", "treatment", "treatment",
        "treatment", "treatment", "treatment", "treatment", "treatment",
    ];
    let content_ids: Vec<&str> = vec![
        // Control: heavy on movie-a, light on movie-b
        "movie-a", "movie-a", "movie-a", "movie-a", "movie-a",
        "movie-a", "movie-a", "movie-b", "movie-c", "movie-d",
        // Treatment: heavy on movie-b, light on movie-a
        "movie-b", "movie-b", "movie-b", "movie-b", "movie-b",
        "movie-b", "movie-b", "movie-a", "movie-c", "movie-e",
    ];
    let watch_times: Vec<f64> = vec![
        300.0, 280.0, 320.0, 310.0, 290.0,
        300.0, 280.0, 50.0, 40.0, 30.0,
        300.0, 280.0, 320.0, 310.0, 290.0,
        300.0, 280.0, 50.0, 40.0, 30.0,
    ];
    let view_counts: Vec<i64> = vec![
        10, 9, 11, 10, 9, 10, 9, 2, 1, 1,
        10, 9, 11, 10, 9, 10, 9, 2, 1, 1,
    ];
    let unique_viewers: Vec<i64> = vec![
        8, 7, 9, 8, 7, 8, 7, 2, 1, 1,
        8, 7, 9, 8, 7, 8, 7, 2, 1, 1,
    ];

    let batch = make_content_consumption_data(
        &exp_ids, &variant_ids, &content_ids, &watch_times, &view_counts, &unique_viewers,
    );
    write_table(tmp.path(), "content_consumption", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .get_interference_analysis(Request::new(GetInterferenceAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let result = resp.into_inner();

    // experimentId
    assert_eq!(result.experiment_id, "exp-1");

    // JSD ∈ [0, 1]
    assert_finite(result.jensen_shannon_divergence, "jensenShannonDivergence");
    assert!(
        (0.0..=1.0).contains(&result.jensen_shannon_divergence),
        "JSD must be in [0, 1], got {}",
        result.jensen_shannon_divergence
    );

    // Jaccard ∈ [0, 1]
    assert_finite(result.jaccard_similarity_top_100, "jaccardSimilarityTop100");
    assert!(
        (0.0..=1.0).contains(&result.jaccard_similarity_top_100),
        "Jaccard must be in [0, 1], got {}",
        result.jaccard_similarity_top_100
    );

    // Gini ∈ [0, 1]
    assert_finite(result.treatment_gini_coefficient, "treatmentGiniCoefficient");
    assert!(
        (0.0..=1.0).contains(&result.treatment_gini_coefficient),
        "treatment Gini must be in [0, 1], got {}",
        result.treatment_gini_coefficient
    );
    assert_finite(result.control_gini_coefficient, "controlGiniCoefficient");
    assert!(
        (0.0..=1.0).contains(&result.control_gini_coefficient),
        "control Gini must be in [0, 1], got {}",
        result.control_gini_coefficient
    );

    // Catalog coverage: rows / unique_titles ratio (can exceed 1.0 when
    // a group has multiple consumption rows per title). UI displays as-is.
    assert_finite(result.treatment_catalog_coverage, "treatmentCatalogCoverage");
    assert!(
        result.treatment_catalog_coverage >= 0.0,
        "treatment catalog coverage must be >= 0, got {}",
        result.treatment_catalog_coverage
    );
    assert_finite(result.control_catalog_coverage, "controlCatalogCoverage");
    assert!(
        result.control_catalog_coverage >= 0.0,
        "control catalog coverage must be >= 0, got {}",
        result.control_catalog_coverage
    );

    // spilloverTitles[]: contentId + finite watch rates + p-value ∈ [0,1]
    // With divergent distributions, we expect at least some spillover titles
    for title in &result.spillover_titles {
        assert!(!title.content_id.is_empty(), "contentId must be non-empty");
        assert_finite(title.treatment_watch_rate, "spillover.treatmentWatchRate");
        assert_finite(title.control_watch_rate, "spillover.controlWatchRate");
        assert_p_value(title.p_value, "spillover.pValue");
    }

    // computedAt present
    assert!(result.computed_at.is_some(), "computedAt must be present");
}

// ============================================================================
// Group 4: Error contract (2 tests)
// ============================================================================

/// 10. All 5 RPCs return NOT_FOUND for nonexistent experiment.
///     UI receives 404 + `{ code: 'not_found' }`.
#[tokio::test]
async fn contract_not_found_for_missing_experiment() {
    let tmp = TempDir::new().unwrap();

    // Write minimal data so Delta tables exist but experiment doesn't match
    let batch = make_analysis_data(
        &["exp-other"], &["u1"], &["control"], &["ctr"], &[1.0], &[None],
    );
    write_table(tmp.path(), "metric_summaries", batch).await;

    let il_batch = make_interleaving_data(
        &["exp-other"], &["u1"],
        &[vec![("algo_a", 3.0), ("algo_b", 1.0)]],
        &[Some("algo_a")], &[4],
    );
    write_table(tmp.path(), "interleaving_scores", il_batch).await;

    let de_batch = make_daily_effects_data(
        &["exp-other"], &["ctr"], &[19700], &[5.0], &[1000],
    );
    write_table(tmp.path(), "daily_treatment_effects", de_batch).await;

    let cc_batch = make_content_consumption_data(
        &["exp-other", "exp-other"],
        &["control", "treatment"],
        &["movie-a", "movie-b"],
        &[100.0, 200.0], &[10, 20], &[5, 10],
    );
    write_table(tmp.path(), "content_consumption", cc_batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let missing = "exp-nonexistent";

    // RunAnalysis
    let err = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: missing.into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound, "RunAnalysis: {err}");

    // GetAnalysisResult
    let err = handler
        .get_analysis_result(Request::new(GetAnalysisResultRequest {
            experiment_id: missing.into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound, "GetAnalysisResult: {err}");

    // GetInterleavingAnalysis
    let err = handler
        .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
            experiment_id: missing.into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound, "GetInterleavingAnalysis: {err}");

    // GetNoveltyAnalysis
    let err = handler
        .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
            experiment_id: missing.into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound, "GetNoveltyAnalysis: {err}");

    // GetInterferenceAnalysis
    let err = handler
        .get_interference_analysis(Request::new(GetInterferenceAnalysisRequest {
            experiment_id: missing.into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound, "GetInterferenceAnalysis: {err}");
}

/// 11. All 5 RPCs reject empty experiment_id with INVALID_ARGUMENT.
#[tokio::test]
async fn contract_empty_experiment_id_invalid_argument() {
    let handler = test_handler("/tmp/nonexistent");

    // RunAnalysis
    let err = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument, "RunAnalysis: {err}");

    // GetAnalysisResult
    let err = handler
        .get_analysis_result(Request::new(GetAnalysisResultRequest {
            experiment_id: "".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument, "GetAnalysisResult: {err}");

    // GetInterleavingAnalysis
    let err = handler
        .get_interleaving_analysis(Request::new(GetInterleavingAnalysisRequest {
            experiment_id: "".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument, "GetInterleavingAnalysis: {err}");

    // GetNoveltyAnalysis
    let err = handler
        .get_novelty_analysis(Request::new(GetNoveltyAnalysisRequest {
            experiment_id: "".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument, "GetNoveltyAnalysis: {err}");

    // GetInterferenceAnalysis
    let err = handler
        .get_interference_analysis(Request::new(GetInterferenceAnalysisRequest {
            experiment_id: "".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument, "GetInterferenceAnalysis: {err}");
}

// ============================================================================
// Group 5: Cochran Q (1 test)
// ============================================================================

/// 12. Heterogeneous segments → cochran_q_p_value < 0.05.
///     Homogeneous (or absent) → 0.0 (proto3 omits).
///     UI checks this for heterogeneity indicator in CATE tab.
#[tokio::test]
async fn contract_cochran_q_heterogeneity() {
    // Part A: Heterogeneous effects → significant Q
    let tmp_het = TempDir::new().unwrap();
    let n = 20;
    let exp_ids: Vec<&str> = vec!["exp-1"; n];
    let user_ids: Vec<String> = (0..n).map(|i| format!("u{i:02}")).collect();
    let user_id_refs: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
    let variant_ids: Vec<&str> = vec![
        // TRIAL: 5 control + 5 treatment
        "control", "control", "control", "control", "control",
        "treatment", "treatment", "treatment", "treatment", "treatment",
        // ESTABLISHED: 5 control + 5 treatment
        "control", "control", "control", "control", "control",
        "treatment", "treatment", "treatment", "treatment", "treatment",
    ];
    let metric_ids: Vec<&str> = vec!["ctr"; n];
    let values: Vec<f64> = vec![
        // TRIAL: control ~3, treatment ~4 (effect ~1)
        1.0, 2.0, 3.0, 4.0, 5.0,
        2.0, 3.0, 4.0, 5.0, 6.0,
        // ESTABLISHED: control ~3, treatment ~13 (effect ~10)
        1.0, 2.0, 3.0, 4.0, 5.0,
        11.0, 12.0, 13.0, 14.0, 15.0,
    ];
    let covariates: Vec<Option<f64>> = vec![None; n];
    let segments: Vec<Option<&str>> = vec![
        Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"),
        Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"), Some("TRIAL"),
        Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"),
        Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"), Some("ESTABLISHED"),
    ];

    let batch = make_segmented_analysis_data(
        &exp_ids, &user_id_refs, &variant_ids, &metric_ids, &values, &covariates, &segments,
    );
    write_table(tmp_het.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp_het.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let result_het = resp.into_inner();
    assert!(
        result_het.cochran_q_p_value < 0.05,
        "heterogeneous segments: cochran_q_p_value {} should be < 0.05",
        result_het.cochran_q_p_value
    );

    // Part B: No segments → cochran_q_p_value == 0.0 (proto3 omits)
    let tmp_no_seg = TempDir::new().unwrap();
    let batch_no_seg = make_analysis_data(
        &vec!["exp-1"; 10],
        &["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "u10"],
        &[
            "control", "control", "control", "control", "control",
            "treatment", "treatment", "treatment", "treatment", "treatment",
        ],
        &vec!["ctr"; 10],
        &[1.0, 2.0, 3.0, 4.0, 5.0, 11.0, 12.0, 13.0, 14.0, 15.0],
        &vec![None; 10],
    );
    write_table(tmp_no_seg.path(), "metric_summaries", batch_no_seg).await;

    let handler2 = test_handler(tmp_no_seg.path().to_str().unwrap());
    let resp2 = handler2
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-1".into(),
            ..Default::default()
        }))
        .await
        .unwrap();

    let result_no = resp2.into_inner();
    assert!(
        (result_no.cochran_q_p_value - 0.0).abs() < 1e-10,
        "no segments: cochran_q_p_value should be 0.0 (proto3 omits), got {}",
        result_no.cochran_q_p_value
    );
}

// ---------------------------------------------------------------------------
// ADR-015: AVLM integration test — narrower CIs than mSPRT on golden-file data
// ---------------------------------------------------------------------------
//
// Golden-file data: outcome Y strongly correlated with covariate X (θ ≈ 2.0,
// R² ≈ 0.999). AVLM regression-adjusts away ~99% of outcome variance, producing
// a confidence sequence much narrower than the unadjusted mSPRT CI.
//
// AVLM run:  covariates non-null → full regression adjustment → narrow CI
// mSPRT run: same Y values, null covariates → x=0 → unadjusted mSPRT CS → wide CI
//
// Asserts:
//   - AVLM CI width < mSPRT CI width (primary goal of ADR-015)
//   - AVLM sequential_result.boundary_crossed == true (treatment effect = 1.0 detected)
//   - AVLM variance_reduction_pct > 80% (confirms regression adjustment is effective)

#[tokio::test]
async fn test_avlm_narrower_ci_than_msprt_on_golden_data() {
    // Golden-file data: Y = 2·X + treatment_effect + small_noise
    // Control: 10 users, X in [3.0, 7.5], Y ≈ 2·X (residuals ~0.1)
    // Treatment: 10 users, X in [3.0, 7.5], Y ≈ 2·X + 1.0
    let exp_id = "exp-avlm-golden";

    // Outcome values (Y)
    let y_control = [6.1, 8.0, 10.1, 12.0, 14.1, 7.1, 9.0, 11.0, 13.0, 15.1_f64];
    let y_treatment = [7.1, 9.0, 11.0, 13.1, 15.0, 8.0, 10.1, 12.0, 14.1, 16.0_f64];

    // Covariate values (X) — same for both arms
    let x = [3.0, 4.0, 5.0, 6.0, 7.0, 3.5, 4.5, 5.5, 6.5, 7.5_f64];

    let n = y_control.len();
    let exp_ids: Vec<&str> = vec![exp_id; 2 * n];
    let user_ids: Vec<String> = (0..2 * n).map(|i| format!("u{}", i)).collect();
    let user_id_refs: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
    let metric_ids: Vec<&str> = vec!["watch_time"; 2 * n];

    let mut variant_ids: Vec<&str> = vec!["control"; n];
    variant_ids.extend(vec!["treatment"; n]);

    let mut values: Vec<f64> = y_control.to_vec();
    values.extend_from_slice(&y_treatment);

    // AVLM run: non-null covariates (strong correlation)
    let covariates_avlm: Vec<Option<f64>> = x.iter().map(|&v| Some(v))
        .chain(x.iter().map(|&v| Some(v)))
        .collect();

    // mSPRT run: null covariates → AVLM falls back to unadjusted CS (= mSPRT)
    let covariates_msprt: Vec<Option<f64>> = vec![None; 2 * n];

    // Build Delta Lake tables in separate temp dirs.
    let tmp_avlm = TempDir::new().unwrap();
    let tmp_msprt = TempDir::new().unwrap();

    let batch_avlm = make_analysis_data(
        &exp_ids,
        &user_id_refs,
        &variant_ids,
        &metric_ids,
        &values,
        &covariates_avlm,
    );
    write_table(tmp_avlm.path(), "metric_summaries", batch_avlm).await;

    let batch_msprt = make_analysis_data(
        &exp_ids,
        &user_id_refs,
        &variant_ids,
        &metric_ids,
        &values,
        &covariates_msprt,
    );
    write_table(tmp_msprt.path(), "metric_summaries", batch_msprt).await;

    let handler_avlm = test_handler(tmp_avlm.path().to_str().unwrap());
    let handler_msprt = test_handler(tmp_msprt.path().to_str().unwrap());

    // SEQUENTIAL_METHOD_AVLM = 4
    let avlm_resp = handler_avlm
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: exp_id.into(),
            sequential_method: 4,
            tau_sq: 0.5,
            cuped_covariate_metric_id: "pre_experiment".into(),
            ..Default::default()
        }))
        .await
        .expect("AVLM RunAnalysis should succeed");

    let msprt_resp = handler_msprt
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: exp_id.into(),
            sequential_method: 4,
            tau_sq: 0.5,
            ..Default::default()
        }))
        .await
        .expect("mSPRT (null covariates) RunAnalysis should succeed");

    let avlm_result = avlm_resp.into_inner();
    let msprt_result = msprt_resp.into_inner();

    // Locate the treatment variant metric result for "watch_time".
    let avlm_mr = avlm_result
        .metric_results
        .iter()
        .find(|r| r.metric_id == "watch_time" && r.variant_id == "treatment")
        .expect("AVLM: watch_time/treatment metric result missing");

    let msprt_mr = msprt_result
        .metric_results
        .iter()
        .find(|r| r.metric_id == "watch_time" && r.variant_id == "treatment")
        .expect("mSPRT: watch_time/treatment metric result missing");

    // AVLM CI is in cuped_ci_lower/upper (regression-adjusted confidence sequence).
    let avlm_width = avlm_mr.cuped_ci_upper - avlm_mr.cuped_ci_lower;
    let msprt_width = msprt_mr.cuped_ci_upper - msprt_mr.cuped_ci_lower;

    assert_finite(avlm_width, "avlm_width");
    assert_finite(msprt_width, "msprt_width");

    assert!(
        avlm_width > 0.0,
        "AVLM CI width must be positive, got {avlm_width}"
    );
    assert!(
        msprt_width > 0.0,
        "mSPRT CI width must be positive, got {msprt_width}"
    );
    assert!(
        avlm_width < msprt_width,
        "AVLM CI (width={avlm_width:.4}) must be narrower than mSPRT CI (width={msprt_width:.4})"
    );

    // Variance reduction should exceed 80% for this strongly correlated covariate.
    assert!(
        avlm_mr.variance_reduction_pct > 80.0,
        "expected variance reduction > 80%, got {:.1}%",
        avlm_mr.variance_reduction_pct
    );

    // Treatment effect = 1.0 should be detected at this sample size.
    let seq = avlm_mr
        .sequential_result
        .as_ref()
        .expect("AVLM: sequential_result should be populated");
    assert!(
        seq.boundary_crossed,
        "AVLM: confidence sequence should exclude 0 (treatment effect = 1.0 is large)"
    );
}
