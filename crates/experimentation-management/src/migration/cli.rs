//! Public subcommand logic for the `custom_migrator` CLI binary (Task A7,
//! ADR-026 Phase 3 #437).
//!
//! Each `*_subcommand` function contains the business logic for one CLI
//! subcommand. They are `pub` so that the integration test
//! (`tests/custom_migrator_cli_test.rs`) can invoke them directly without
//! shelling out to the binary, avoiding the need for a live M5 gRPC server.
//!
//! ## MigratorLookup — permissive at translate-time
//!
//! The translate subcommand uses a `MigratorLookup` built from the scan
//! output (a `HashSet` of known metric IDs). This lookup answers
//! `exists_all_metrics` from that set and returns defaults for every other
//! method. Cycle detection and COMPOSITE operand validation are therefore
//! only approximated during translation.
//!
//! **This is intentional and documented here:** the CLI does not enforce
//! cycle detection at translate-time. The operator review step (the proposals
//! JSON) and M5's `MigrateMetricDefinition` RPC (Phase C) will re-validate
//! with a real store-backed lookup before anything is applied. The permissive
//! lookup exists solely to let `classify_and_translate` run without a live
//! gRPC connection, making offline dry-runs possible.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use experimentation_proto::experimentation::common::v1::{MetricDefinition, MetricType};
use serde_json::Value;

use crate::migration::{classify_and_translate, report::ReportEntry};
use crate::store::StoreError;
use crate::validators::MetricLookup;

// ---------------------------------------------------------------------------
// MigratorLookup — offline permissive lookup (see module-level doc)
// ---------------------------------------------------------------------------

/// Permissive `MetricLookup` backed by the set of metric IDs seen in the scan
/// output.
///
/// ## Why this is sufficient for translate-time
///
/// * `exists_all_metrics` — answers from the known-id set. Any metric the scan
///   returned is considered present. External refs (e.g., COMPOSITE operands
///   that are not CUSTOM) will resolve `false`, causing those translate calls to
///   fall through to Tier 3 rather than producing a wrong proposal — which is
///   the safe direction (conservative-by-default per ADR-026 L4).
///
/// * `get_composite_operands` / `get_metricql_refs` / `get_metric_type` — return
///   empty/default. These are only called by the DFS cycle detector, which is
///   vacuously safe here: if we can't resolve the full graph offline, we treat
///   nodes as leaves. The Tier 1 COMPOSITE translator will still produce a
///   proposal because `exists_all_metrics` succeeds for operands that are in the
///   scan corpus; cycle detection across those operands terminates trivially.
///
/// M5's `MigrateMetricDefinition` RPC (Phase C) will re-validate everything
/// with a live `ManagementStore`-backed lookup before applying any change.
pub struct MigratorLookup {
    known_ids: HashSet<String>,
}

impl MigratorLookup {
    /// Build a lookup from a set of metric IDs (typically the scan corpus).
    pub fn from_ids(ids: impl IntoIterator<Item = String>) -> Self {
        Self { known_ids: ids.into_iter().collect() }
    }
}

#[tonic::async_trait]
impl MetricLookup for MigratorLookup {
    async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
        Ok(metric_ids.iter().all(|id| self.known_ids.contains(*id)))
    }

    async fn get_composite_operands(
        &self,
        _metric_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        // Treat every node as a leaf. Cycle detection terminates immediately.
        Ok(vec![])
    }

    async fn get_metricql_refs(
        &self,
        _metric_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        Ok(vec![])
    }

    async fn get_metric_type(
        &self,
        metric_id: &str,
    ) -> Result<MetricType, StoreError> {
        if self.known_ids.contains(metric_id) {
            // Report all known IDs as leaves (FILTERED_MEAN) so DFS does not
            // try to descend further. The precise type is irrelevant at this
            // stage; only Phase C needs to resolve it accurately.
            Ok(MetricType::FilteredMean)
        } else {
            Err(StoreError::NotFound(metric_id.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// translate_subcommand — public for testability
// ---------------------------------------------------------------------------

/// Parse `metric_definitions` JSON from `report_path` (scan output), run
/// `classify_and_translate` on each entry, then write the proposals JSON to
/// `output_path` and optionally a Markdown summary to `markdown_path`.
///
/// Returns the tier counts for the caller (primarily the integration test).
pub async fn translate_subcommand(
    report_path: &Path,
    output_path: &Path,
    markdown_path: Option<&Path>,
) -> Result<TierCounts> {
    // 1. Read scan output.
    let raw = fs::read_to_string(report_path)
        .with_context(|| format!("reading scan report from {}", report_path.display()))?;
    let json: Value = serde_json::from_str(&raw)
        .with_context(|| format!("parsing scan report JSON from {}", report_path.display()))?;

    // The scan output is a JSON array of MetricDefinition-like objects.
    let arr = json
        .as_array()
        .with_context(|| format!("scan report must be a JSON array, got: {}", report_path.display()))?;

    // 2. Reconstruct MetricDefinition values from the JSON array.
    //    We use the fields written by the scan subcommand (metric_id, name,
    //    type, custom_sql, etc.).
    let mut metrics: Vec<MetricDefinition> = Vec::with_capacity(arr.len());
    for item in arr {
        let m = json_to_metric_definition(item)?;
        metrics.push(m);
    }

    // 3. Build the permissive lookup from all scanned metric IDs.
    let known_ids = metrics.iter().map(|m| m.metric_id.clone()).collect::<Vec<_>>();
    let lookup = MigratorLookup::from_ids(known_ids);

    // 4. Classify and translate each metric.
    let mut entries: Vec<ReportEntry> = Vec::with_capacity(metrics.len());
    for m in &metrics {
        let sql = m.custom_sql.as_str();
        let result = classify_and_translate(sql, m, &lookup).await;
        entries.push(ReportEntry {
            original_metric_id: m.metric_id.clone(),
            result,
        });
    }

    // 5. Render JSON proposals.
    let json_out = crate::migration::report::render_json(&entries)
        .context("rendering proposals JSON")?;
    fs::write(output_path, &json_out)
        .with_context(|| format!("writing proposals JSON to {}", output_path.display()))?;

    // 6. Optionally render Markdown.
    if let Some(md_path) = markdown_path {
        let md_out = crate::migration::report::render_markdown(&entries);
        fs::write(md_path, md_out)
            .with_context(|| format!("writing Markdown summary to {}", md_path.display()))?;
    }

    // 7. Compute and return tier counts.
    let summary = crate::migration::report::build_summary(&entries);
    Ok(TierCounts {
        total: summary.total,
        tier1_filtered_mean: summary.tier1_filtered_mean,
        tier1_composite: summary.tier1_composite,
        tier1_windowed_count: summary.tier1_windowed_count,
        tier2_metricql: summary.tier2_metricql,
        tier3_untranslatable: summary.tier3_untranslatable,
    })
}

/// Summary counts returned by `translate_subcommand`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierCounts {
    pub total: usize,
    pub tier1_filtered_mean: usize,
    pub tier1_composite: usize,
    pub tier1_windowed_count: usize,
    pub tier2_metricql: usize,
    pub tier3_untranslatable: usize,
}

// ---------------------------------------------------------------------------
// JSON → MetricDefinition conversion
// ---------------------------------------------------------------------------

/// Reconstruct a `MetricDefinition` from the JSON object written by `scan`.
///
/// Only the fields that the scan serializes are read; everything else stays at
/// proto3 defaults. `custom_sql` is the field the classifier reads.
fn json_to_metric_definition(v: &Value) -> Result<MetricDefinition> {
    use experimentation_proto::experimentation::common::v1::{
        metric_definition::TypeConfig, CompositeConfig, CompositeOperand, FilteredMeanConfig,
        WindowedCountConfig,
    };

    let obj = v.as_object().context("each scan entry must be a JSON object")?;

    let metric_id = obj
        .get("metric_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let name = obj
        .get("name")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let description = obj
        .get("description")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let source_event_type = obj
        .get("source_event_type")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let raw_type = obj.get("type").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
    let custom_sql = obj
        .get("custom_sql")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let metricql_expression = obj
        .get("metricql_expression")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    // Reconstruct type_config from the serialized nested objects.
    let type_config = if let Some(fm) = obj.get("filtered_mean") {
        let filter_sql = fm
            .get("filter_sql")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let value_column = fm
            .get("value_column")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        Some(TypeConfig::FilteredMean(FilteredMeanConfig {
            filter_sql,
            value_column,
        }))
    } else if let Some(wc) = obj.get("windowed_count") {
        let event_type = wc
            .get("event_type")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let filter_sql = wc
            .get("filter_sql")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let window_hours = wc
            .get("window_hours")
            .and_then(|n| n.as_i64())
            .unwrap_or(0) as i32;
        Some(TypeConfig::WindowedCount(WindowedCountConfig {
            event_type,
            filter_sql,
            window_hours,
        }))
    } else if let Some(comp) = obj.get("composite") {
        let operator = comp
            .get("operator")
            .and_then(|n| n.as_i64())
            .unwrap_or(0) as i32;
        let operands = comp
            .get("operands")
            .and_then(|arr| arr.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|op| CompositeOperand {
                        metric_id: op
                            .get("metric_id")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        weight: op
                            .get("weight")
                            .and_then(|n| n.as_f64())
                            .unwrap_or(0.0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(TypeConfig::Composite(CompositeConfig { operator, operands }))
    } else {
        None
    };

    Ok(MetricDefinition {
        metric_id,
        name,
        description,
        source_event_type,
        r#type: raw_type,
        custom_sql,
        metricql_expression,
        type_config,
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use experimentation_proto::experimentation::common::v1::MetricType;

    #[test]
    fn migrator_lookup_returns_true_for_known_ids() {
        let lookup = MigratorLookup::from_ids(
            vec!["a".to_string(), "b".to_string()]
        );

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            assert!(lookup.exists_all_metrics(&["a", "b"]).await.unwrap());
            assert!(!lookup.exists_all_metrics(&["a", "ghost"]).await.unwrap());
            assert!(lookup.exists_all_metrics(&[]).await.unwrap());
        });
    }

    #[test]
    fn migrator_lookup_get_composite_operands_returns_empty() {
        let lookup = MigratorLookup::from_ids(vec!["x".to_string()]);

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let ops = lookup.get_composite_operands("x").await.unwrap();
            assert!(ops.is_empty());
        });
    }

    #[test]
    fn migrator_lookup_get_metric_type_known_returns_filtered_mean() {
        let lookup = MigratorLookup::from_ids(vec!["x".to_string()]);

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let t = lookup.get_metric_type("x").await.unwrap();
            assert_eq!(t, MetricType::FilteredMean);
        });
    }

    #[test]
    fn migrator_lookup_get_metric_type_unknown_returns_not_found() {
        let lookup = MigratorLookup::from_ids(vec![]);

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let err = lookup.get_metric_type("ghost").await.unwrap_err();
            assert!(matches!(err, StoreError::NotFound(_)));
        });
    }

    #[test]
    fn json_to_metric_definition_reads_custom_sql() {
        let v = serde_json::json!({
            "metric_id": "m1",
            "name": "M1",
            "type": 6,
            "custom_sql": "SELECT AVG(x) FROM t"
        });
        let m = json_to_metric_definition(&v).unwrap();
        assert_eq!(m.metric_id, "m1");
        assert_eq!(m.custom_sql, "SELECT AVG(x) FROM t");
        assert_eq!(m.r#type, 6);
    }

    #[test]
    fn json_to_metric_definition_handles_missing_custom_sql() {
        let v = serde_json::json!({
            "metric_id": "m2",
            "name": "M2",
            "type": 6
        });
        let m = json_to_metric_definition(&v).unwrap();
        assert_eq!(m.custom_sql, "");
    }
}
