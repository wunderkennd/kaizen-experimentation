//! Migration report generator for Task A6 (ADR-026 Phase 3 #437).
//!
//! Consumes a `Vec<ReportEntry>` (keyed by original metric ID) and generates:
//! - **JSON report** (machine-readable; for apply subcommand + future M6 dashboard)
//! - **Markdown summary** (human-readable; printed by translate subcommand)
//!
//! ## Report structure
//!
//! Both output formats distinguish between:
//! - **Tier 1**: FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT — direct structural match
//! - **Tier 2**: METRICQL — translatable via MetricQL expression emitter
//! - **Tier 3**: Untranslatable — either parse failure or matched no pattern
//!
//! The Tier 3 field (parse_error) allows operators to distinguish:
//! - "did not parse" (Some(...))
//! - "parsed but no matching pattern" (None)

use serde_json::json;

use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig, MetricDefinition,
};

use crate::migration::ClassificationResult;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single entry in the migration report, coupling the original metric ID
/// with its classification result.
pub struct ReportEntry {
    pub original_metric_id: String,
    pub result: ClassificationResult,
}

/// Summary counts of all tier outcomes in a batch.
#[derive(serde::Serialize)]
pub struct ReportSummary {
    pub total: usize,
    pub tier1_filtered_mean: usize,
    pub tier1_composite: usize,
    pub tier1_windowed_count: usize,
    pub tier2_metricql: usize,
    pub tier3_untranslatable: usize,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Convert a `MetricDefinition` proto to a JSON value.
///
/// This manually builds the JSON object since proto types don't auto-implement
/// serde::Serialize. Only includes essential fields for the report.
fn metric_to_json(metric: &MetricDefinition) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    obj.insert("metric_id".to_string(), json!(metric.metric_id));
    obj.insert("name".to_string(), json!(metric.name));
    obj.insert("type".to_string(), json!(metric.r#type));

    if !metric.description.is_empty() {
        obj.insert("description".to_string(), json!(metric.description));
    }

    if !metric.source_event_type.is_empty() {
        obj.insert("source_event_type".to_string(), json!(metric.source_event_type));
    }

    // Include type-specific config fields (oneof type_config)
    if let Some(ref type_cfg) = metric.type_config {
        match type_cfg {
            TypeConfig::FilteredMean(fm) => {
                obj.insert(
                    "filtered_mean".to_string(),
                    json!({
                        "filter_sql": fm.filter_sql,
                        "value_column": fm.value_column,
                    }),
                );
            }
            TypeConfig::Composite(comp) => {
                obj.insert(
                    "composite".to_string(),
                    json!({
                        "operands": comp.operands.iter().map(|op| json!({
                            "metric_id": op.metric_id,
                            "weight": op.weight,
                        })).collect::<Vec<_>>(),
                        "operator": comp.operator,
                    }),
                );
            }
            TypeConfig::WindowedCount(wc) => {
                obj.insert(
                    "windowed_count".to_string(),
                    json!({
                        "event_type": wc.event_type,
                        "filter_sql": wc.filter_sql,
                        "window_hours": wc.window_hours,
                    }),
                );
            }
        }
    }

    if !metric.metricql_expression.is_empty() {
        obj.insert(
            "metricql_expression".to_string(),
            json!(metric.metricql_expression),
        );
    }

    if !metric.custom_sql.is_empty() {
        obj.insert("custom_sql".to_string(), json!(metric.custom_sql));
    }

    serde_json::Value::Object(obj)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a summary from a batch of report entries.
pub fn build_summary(entries: &[ReportEntry]) -> ReportSummary {
    let mut tier1_filtered_mean = 0usize;
    let mut tier1_composite = 0usize;
    let mut tier1_windowed_count = 0usize;
    let mut tier2_metricql = 0usize;
    let mut tier3_untranslatable = 0usize;

    for entry in entries {
        match &entry.result {
            ClassificationResult::Tier1Filtered { .. } => tier1_filtered_mean += 1,
            ClassificationResult::Tier1Composite { .. } => tier1_composite += 1,
            ClassificationResult::Tier1WindowedCount { .. } => tier1_windowed_count += 1,
            ClassificationResult::Tier2Metricql { .. } => tier2_metricql += 1,
            ClassificationResult::Tier3Untranslatable { .. } => tier3_untranslatable += 1,
        }
    }

    ReportSummary {
        total: entries.len(),
        tier1_filtered_mean,
        tier1_composite,
        tier1_windowed_count,
        tier2_metricql,
        tier3_untranslatable,
    }
}

/// Render a batch of report entries as a JSON object.
///
/// JSON shape:
/// ```json
/// {
///   "summary": { ... },
///   "entries": [
///     {
///       "original_metric_id": "...",
///       "tier": "tier1_filtered_mean",
///       "reason": "...",
///       "proposal": { /* full MetricDefinition */ },
///       "parse_error": null
///     },
///     ...
///   ]
/// }
/// ```
///
/// For Tier 3 entries, `proposal` is always `null`.
/// For Tier 1/2 entries, `parse_error` is always `null`.
pub fn render_json(entries: &[ReportEntry]) -> serde_json::Result<String> {
    let summary = build_summary(entries);

    let mut json_entries = Vec::new();
    for entry in entries {
        let (tier, reason, proposal, parse_error) = match &entry.result {
            ClassificationResult::Tier1Filtered { proposal, reason } => (
                "tier1_filtered_mean",
                reason.clone(),
                Some(metric_to_json(proposal)),
                None,
            ),
            ClassificationResult::Tier1Composite { proposal, reason } => (
                "tier1_composite",
                reason.clone(),
                Some(metric_to_json(proposal)),
                None,
            ),
            ClassificationResult::Tier1WindowedCount { proposal, reason } => (
                "tier1_windowed_count",
                reason.clone(),
                Some(metric_to_json(proposal)),
                None,
            ),
            ClassificationResult::Tier2Metricql { proposal, reason } => (
                "tier2_metricql",
                reason.clone(),
                Some(metric_to_json(proposal)),
                None,
            ),
            ClassificationResult::Tier3Untranslatable { reason, parse_error } => (
                "tier3_untranslatable",
                reason.clone(),
                None,
                parse_error.clone(),
            ),
        };

        json_entries.push(json!({
            "original_metric_id": &entry.original_metric_id,
            "tier": tier,
            "reason": reason,
            "proposal": proposal,
            "parse_error": parse_error,
        }));
    }

    let report = json!({
        "summary": summary,
        "entries": json_entries,
    });

    serde_json::to_string_pretty(&report)
}

/// Render a batch of report entries as a human-readable Markdown summary.
///
/// Entries are grouped by tier. Tier 3 entries are clearly separated.
/// For Tier 1/2 entries, the proposal is NOT inlined (too verbose); only
/// metric_id, type, and relevant config fields are mentioned. The JSON
/// report contains the full detail.
pub fn render_markdown(entries: &[ReportEntry]) -> String {
    let summary = build_summary(entries);

    let mut md = String::new();

    // Header
    md.push_str("# CUSTOM Metric Migration Report\n\n");

    // Summary section
    md.push_str("## Summary\n\n");
    md.push_str(&format!("- **Total CUSTOM metrics analyzed:** {}\n", summary.total));
    md.push_str("- **Tier 1 (structured):**\n");
    md.push_str(&format!(
        "  - FILTERED_MEAN: {}\n",
        summary.tier1_filtered_mean
    ));
    md.push_str(&format!("  - COMPOSITE: {}\n", summary.tier1_composite));
    md.push_str(&format!(
        "  - WINDOWED_COUNT: {}\n",
        summary.tier1_windowed_count
    ));
    md.push_str(&format!("- **Tier 2 (METRICQL):** {}\n", summary.tier2_metricql));
    md.push_str(&format!(
        "- **Tier 3 (un-translatable):** {}\n\n",
        summary.tier3_untranslatable
    ));

    // Entries section
    md.push_str("## Entries\n\n");

    // Group by tier
    let mut tier1_entries = Vec::new();
    let mut tier2_entries = Vec::new();
    let mut tier3_entries = Vec::new();

    for entry in entries {
        match &entry.result {
            ClassificationResult::Tier1Filtered { .. }
            | ClassificationResult::Tier1Composite { .. }
            | ClassificationResult::Tier1WindowedCount { .. } => {
                tier1_entries.push(entry);
            }
            ClassificationResult::Tier2Metricql { .. } => {
                tier2_entries.push(entry);
            }
            ClassificationResult::Tier3Untranslatable { .. } => {
                tier3_entries.push(entry);
            }
        }
    }

    // Tier 1 entries
    for entry in &tier1_entries {
        render_markdown_tier1_entry(&mut md, entry);
    }

    // Tier 2 entries
    for entry in &tier2_entries {
        render_markdown_tier2_entry(&mut md, entry);
    }

    // Tier 3 entries
    for entry in &tier3_entries {
        render_markdown_tier3_entry(&mut md, entry);
    }

    md
}

// ---------------------------------------------------------------------------
// Markdown entry renderers
// ---------------------------------------------------------------------------

fn render_markdown_tier1_entry(md: &mut String, entry: &ReportEntry) {
    let (tier_tag, metric_type, proposal) = match &entry.result {
        ClassificationResult::Tier1Filtered { proposal, reason: _ } => {
            ("tier1_filtered_mean", "FILTERED_MEAN", proposal)
        }
        ClassificationResult::Tier1Composite { proposal, reason: _ } => {
            ("tier1_composite", "COMPOSITE", proposal)
        }
        ClassificationResult::Tier1WindowedCount { proposal, reason: _ } => {
            ("tier1_windowed_count", "WINDOWED_COUNT", proposal)
        }
        _ => return,
    };

    let reason = match &entry.result {
        ClassificationResult::Tier1Filtered { reason, .. }
        | ClassificationResult::Tier1Composite { reason, .. }
        | ClassificationResult::Tier1WindowedCount { reason, .. } => reason.clone(),
        _ => String::new(),
    };

    md.push_str(&format!(
        "### {} → {}\n\n",
        entry.original_metric_id, metric_type
    ));
    md.push_str(&format!("**Tier:** {}\n\n", tier_tag));
    md.push_str(&format!("**Reason:** {}\n\n", reason));
    md.push_str("**Proposal:**\n\n");
    md.push_str("```json\n");

    let proposal_json = metric_to_json(proposal);
    let proposal_str = serde_json::to_string_pretty(&proposal_json).unwrap_or_default();
    md.push_str(&proposal_str);
    md.push_str("\n```\n\n");
}

fn render_markdown_tier2_entry(md: &mut String, entry: &ReportEntry) {
    let (tier_tag, metric_type, proposal, reason) =
        match &entry.result {
            ClassificationResult::Tier2Metricql { proposal, reason } => {
                ("tier2_metricql", "METRICQL", proposal, reason.clone())
            }
            _ => return,
        };

    md.push_str(&format!(
        "### {} → {}\n\n",
        entry.original_metric_id, metric_type
    ));
    md.push_str(&format!("**Tier:** {}\n\n", tier_tag));
    md.push_str(&format!("**Reason:** {}\n\n", reason));
    md.push_str("**Proposal:**\n\n");
    md.push_str("```json\n");

    let proposal_json = metric_to_json(proposal);
    let proposal_str = serde_json::to_string_pretty(&proposal_json).unwrap_or_default();
    md.push_str(&proposal_str);
    md.push_str("\n```\n\n");
}

fn render_markdown_tier3_entry(md: &mut String, entry: &ReportEntry) {
    let (tier_tag, reason, parse_error) = match &entry.result {
        ClassificationResult::Tier3Untranslatable { reason, parse_error } => {
            ("tier3_untranslatable", reason.clone(), parse_error.clone())
        }
        _ => return,
    };

    md.push_str(&format!(
        "### {} → Tier 3 (un-translatable)\n\n",
        entry.original_metric_id
    ));
    md.push_str(&format!("**Tier:** {}\n\n", tier_tag));
    md.push_str(&format!("**Reason:** {}\n\n", reason));

    if let Some(pe) = parse_error {
        md.push_str(&format!("**Parse error:** {}\n\n", pe));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use experimentation_proto::experimentation::common::v1::{
        MetricDefinition, MetricType,
    };

    fn dummy_metric(id: &str) -> MetricDefinition {
        MetricDefinition {
            metric_id: id.to_string(),
            name: format!("{} name", id),
            r#type: MetricType::FilteredMean as i32,
            ..Default::default()
        }
    }

    #[test]
    fn summary_counts_all_tiers() {
        let entries = vec![
            ReportEntry {
                original_metric_id: "m1".to_string(),
                result: ClassificationResult::Tier1Filtered {
                    proposal: dummy_metric("m1"),
                    reason: "reason1".to_string(),
                },
            },
            ReportEntry {
                original_metric_id: "m2".to_string(),
                result: ClassificationResult::Tier1Composite {
                    proposal: dummy_metric("m2"),
                    reason: "reason2".to_string(),
                },
            },
            ReportEntry {
                original_metric_id: "m3".to_string(),
                result: ClassificationResult::Tier1WindowedCount {
                    proposal: dummy_metric("m3"),
                    reason: "reason3".to_string(),
                },
            },
            ReportEntry {
                original_metric_id: "m4".to_string(),
                result: ClassificationResult::Tier2Metricql {
                    proposal: dummy_metric("m4"),
                    reason: "reason4".to_string(),
                },
            },
            ReportEntry {
                original_metric_id: "m5".to_string(),
                result: ClassificationResult::Tier3Untranslatable {
                    reason: "reason5".to_string(),
                    parse_error: Some("parse error".to_string()),
                },
            },
        ];

        let summary = build_summary(&entries);

        assert_eq!(summary.total, 5);
        assert_eq!(summary.tier1_filtered_mean, 1);
        assert_eq!(summary.tier1_composite, 1);
        assert_eq!(summary.tier1_windowed_count, 1);
        assert_eq!(summary.tier2_metricql, 1);
        assert_eq!(summary.tier3_untranslatable, 1);
    }

    #[test]
    fn json_render_includes_all_fields() {
        let entries = vec![
            ReportEntry {
                original_metric_id: "test_metric".to_string(),
                result: ClassificationResult::Tier1Filtered {
                    proposal: dummy_metric("test_metric"),
                    reason: "matches FILTERED_MEAN shape".to_string(),
                },
            },
            ReportEntry {
                original_metric_id: "bad_metric".to_string(),
                result: ClassificationResult::Tier3Untranslatable {
                    reason: "parse failed".to_string(),
                    parse_error: Some("unexpected token".to_string()),
                },
            },
        ];

        let json_str = render_json(&entries).expect("render_json should succeed");
        let json_obj: serde_json::Value =
            serde_json::from_str(&json_str).expect("valid JSON");

        // Check summary exists
        assert!(json_obj.get("summary").is_some());
        let summary = &json_obj["summary"];
        assert_eq!(summary["total"], 2);
        assert_eq!(summary["tier1_filtered_mean"], 1);
        assert_eq!(summary["tier3_untranslatable"], 1);

        // Check entries exist
        assert!(json_obj.get("entries").is_some());
        let entries_arr = json_obj["entries"]
            .as_array()
            .expect("entries must be array");
        assert_eq!(entries_arr.len(), 2);

        // Check Tier 1 entry structure
        let tier1_entry = &entries_arr[0];
        assert_eq!(tier1_entry["original_metric_id"], "test_metric");
        assert_eq!(tier1_entry["tier"], "tier1_filtered_mean");
        assert!(tier1_entry["proposal"].is_object());
        assert!(tier1_entry["parse_error"].is_null());

        // Check Tier 3 entry structure
        let tier3_entry = &entries_arr[1];
        assert_eq!(tier3_entry["original_metric_id"], "bad_metric");
        assert_eq!(tier3_entry["tier"], "tier3_untranslatable");
        assert!(tier3_entry["proposal"].is_null());
        assert_eq!(tier3_entry["parse_error"], "unexpected token");
    }

    #[test]
    fn markdown_render_includes_all_sections() {
        let entries = vec![
            ReportEntry {
                original_metric_id: "filtered_metric".to_string(),
                result: ClassificationResult::Tier1Filtered {
                    proposal: dummy_metric("filtered_metric"),
                    reason: "matches FILTERED_MEAN shape".to_string(),
                },
            },
            ReportEntry {
                original_metric_id: "metricql_metric".to_string(),
                result: ClassificationResult::Tier2Metricql {
                    proposal: dummy_metric("metricql_metric"),
                    reason: "translatable via MetricQL".to_string(),
                },
            },
            ReportEntry {
                original_metric_id: "bad_metric".to_string(),
                result: ClassificationResult::Tier3Untranslatable {
                    reason: "parse failed".to_string(),
                    parse_error: Some("syntax error at line 5".to_string()),
                },
            },
        ];

        let md = render_markdown(&entries);

        // Check header
        assert!(md.contains("# CUSTOM Metric Migration Report"));

        // Check summary section
        assert!(md.contains("## Summary"));
        assert!(md.contains("analyzed:** 3"), "markdown should show total count");
        assert!(md.contains("FILTERED_MEAN: 1"));
        assert!(md.contains("COMPOSITE: 0"));
        assert!(md.contains("WINDOWED_COUNT: 0"));
        assert!(md.contains("METRICQL):** 1"), "should show tier 2 count");
        assert!(md.contains("un-translatable):** 1"), "should show tier 3 count");

        // Check entries section
        assert!(md.contains("## Entries"));

        // Check Tier 1 entry
        assert!(md.contains("filtered_metric → FILTERED_MEAN"));
        assert!(md.contains("tier1_filtered_mean"));

        // Check Tier 2 entry
        assert!(md.contains("metricql_metric → METRICQL"));
        assert!(md.contains("tier2_metricql"));

        // Check Tier 3 entry
        assert!(md.contains("bad_metric → Tier 3 (un-translatable)"));
        assert!(md.contains("syntax error at line 5"));
    }
}
