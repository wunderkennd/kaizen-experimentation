//! Delta Lake reader for content consumption data.
//!
//! Reads `content_consumption` tables from Delta Lake, filtered by experiment_id,
//! and maps the result into `InterferenceInput` for the stats crate.

use anyhow::{bail, Context};
use deltalake::arrow::array::{Array, Float64Array, Int64Array, StringArray};
use deltalake::arrow::record_batch::RecordBatch;
use deltalake::DeltaTable;
use experimentation_stats::interference::{ContentConsumption, InterferenceInput};
use std::collections::HashMap;
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
async fn extract_rows(table: &DeltaTable, experiment_id: &str) -> anyhow::Result<Vec<ConsumptionRow>> {
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
        by_variant.entry(row.variant_id.clone()).or_default().push(row);
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


#[cfg(test)]
mod tests {
    use super::*;
    use deltalake::arrow::array::{Float64Array, Int64Array, StringArray};
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

    /// Write a single-batch Delta table into a temp directory.
    async fn write_test_table(dir: &std::path::Path, batch: RecordBatch) {
        let table_path = dir.join("content_consumption");
        std::fs::create_dir_all(&table_path).unwrap();

        let ops = DeltaOps::try_from_uri(table_path.to_str().unwrap())
            .await
            .unwrap();
        ops.write(vec![batch]).await.unwrap();
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
}
