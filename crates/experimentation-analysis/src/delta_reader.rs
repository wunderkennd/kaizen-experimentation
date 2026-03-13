//! Delta Lake readers for analysis service tables.
//!
//! Reads Delta Lake tables filtered by experiment_id and maps results into
//! domain structs for the stats crate.

use anyhow::{bail, Context};
use deltalake::arrow::array::{
    Array, Date32Array, Float64Array, Int64Array, MapArray, StringArray,
};
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::DeltaTable;
use experimentation_stats::interference::{ContentConsumption, InterferenceInput};
use experimentation_stats::interleaving::InterleavingScore;
use experimentation_stats::novelty::DailyEffect;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Row from the content_consumption Delta table.
#[derive(Debug)]
struct ConsumptionRow {
    variant_id: String,
    content_id: String,
    watch_time_seconds: f64,
    view_count: u64,
    unique_viewers: u64,
}

/// Read content_consumption data from Delta Lake for a given experiment,
/// returning an `InterferenceInput` suitable for `analyze_interference()`.
pub async fn read_content_consumption(
    delta_path: &str,
    experiment_id: &str,
) -> anyhow::Result<InterferenceInput> {
    let table_path = format!("{}/content_consumption", delta_path);

    let table = deltalake::open_table(&table_path)
        .await
        .context("content_consumption table not found")?;

    let rows = extract_rows(&table, experiment_id).await?;

    if rows.is_empty() {
        bail!(
            "no content_consumption data found for experiment '{}'",
            experiment_id
        );
    }

    build_interference_input(rows, experiment_id)
}

/// Extract matching rows from the Delta table filtered by experiment_id.
async fn extract_rows(
    table: &DeltaTable,
    experiment_id: &str,
) -> anyhow::Result<Vec<ConsumptionRow>> {
    let mut rows = Vec::new();

    // Read all record batches and filter by experiment_id in-process.
    // For production scale, use DataFusion pushdown; for now this is simpler
    // and avoids the DataFusion session context complexity.
    let batches = collect_batches(table).await?;

    for batch in &batches {
        let exp_col = batch
            .column_by_name("experiment_id")
            .context("missing experiment_id column")?;
        let exp_arr = exp_col
            .as_any()
            .downcast_ref::<StringArray>()
            .context("experiment_id is not string")?;

        let variant_col = batch
            .column_by_name("variant_id")
            .context("missing variant_id column")?;
        let variant_arr = variant_col
            .as_any()
            .downcast_ref::<StringArray>()
            .context("variant_id is not string")?;

        let content_col = batch
            .column_by_name("content_id")
            .context("missing content_id column")?;
        let content_arr = content_col
            .as_any()
            .downcast_ref::<StringArray>()
            .context("content_id is not string")?;

        let watch_col = batch
            .column_by_name("watch_time_seconds")
            .context("missing watch_time_seconds column")?;
        let watch_arr = watch_col
            .as_any()
            .downcast_ref::<Float64Array>()
            .context("watch_time_seconds is not f64")?;

        let view_col = batch
            .column_by_name("view_count")
            .context("missing view_count column")?;
        let view_arr = view_col
            .as_any()
            .downcast_ref::<Int64Array>()
            .context("view_count is not i64")?;

        let viewer_col = batch
            .column_by_name("unique_viewers")
            .context("missing unique_viewers column")?;
        let viewer_arr = viewer_col
            .as_any()
            .downcast_ref::<Int64Array>()
            .context("unique_viewers is not i64")?;

        for i in 0..batch.num_rows() {
            if exp_arr.is_null(i) {
                continue;
            }
            if exp_arr.value(i) != experiment_id {
                continue;
            }
            rows.push(ConsumptionRow {
                variant_id: variant_arr.value(i).to_string(),
                content_id: content_arr.value(i).to_string(),
                watch_time_seconds: watch_arr.value(i),
                view_count: view_arr.value(i) as u64,
                unique_viewers: viewer_arr.value(i) as u64,
            });
        }
    }

    Ok(rows)
}

/// Collect all record batches from the Delta table.
async fn collect_batches(table: &DeltaTable) -> anyhow::Result<Vec<RecordBatch>> {
    use deltalake::datafusion::prelude::SessionContext;

    let ctx = SessionContext::new();
    let df = ctx
        .read_table(Arc::new(table.clone()))
        .context("failed to create DataFrame from Delta table")?;

    let batches = df.collect().await.context("failed to collect batches")?;
    Ok(batches)
}

/// Classify rows into control and treatment groups and build InterferenceInput.
fn build_interference_input(
    rows: Vec<ConsumptionRow>,
    experiment_id: &str,
) -> anyhow::Result<InterferenceInput> {
    // Group by variant_id.
    let mut by_variant: HashMap<String, Vec<ConsumptionRow>> = HashMap::new();
    for row in rows {
        by_variant
            .entry(row.variant_id.clone())
            .or_default()
            .push(row);
    }

    if by_variant.len() < 2 {
        bail!(
            "experiment '{}' has only {} variant(s), need at least 2 for interference analysis",
            experiment_id,
            by_variant.len()
        );
    }

    // Identify control variant: one named "control", or first alphabetically.
    let control_variant = if by_variant.contains_key("control") {
        "control".to_string()
    } else {
        let mut variants: Vec<&String> = by_variant.keys().collect();
        variants.sort();
        variants[0].clone()
    };

    // Build control and treatment content consumption lists.
    let control_rows = by_variant.remove(&control_variant).unwrap_or_default();
    let control: Vec<ContentConsumption> = control_rows
        .iter()
        .map(|r| ContentConsumption {
            content_id: r.content_id.clone(),
            watch_time_seconds: r.watch_time_seconds,
            view_count: r.view_count,
            unique_viewers: r.unique_viewers,
        })
        .collect();

    // Merge all non-control variants into a single treatment group.
    let mut treatment = Vec::new();
    for (_variant, variant_rows) in by_variant {
        for r in &variant_rows {
            treatment.push(ContentConsumption {
                content_id: r.content_id.clone(),
                watch_time_seconds: r.watch_time_seconds,
                view_count: r.view_count,
                unique_viewers: r.unique_viewers,
            });
        }
    }

    // Count total unique viewers per group.
    let total_control_viewers: u64 = control.iter().map(|c| c.unique_viewers).sum();
    let total_treatment_viewers: u64 = treatment.iter().map(|c| c.unique_viewers).sum();

    if control.is_empty() {
        bail!("no control data for experiment '{}'", experiment_id);
    }
    if treatment.is_empty() {
        bail!("no treatment data for experiment '{}'", experiment_id);
    }

    Ok(InterferenceInput {
        treatment,
        control,
        total_treatment_viewers,
        total_control_viewers,
    })
}

// ---------------------------------------------------------------------------
// metric_summaries reader (for RunAnalysis / GetAnalysisResult)
// ---------------------------------------------------------------------------

/// Per-variant metric observations: Vec<(metric_value, Option<cuped_covariate>)>.
type VariantObservations = Vec<(f64, Option<f64>)>;

/// metric_id → variant_id → observations.
type MetricsByVariant = HashMap<String, HashMap<String, VariantObservations>>;

/// Segment-stratified observations: metric_id → segment → variant_id → values.
type SegmentData = HashMap<String, HashMap<String, HashMap<String, Vec<f64>>>>;

/// Grouped metric data for an experiment, ready for t-test / CUPED / SRM.
#[derive(Debug)]
pub struct ExperimentMetrics {
    /// metric_id → variant_id → observations.
    pub metrics: MetricsByVariant,
    /// variant_id → count of distinct users (for SRM check).
    pub variant_user_counts: HashMap<String, u64>,
    /// Segment-stratified observations for CATE analysis.
    /// Only populated when metric_summaries has a `lifecycle_segment` column.
    pub segment_data: SegmentData,
    /// Session-level observations for clustering: metric_id → Vec<(value, user_id, variant_id)>.
    /// Populated only when metric_summaries has a `session_id` column,
    /// indicating multiple rows per user (one per session).
    pub session_data: HashMap<String, Vec<(f64, String, String)>>,
}

/// Read metric_summaries from Delta Lake for a given experiment.
pub async fn read_metric_summaries(
    delta_path: &str,
    experiment_id: &str,
) -> anyhow::Result<ExperimentMetrics> {
    let table_path = format!("{}/metric_summaries", delta_path);
    let table = deltalake::open_table(&table_path)
        .await
        .context("metric_summaries table not found")?;

    let batches = collect_batches(&table).await?;

    let mut metrics: MetricsByVariant = HashMap::new();
    let mut segment_data: SegmentData = HashMap::new();
    let mut session_data: HashMap<String, Vec<(f64, String, String)>> = HashMap::new();
    let mut variant_users: HashMap<String, HashSet<String>> = HashMap::new();
    let mut found = false;

    for batch in &batches {
        let exp_arr = downcast_string(batch, "experiment_id")?;
        let user_arr = downcast_string(batch, "user_id")?;
        let variant_arr = downcast_string(batch, "variant_id")?;
        let metric_arr = downcast_string(batch, "metric_id")?;
        let value_arr = downcast_f64(batch, "metric_value")?;
        let cov_col = batch
            .column_by_name("cuped_covariate")
            .context("missing cuped_covariate column")?;
        let cov_arr = cov_col
            .as_any()
            .downcast_ref::<Float64Array>()
            .context("cuped_covariate is not f64")?;

        // lifecycle_segment is optional — older tables may not have it.
        let segment_arr = batch
            .column_by_name("lifecycle_segment")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>());

        // session_id is optional — when present, enables clustered SE computation.
        let session_arr = batch
            .column_by_name("session_id")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>());

        for i in 0..batch.num_rows() {
            if exp_arr.is_null(i) || exp_arr.value(i) != experiment_id {
                continue;
            }
            found = true;

            let variant = variant_arr.value(i).to_string();
            let metric = metric_arr.value(i).to_string();
            let value = value_arr.value(i);
            let covariate = if cov_arr.is_null(i) {
                None
            } else {
                Some(cov_arr.value(i))
            };

            variant_users
                .entry(variant.clone())
                .or_default()
                .insert(user_arr.value(i).to_string());

            metrics
                .entry(metric.clone())
                .or_default()
                .entry(variant.clone())
                .or_default()
                .push((value, covariate));

            // Populate segment-stratified data when lifecycle_segment is present.
            if let Some(seg_arr) = segment_arr {
                if !seg_arr.is_null(i) {
                    let segment = seg_arr.value(i).to_string();
                    segment_data
                        .entry(metric.clone())
                        .or_default()
                        .entry(segment)
                        .or_default()
                        .entry(variant.clone())
                        .or_default()
                        .push(value);
                }
            }

            // Populate session-level data when session_id is present.
            if session_arr.is_some() {
                session_data.entry(metric).or_default().push((
                    value,
                    user_arr.value(i).to_string(),
                    variant,
                ));
            }
        }
    }

    if !found {
        bail!(
            "no metric_summaries data found for experiment '{}'",
            experiment_id
        );
    }

    let variant_user_counts: HashMap<String, u64> = variant_users
        .into_iter()
        .map(|(k, v)| (k, v.len() as u64))
        .collect();

    Ok(ExperimentMetrics {
        metrics,
        variant_user_counts,
        segment_data,
        session_data,
    })
}

// ---------------------------------------------------------------------------
// interleaving_scores reader (for GetInterleavingAnalysis)
// ---------------------------------------------------------------------------

/// Read interleaving_scores from Delta Lake for a given experiment.
pub async fn read_interleaving_scores(
    delta_path: &str,
    experiment_id: &str,
) -> anyhow::Result<Vec<InterleavingScore>> {
    let table_path = format!("{}/interleaving_scores", delta_path);
    let table = deltalake::open_table(&table_path)
        .await
        .context("interleaving_scores table not found")?;

    let batches = collect_batches(&table).await?;
    let mut scores = Vec::new();

    for batch in &batches {
        let exp_arr = downcast_string(batch, "experiment_id")?;
        let user_arr = downcast_string(batch, "user_id")?;
        let map_col = batch
            .column_by_name("algorithm_scores")
            .context("missing algorithm_scores column")?;
        let map_arr = map_col
            .as_any()
            .downcast_ref::<MapArray>()
            .context("algorithm_scores is not a MapArray")?;
        let winner_col = batch
            .column_by_name("winning_algorithm_id")
            .context("missing winning_algorithm_id column")?;
        let winner_arr = winner_col
            .as_any()
            .downcast_ref::<StringArray>()
            .context("winning_algorithm_id is not string")?;
        let eng_arr = downcast_i64(batch, "total_engagements")?;

        for i in 0..batch.num_rows() {
            if exp_arr.is_null(i) || exp_arr.value(i) != experiment_id {
                continue;
            }

            // Extract MAP<STRING, DOUBLE> entries for this row.
            let entries = map_arr.value(i);
            let keys = entries
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .context("map keys not string")?;
            let values = entries
                .column(1)
                .as_any()
                .downcast_ref::<Float64Array>()
                .context("map values not f64")?;

            let mut algo_scores = HashMap::new();
            for j in 0..entries.len() {
                algo_scores.insert(keys.value(j).to_string(), values.value(j));
            }

            let winning = if winner_arr.is_null(i) {
                None
            } else {
                Some(winner_arr.value(i).to_string())
            };

            scores.push(InterleavingScore {
                user_id: user_arr.value(i).to_string(),
                algorithm_scores: algo_scores,
                winning_algorithm_id: winning,
                total_engagements: eng_arr.value(i) as u32,
            });
        }
    }

    if scores.is_empty() {
        bail!(
            "no interleaving_scores data found for experiment '{}'",
            experiment_id
        );
    }

    Ok(scores)
}

// ---------------------------------------------------------------------------
// daily_treatment_effects reader (for GetNoveltyAnalysis)
// ---------------------------------------------------------------------------

/// Read daily_treatment_effects from Delta Lake for a given experiment.
///
/// Returns `(metric_id, effects)` where `metric_id` is the metric with the
/// most data points (when the request only has experiment_id, not metric_id).
pub async fn read_daily_treatment_effects(
    delta_path: &str,
    experiment_id: &str,
) -> anyhow::Result<(String, Vec<DailyEffect>)> {
    let table_path = format!("{}/daily_treatment_effects", delta_path);
    let table = deltalake::open_table(&table_path)
        .await
        .context("daily_treatment_effects table not found")?;

    let batches = collect_batches(&table).await?;

    // metric_id → Vec<(effect_date_i32, effect, sample_size)>
    let mut by_metric: HashMap<String, Vec<(i32, f64, u64)>> = HashMap::new();

    for batch in &batches {
        let exp_arr = downcast_string(batch, "experiment_id")?;
        let metric_arr = downcast_string(batch, "metric_id")?;
        let date_col = batch
            .column_by_name("effect_date")
            .context("missing effect_date column")?;
        let date_arr = date_col
            .as_any()
            .downcast_ref::<Date32Array>()
            .context("effect_date is not Date32")?;
        let effect_arr = downcast_f64(batch, "absolute_effect")?;
        let size_arr = downcast_i64(batch, "sample_size")?;

        for i in 0..batch.num_rows() {
            if exp_arr.is_null(i) || exp_arr.value(i) != experiment_id {
                continue;
            }

            let metric = metric_arr.value(i).to_string();
            let date_val = date_arr.value(i); // days since epoch (i32)
            let effect = effect_arr.value(i);
            let sample_size = size_arr.value(i) as u64;

            by_metric
                .entry(metric)
                .or_default()
                .push((date_val, effect, sample_size));
        }
    }

    if by_metric.is_empty() {
        bail!(
            "no daily_treatment_effects data found for experiment '{}'",
            experiment_id
        );
    }

    // Pick metric with most data points.
    let (best_metric, mut rows) = by_metric.into_iter().max_by_key(|(_, v)| v.len()).unwrap();

    // Sort by date, convert to sequential days from minimum.
    rows.sort_by_key(|(date, _, _)| *date);
    let min_date = rows[0].0;

    let effects: Vec<DailyEffect> = rows
        .iter()
        .map(|&(date, effect, sample_size)| DailyEffect {
            day: (date - min_date) as u32,
            effect,
            sample_size,
        })
        .collect();

    Ok((best_metric, effects))
}

// ---------------------------------------------------------------------------
// Shared column downcast helpers
// ---------------------------------------------------------------------------

fn downcast_string<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .context(format!("missing {name} column"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .context(format!("{name} is not string"))
}

fn downcast_f64<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a Float64Array> {
    batch
        .column_by_name(name)
        .context(format!("missing {name} column"))?
        .as_any()
        .downcast_ref::<Float64Array>()
        .context(format!("{name} is not f64"))
}

fn downcast_i64<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a Int64Array> {
    batch
        .column_by_name(name)
        .context(format!("missing {name} column"))?
        .as_any()
        .downcast_ref::<Int64Array>()
        .context(format!("{name} is not i64"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deltalake::arrow::array::{
        builder::{Float64Builder, MapBuilder, StringBuilder},
        Date32Array, Float64Array, Int64Array, StringArray,
    };
    use deltalake::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
    use deltalake::arrow::record_batch::RecordBatch;
    use deltalake::DeltaOps;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_schema() -> Arc<ArrowSchema> {
        Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("variant_id", DataType::Utf8, false),
            Field::new("content_id", DataType::Utf8, false),
            Field::new("watch_time_seconds", DataType::Float64, false),
            Field::new("view_count", DataType::Int64, false),
            Field::new("unique_viewers", DataType::Int64, false),
        ]))
    }

    /// Write a single-batch Delta table into a named subdirectory.
    async fn write_named_test_table(dir: &std::path::Path, table_name: &str, batch: RecordBatch) {
        let table_path = dir.join(table_name);
        std::fs::create_dir_all(&table_path).unwrap();

        let ops = DeltaOps::try_from_uri(table_path.to_str().unwrap())
            .await
            .unwrap();
        ops.write(vec![batch]).await.unwrap();
    }

    /// Write a single-batch Delta table into a temp directory.
    async fn write_test_table(dir: &std::path::Path, batch: RecordBatch) {
        write_named_test_table(dir, "content_consumption", batch).await;
    }

    fn make_batch(
        experiment_ids: &[&str],
        variant_ids: &[&str],
        content_ids: &[&str],
        watch_times: &[f64],
        view_counts: &[i64],
        unique_viewers: &[i64],
    ) -> RecordBatch {
        let schema = test_schema();
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(experiment_ids.to_vec())),
                Arc::new(StringArray::from(variant_ids.to_vec())),
                Arc::new(StringArray::from(content_ids.to_vec())),
                Arc::new(Float64Array::from(watch_times.to_vec())),
                Arc::new(Int64Array::from(view_counts.to_vec())),
                Arc::new(Int64Array::from(unique_viewers.to_vec())),
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_read_content_consumption_basic() {
        let tmp = TempDir::new().unwrap();
        let batch = make_batch(
            &["exp-1", "exp-1", "exp-1", "exp-1"],
            &["control", "control", "treatment", "treatment"],
            &["movie-a", "movie-b", "movie-a", "movie-c"],
            &[100.0, 200.0, 150.0, 250.0],
            &[10, 20, 15, 25],
            &[5, 10, 8, 12],
        );
        write_test_table(tmp.path(), batch).await;

        let input = read_content_consumption(tmp.path().to_str().unwrap(), "exp-1")
            .await
            .unwrap();

        assert_eq!(input.control.len(), 2);
        assert_eq!(input.treatment.len(), 2);
        assert_eq!(input.total_control_viewers, 15); // 5 + 10
        assert_eq!(input.total_treatment_viewers, 20); // 8 + 12
    }

    #[tokio::test]
    async fn test_read_content_consumption_empty() {
        let tmp = TempDir::new().unwrap();
        let batch = make_batch(
            &["exp-1"],
            &["control"],
            &["movie-a"],
            &[100.0],
            &[10],
            &[5],
        );
        write_test_table(tmp.path(), batch).await;

        let result = read_content_consumption(tmp.path().to_str().unwrap(), "exp-999").await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no content_consumption data"),
            "expected 'no data' error, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_read_content_consumption_single_variant() {
        let tmp = TempDir::new().unwrap();
        let batch = make_batch(
            &["exp-1", "exp-1"],
            &["control", "control"],
            &["movie-a", "movie-b"],
            &[100.0, 200.0],
            &[10, 20],
            &[5, 10],
        );
        write_test_table(tmp.path(), batch).await;

        let result = read_content_consumption(tmp.path().to_str().unwrap(), "exp-1").await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("only 1 variant"),
            "expected 'only 1 variant' error, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_variant_classification() {
        let tmp = TempDir::new().unwrap();
        // No variant named "control" — first alphabetically ("alpha") becomes control
        let batch = make_batch(
            &["exp-1", "exp-1", "exp-1"],
            &["beta", "alpha", "beta"],
            &["movie-a", "movie-a", "movie-b"],
            &[100.0, 200.0, 300.0],
            &[10, 20, 30],
            &[5, 10, 15],
        );
        write_test_table(tmp.path(), batch).await;

        let input = read_content_consumption(tmp.path().to_str().unwrap(), "exp-1")
            .await
            .unwrap();

        // "alpha" is control (first alphabetically)
        assert_eq!(input.control.len(), 1);
        assert_eq!(input.control[0].content_id, "movie-a");
        assert_eq!(input.control[0].unique_viewers, 10);

        // "beta" is treatment
        assert_eq!(input.treatment.len(), 2);
        assert_eq!(input.total_treatment_viewers, 20); // 5 + 15
    }

    #[tokio::test]
    async fn test_multiple_experiments_filtered() {
        let tmp = TempDir::new().unwrap();
        let batch = make_batch(
            &["exp-1", "exp-1", "exp-2", "exp-2"],
            &["control", "treatment", "control", "treatment"],
            &["movie-a", "movie-b", "movie-c", "movie-d"],
            &[100.0, 200.0, 300.0, 400.0],
            &[10, 20, 30, 40],
            &[5, 10, 15, 20],
        );
        write_test_table(tmp.path(), batch).await;

        let input = read_content_consumption(tmp.path().to_str().unwrap(), "exp-1")
            .await
            .unwrap();

        // Only exp-1 rows
        assert_eq!(input.control.len(), 1);
        assert_eq!(input.treatment.len(), 1);
        assert_eq!(input.control[0].content_id, "movie-a");
        assert_eq!(input.treatment[0].content_id, "movie-b");
    }

    // -----------------------------------------------------------------------
    // metric_summaries tests
    // -----------------------------------------------------------------------

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

    fn make_metric_batch(
        experiment_ids: &[&str],
        user_ids: &[&str],
        variant_ids: &[&str],
        metric_ids: &[&str],
        metric_values: &[f64],
        covariates: &[Option<f64>],
    ) -> RecordBatch {
        let schema = metric_summaries_schema();
        let cov_arr: Float64Array = covariates.iter().copied().collect();
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(experiment_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(StringArray::from(variant_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Float64Array::from(metric_values.to_vec())),
                Arc::new(cov_arr),
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_read_metric_summaries_basic() {
        let tmp = TempDir::new().unwrap();
        let batch = make_metric_batch(
            &["exp-1", "exp-1", "exp-1", "exp-1"],
            &["u1", "u2", "u3", "u4"],
            &["control", "control", "treatment", "treatment"],
            &["ctr", "ctr", "ctr", "ctr"],
            &[0.1, 0.2, 0.3, 0.4],
            &[None, None, None, None],
        );
        write_named_test_table(tmp.path(), "metric_summaries", batch).await;

        let result = read_metric_summaries(tmp.path().to_str().unwrap(), "exp-1")
            .await
            .unwrap();

        assert_eq!(result.metrics.len(), 1);
        let ctr = &result.metrics["ctr"];
        assert_eq!(ctr["control"].len(), 2);
        assert_eq!(ctr["treatment"].len(), 2);
        assert_eq!(result.variant_user_counts["control"], 2);
        assert_eq!(result.variant_user_counts["treatment"], 2);
    }

    #[tokio::test]
    async fn test_read_metric_summaries_with_cuped() {
        let tmp = TempDir::new().unwrap();
        let batch = make_metric_batch(
            &["exp-1", "exp-1", "exp-1", "exp-1"],
            &["u1", "u2", "u3", "u4"],
            &["control", "control", "treatment", "treatment"],
            &["ctr", "ctr", "ctr", "ctr"],
            &[0.1, 0.2, 0.3, 0.4],
            &[Some(1.0), Some(2.0), Some(3.0), Some(4.0)],
        );
        write_named_test_table(tmp.path(), "metric_summaries", batch).await;

        let result = read_metric_summaries(tmp.path().to_str().unwrap(), "exp-1")
            .await
            .unwrap();

        let ctr = &result.metrics["ctr"];
        // All covariates should be Some
        for (_, cov) in &ctr["control"] {
            assert!(cov.is_some());
        }
    }

    #[tokio::test]
    async fn test_read_metric_summaries_empty() {
        let tmp = TempDir::new().unwrap();
        let batch = make_metric_batch(&["exp-1"], &["u1"], &["control"], &["ctr"], &[0.1], &[None]);
        write_named_test_table(tmp.path(), "metric_summaries", batch).await;

        let result = read_metric_summaries(tmp.path().to_str().unwrap(), "exp-999").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no metric_summaries data"));
    }

    #[tokio::test]
    async fn test_read_metric_summaries_multi_metric() {
        let tmp = TempDir::new().unwrap();
        let batch = make_metric_batch(
            &["exp-1", "exp-1", "exp-1", "exp-1"],
            &["u1", "u2", "u1", "u2"],
            &["control", "treatment", "control", "treatment"],
            &["ctr", "ctr", "revenue", "revenue"],
            &[0.1, 0.3, 10.0, 15.0],
            &[None, None, None, None],
        );
        write_named_test_table(tmp.path(), "metric_summaries", batch).await;

        let result = read_metric_summaries(tmp.path().to_str().unwrap(), "exp-1")
            .await
            .unwrap();

        assert_eq!(result.metrics.len(), 2);
        assert!(result.metrics.contains_key("ctr"));
        assert!(result.metrics.contains_key("revenue"));
    }

    // -----------------------------------------------------------------------
    // interleaving_scores tests
    // -----------------------------------------------------------------------

    fn build_interleaving_batch(
        experiment_ids: &[&str],
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

        // Build schema from the actual MapArray's data type to avoid field name mismatches.
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
                Arc::new(StringArray::from(experiment_ids.to_vec())),
                Arc::new(StringArray::from(user_ids.to_vec())),
                Arc::new(map_arr),
                Arc::new(winner_arr),
                Arc::new(Int64Array::from(engagements.to_vec())),
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_read_interleaving_scores_basic() {
        let tmp = TempDir::new().unwrap();
        let batch = build_interleaving_batch(
            &["exp-1", "exp-1", "exp-1"],
            &["u1", "u2", "u3"],
            &[
                vec![("algo_a", 3.0), ("algo_b", 1.0)],
                vec![("algo_a", 2.0), ("algo_b", 4.0)],
                vec![("algo_a", 5.0), ("algo_b", 2.0)],
            ],
            &[Some("algo_a"), Some("algo_b"), Some("algo_a")],
            &[4, 6, 7],
        );
        write_named_test_table(tmp.path(), "interleaving_scores", batch).await;

        let scores = read_interleaving_scores(tmp.path().to_str().unwrap(), "exp-1")
            .await
            .unwrap();

        assert_eq!(scores.len(), 3);
        assert_eq!(scores[0].user_id, "u1");
        assert_eq!(scores[0].algorithm_scores["algo_a"], 3.0);
        assert_eq!(scores[0].algorithm_scores["algo_b"], 1.0);
        assert_eq!(scores[0].winning_algorithm_id, Some("algo_a".to_string()));
        assert_eq!(scores[0].total_engagements, 4);
    }

    #[tokio::test]
    async fn test_read_interleaving_scores_empty() {
        let tmp = TempDir::new().unwrap();
        let batch = build_interleaving_batch(
            &["exp-1"],
            &["u1"],
            &[vec![("algo_a", 3.0), ("algo_b", 1.0)]],
            &[Some("algo_a")],
            &[4],
        );
        write_named_test_table(tmp.path(), "interleaving_scores", batch).await;

        let result = read_interleaving_scores(tmp.path().to_str().unwrap(), "exp-999").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no interleaving_scores data"));
    }

    // -----------------------------------------------------------------------
    // daily_treatment_effects tests
    // -----------------------------------------------------------------------

    fn daily_effects_schema() -> Arc<ArrowSchema> {
        Arc::new(ArrowSchema::new(vec![
            Field::new("experiment_id", DataType::Utf8, false),
            Field::new("metric_id", DataType::Utf8, false),
            Field::new("effect_date", DataType::Date32, false),
            Field::new("absolute_effect", DataType::Float64, false),
            Field::new("sample_size", DataType::Int64, false),
        ]))
    }

    fn make_daily_effects_batch(
        experiment_ids: &[&str],
        metric_ids: &[&str],
        dates: &[i32],
        effects: &[f64],
        sizes: &[i64],
    ) -> RecordBatch {
        let schema = daily_effects_schema();
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(experiment_ids.to_vec())),
                Arc::new(StringArray::from(metric_ids.to_vec())),
                Arc::new(Date32Array::from(dates.to_vec())),
                Arc::new(Float64Array::from(effects.to_vec())),
                Arc::new(Int64Array::from(sizes.to_vec())),
            ],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_read_daily_effects_basic() {
        let tmp = TempDir::new().unwrap();
        // 10 days of data for one metric (days since epoch: 19700+)
        let base_date = 19700i32;
        let dates: Vec<i32> = (0..10).map(|i| base_date + i).collect();
        let effects: Vec<f64> = (0..10)
            .map(|i| 5.0 + 3.0 * (-(i as f64) / 4.0).exp())
            .collect();
        let sizes: Vec<i64> = vec![1000; 10];
        let exp_ids: Vec<&str> = vec!["exp-1"; 10];
        let metric_ids: Vec<&str> = vec!["ctr"; 10];

        let batch = make_daily_effects_batch(&exp_ids, &metric_ids, &dates, &effects, &sizes);
        write_named_test_table(tmp.path(), "daily_treatment_effects", batch).await;

        let (metric_id, daily_effects) =
            read_daily_treatment_effects(tmp.path().to_str().unwrap(), "exp-1")
                .await
                .unwrap();

        assert_eq!(metric_id, "ctr");
        assert_eq!(daily_effects.len(), 10);
        // Days should be sequential from 0
        assert_eq!(daily_effects[0].day, 0);
        assert_eq!(daily_effects[9].day, 9);
    }

    #[tokio::test]
    async fn test_read_daily_effects_multi_metric() {
        let tmp = TempDir::new().unwrap();
        // 10 rows for "ctr" + 5 rows for "revenue" → should pick "ctr"
        let base = 19700i32;
        let exp_ids = vec!["exp-1"; 15];
        let mut metric_ids: Vec<&str> = vec!["ctr"; 10];
        metric_ids.extend(vec!["revenue"; 5]);
        let mut dates: Vec<i32> = (0..10).map(|i| base + i).collect();
        dates.extend((0..5).map(|i| base + i));
        let mut effects: Vec<f64> = (0..10).map(|i| 5.0 + 0.1 * i as f64).collect();
        effects.extend((0..5).map(|i| 10.0 + 0.1 * i as f64));
        let sizes = vec![1000i64; 15];

        let batch = make_daily_effects_batch(&exp_ids, &metric_ids, &dates, &effects, &sizes);
        write_named_test_table(tmp.path(), "daily_treatment_effects", batch).await;

        let (metric_id, daily_effects) =
            read_daily_treatment_effects(tmp.path().to_str().unwrap(), "exp-1")
                .await
                .unwrap();

        assert_eq!(metric_id, "ctr"); // most data points
        assert_eq!(daily_effects.len(), 10);
    }

    #[tokio::test]
    async fn test_read_daily_effects_empty() {
        let tmp = TempDir::new().unwrap();
        let batch = make_daily_effects_batch(&["exp-1"], &["ctr"], &[19700], &[5.0], &[1000]);
        write_named_test_table(tmp.path(), "daily_treatment_effects", batch).await;

        let result = read_daily_treatment_effects(tmp.path().to_str().unwrap(), "exp-999").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no daily_treatment_effects data"));
    }
}
