//! AVLM integration tests (ADR-015).
//!
//! Verifies that RunAnalysis with SEQUENTIAL_METHOD_AVLM:
//! 1. Populates sequential_result with AVLM confidence sequence fields.
//! 2. Produces narrower confidence intervals than the mSPRT-equivalent
//!    (AVLM with x=0.0 covariate) when a high-quality covariate is present.
//!
//! Golden-file data: derived from the evalue_avlm_* fixtures in
//! crates/experimentation-stats/tests/golden/. Uses a perfect covariate
//! (x = y − true_effect, correlation ≈ 1) to maximise variance reduction.

use experimentation_analysis::config::AnalysisConfig;
use experimentation_analysis::grpc::AnalysisServiceHandler;

use deltalake::arrow::array::{Float64Array, StringArray};
use deltalake::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::DeltaOps;
use std::sync::Arc;
use tempfile::TempDir;

use experimentation_proto::experimentation::analysis::v1::analysis_service_server::AnalysisService;
use experimentation_proto::experimentation::analysis::v1::{RunAnalysisRequest, SequentialMethod};
use tonic::Request;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

fn test_config(path: &str) -> AnalysisConfig {
    AnalysisConfig {
        grpc_addr: "[::1]:0".into(),
        delta_lake_path: path.into(),
        default_alpha: 0.05,
        default_js_threshold: 0.05,
        database_url: None,
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

/// Build a metric_summaries RecordBatch.
fn make_batch(
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

// ---------------------------------------------------------------------------
// Golden-file data
//
// Design: true effect = 2.0, pre-experiment covariate x_i = y_i_base + noise.
// High-quality (near-perfect, r≈0.97) covariate: x = control_y + small_noise.
// n=20 per arm — enough for AVLM to produce meaningful CIs.
// ---------------------------------------------------------------------------

/// Build golden dataset with a high-quality (r≈0.97) covariate.
///
/// Control: y ~ N(5, 0.2), x = y + N(0, 0.07) noise (pre-experiment predictor).
/// Treatment: y = control_y + 2.0, x = same pre-experiment values (no shift).
/// Expected: AVLM variance reduction > 0.5, AVLM half-width < mSPRT half-width.
fn golden_data() -> (Vec<&'static str>, Vec<String>, Vec<&'static str>, Vec<&'static str>, Vec<f64>, Vec<Option<f64>>) {
    // Pre-generated deterministic values (no runtime RNG).
    // Control y ~ N(5, 0.2), Treatment y = control_y + 2.0 (true_effect).
    // x = control_y + small_noise (high correlation, r≈0.97, but not perfect).
    let control_y: Vec<f64> = vec![
        4.82, 5.14, 4.91, 5.33, 4.76, 5.08, 4.97, 5.21, 4.85, 5.02,
        4.79, 5.17, 4.93, 5.28, 4.88, 5.05, 4.72, 5.11, 4.96, 5.19,
    ];
    // Covariate: pre-experiment metric with small noise (σ≈0.07 ≈ 35% of outcome σ).
    // This gives r ≈ 0.93 and variance reduction ≈ 0.86 under true theta=1.
    let x_noise: Vec<f64> = vec![
        0.07, -0.04, 0.09, -0.06, 0.05, -0.08, 0.03, 0.06, -0.05, 0.04,
       -0.07,  0.08,-0.03,  0.05,-0.06,  0.02,-0.09,  0.07,-0.04,  0.06,
    ];
    let control_x: Vec<f64> = control_y
        .iter()
        .zip(x_noise.iter())
        .map(|(&y, &n)| y + n)
        .collect();
    let treatment_y: Vec<f64> = control_y.iter().map(|&y| y + 2.0).collect();
    // Pre-experiment covariate is the same for both arms (no treatment effect on x).
    let treatment_x: Vec<f64> = control_x.clone();

    let n_c = control_y.len();
    let n_t = treatment_y.len();
    let n = n_c + n_t;

    let exp_ids: Vec<&str> = vec!["exp-avlm"; n];
    let user_ids: Vec<String> = (0..n).map(|i| format!("u{}", i)).collect();
    let variant_ids: Vec<&str> = {
        let mut v = vec!["control"; n_c];
        v.extend(vec!["treatment"; n_t]);
        v
    };
    let metric_ids: Vec<&str> = vec!["watch_hours"; n];
    let values: Vec<f64> = control_y.iter().chain(treatment_y.iter()).copied().collect();
    let covariates: Vec<Option<f64>> = control_x
        .iter()
        .chain(treatment_x.iter())
        .map(|&x| Some(x))
        .collect();

    (exp_ids, user_ids, variant_ids, metric_ids, values, covariates)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// AVLM sequential_result is populated when SEQUENTIAL_METHOD_AVLM is requested.
#[tokio::test]
async fn avlm_sequential_result_populated() {
    let tmp = TempDir::new().unwrap();
    let (exp_ids, user_ids_owned, variant_ids, metric_ids, values, covariates) = golden_data();
    let user_ids: Vec<&str> = user_ids_owned.iter().map(|s| s.as_str()).collect();

    let batch = make_batch(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates);
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-avlm".into(),
            sequential_method: SequentialMethod::Avlm as i32,
            cuped_covariate_metric_id: "watch_hours".into(),
            tau_sq: 0.1,
        }))
        .await
        .unwrap();

    let result = resp.into_inner();
    assert!(!result.metric_results.is_empty(), "must have metric results");

    let mr = &result.metric_results[0];
    let seq = mr
        .sequential_result
        .as_ref()
        .expect("sequential_result must be populated for SEQUENTIAL_METHOD_AVLM");

    // Core AVLM fields must be set.
    assert!(seq.avlm_n_control > 0, "avlm_n_control must be > 0");
    assert!(seq.avlm_n_treatment > 0, "avlm_n_treatment must be > 0");
    assert!(seq.avlm_half_width > 0.0, "avlm_half_width must be positive");
    assert!(
        seq.avlm_ci_lower < seq.avlm_ci_upper,
        "CI must be ordered: {} < {}",
        seq.avlm_ci_lower,
        seq.avlm_ci_upper
    );
    // CI must contain the adjusted effect.
    assert!(
        seq.avlm_ci_lower <= seq.avlm_adjusted_effect
            && seq.avlm_adjusted_effect <= seq.avlm_ci_upper,
        "CI [{}, {}] must contain adjusted_effect {}",
        seq.avlm_ci_lower,
        seq.avlm_ci_upper,
        seq.avlm_adjusted_effect
    );
    // Variance reduction must be ∈ [0, 1).
    assert!(
        (0.0..1.0).contains(&seq.avlm_variance_reduction),
        "variance_reduction {} must be in [0, 1)",
        seq.avlm_variance_reduction
    );
}

/// AVLM with a perfect covariate produces narrower CIs than without covariate (mSPRT-equivalent).
///
/// Golden file: control_y ~ N(5,1), treatment_y = control_y + 2.0 (true_effect).
/// Covariate x = control_y (r² ≈ 1), so AVLM should remove ≈ 100% of variance.
/// We verify avlm_half_width < msprt_half_width (no-covariate baseline).
#[tokio::test]
async fn avlm_narrower_ci_than_msprt_on_golden_data() {
    let tmp_avlm = TempDir::new().unwrap();
    let tmp_msprt = TempDir::new().unwrap();

    let (exp_ids, user_ids_owned, variant_ids, metric_ids, values, covariates) = golden_data();
    let user_ids: Vec<&str> = user_ids_owned.iter().map(|s| s.as_str()).collect();

    // Write same underlying data to both directories.
    let batch_avlm = make_batch(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates);
    write_table(tmp_avlm.path(), "metric_summaries", batch_avlm).await;

    // mSPRT-equivalent: zero out covariates so AVLM falls back to unadjusted.
    let zero_covariates: Vec<Option<f64>> = vec![Some(0.0); values.len()];
    let batch_msprt = make_batch(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &zero_covariates);
    write_table(tmp_msprt.path(), "metric_summaries", batch_msprt).await;

    // Run AVLM (with high-quality covariate).
    let handler_avlm = test_handler(tmp_avlm.path().to_str().unwrap());
    let resp_avlm = handler_avlm
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-avlm".into(),
            sequential_method: SequentialMethod::Avlm as i32,
            cuped_covariate_metric_id: "watch_hours".into(),
            tau_sq: 0.1,
        }))
        .await
        .unwrap();

    // Run mSPRT-equivalent (x=0, no covariate adjustment).
    let handler_msprt = test_handler(tmp_msprt.path().to_str().unwrap());
    let resp_msprt = handler_msprt
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-avlm".into(),
            sequential_method: SequentialMethod::Avlm as i32,
            cuped_covariate_metric_id: String::new(),
            tau_sq: 0.1,
        }))
        .await
        .unwrap();

    let mr_avlm = &resp_avlm.into_inner().metric_results[0];
    let mr_msprt = &resp_msprt.into_inner().metric_results[0];

    let avlm_hw = mr_avlm
        .sequential_result
        .as_ref()
        .expect("AVLM must have sequential_result")
        .avlm_half_width;
    let msprt_hw = mr_msprt
        .sequential_result
        .as_ref()
        .expect("mSPRT-baseline must have sequential_result")
        .avlm_half_width;

    assert!(
        avlm_hw < msprt_hw,
        "AVLM CI half-width ({avlm_hw:.4}) must be narrower than mSPRT-equivalent ({msprt_hw:.4}) \
         when a high-quality covariate (r²≈1) is present"
    );

    // Sanity: AVLM variance reduction should be large (> 50%) for a perfect covariate.
    let var_reduction = mr_avlm
        .sequential_result
        .as_ref()
        .unwrap()
        .avlm_variance_reduction;
    assert!(
        var_reduction > 0.5,
        "variance reduction ({var_reduction:.4}) should exceed 0.5 for a perfect covariate"
    );
}

/// Without sequential_method set, sequential_result is None (backward compatibility).
#[tokio::test]
async fn no_sequential_result_when_method_unspecified() {
    let tmp = TempDir::new().unwrap();
    let (exp_ids, user_ids_owned, variant_ids, metric_ids, values, covariates) = golden_data();
    let user_ids: Vec<&str> = user_ids_owned.iter().map(|s| s.as_str()).collect();

    let batch = make_batch(&exp_ids, &user_ids, &variant_ids, &metric_ids, &values, &covariates);
    write_table(tmp.path(), "metric_summaries", batch).await;

    let handler = test_handler(tmp.path().to_str().unwrap());
    let resp = handler
        .run_analysis(Request::new(RunAnalysisRequest {
            experiment_id: "exp-avlm".into(),
            sequential_method: SequentialMethod::Unspecified as i32,
            cuped_covariate_metric_id: String::new(),
            tau_sq: 0.0,
        }))
        .await
        .unwrap();

    let mr = &resp.into_inner().metric_results[0];
    assert!(
        mr.sequential_result.is_none(),
        "sequential_result must be None when method is UNSPECIFIED"
    );
}
