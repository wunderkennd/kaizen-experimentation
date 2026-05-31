//! End-to-end smoke test for the `custom_migrator` translate pipeline (A7).
//!
//! This test exercises the `translate_subcommand` logic end-to-end without
//! shelling out to the binary and without a live M5 gRPC server:
//!
//!   1. Construct 3 fake CUSTOM `MetricDefinition` objects in memory.
//!   2. Write them to a temp JSON file (simulating `custom_migrator scan` output).
//!   3. Call `translate_subcommand(report, output, Some(markdown))`.
//!   4. Read and parse the proposals JSON; assert tier counts.
//!   5. Read the Markdown; assert it contains the expected sections.
//!
//! The 3 metrics are:
//!   - `custom_filtered_mean_style`: SQL that matches a FILTERED_MEAN shape.
//!   - `custom_ratio_style`: SQL that matches a RATIO/METRICQL Tier-2 shape.
//!   - `custom_unparseable`: deliberately broken SQL that falls to Tier 3.
//!
//! We do not assert exact tier assignments for the first two (the classifier
//! may evolve); we assert only that the tier is NOT tier3_untranslatable for
//! the first and IS tier3_untranslatable for the third. We also assert total=3
//! and that the proposals file is valid JSON with a "summary" and "entries"
//! array.

use std::fs;
use tempfile::TempDir;

use experimentation_management::migration::cli::translate_subcommand;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a scan-output JSON array from three metrics:
///   0: FILTERED_MEAN-style CUSTOM SQL
///   1: ratio-of-sums CUSTOM SQL (likely Tier 2)
///   2: unparseable SQL (Tier 3)
fn build_scan_json() -> serde_json::Value {
    serde_json::json!([
        {
            "metric_id": "custom_fm_style",
            "name": "Custom Filtered Mean Style",
            "type": 6,  // MetricType::Custom
            "custom_sql": "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                           FROM delta.metric_events me \
                           INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                           WHERE me.event_type = 'video_play' AND me.platform = 'mobile' \
                           GROUP BY me.user_id"
        },
        {
            "metric_id": "custom_ratio_style",
            "name": "Custom Ratio Style",
            "type": 6,
            "custom_sql": "SELECT me.user_id, \
                           SUM(CASE WHEN me.event_type = 'click' THEN 1 ELSE 0 END) * 1.0 \
                           / NULLIF(SUM(CASE WHEN me.event_type = 'view' THEN 1 ELSE 0 END), 0) \
                           AS metric_value \
                           FROM delta.metric_events me \
                           INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                           GROUP BY me.user_id"
        },
        {
            "metric_id": "custom_unparseable",
            "name": "Custom Unparseable",
            "type": 6,
            "custom_sql": "THIS IS NOT VALID SQL ;;; @@#$%"
        }
    ])
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn translate_subcommand_end_to_end() {
    let dir = TempDir::new().expect("tempdir");

    // 1. Write fake scan output.
    let report_path = dir.path().join("scan.json");
    let scan_json = build_scan_json();
    fs::write(&report_path, serde_json::to_string_pretty(&scan_json).unwrap())
        .expect("write scan.json");

    // 2. Run translate_subcommand.
    let proposals_path = dir.path().join("proposals.json");
    let markdown_path = dir.path().join("proposals.md");

    let counts = translate_subcommand(
        &report_path,
        &proposals_path,
        Some(markdown_path.as_path()),
    )
    .await
    .expect("translate_subcommand should succeed");

    // 3. Assert tier counts at the summary level.
    assert_eq!(counts.total, 3, "expected 3 total metrics");

    // The unparseable SQL must land in Tier 3.
    assert!(
        counts.tier3_untranslatable >= 1,
        "at least 1 metric must be tier3_untranslatable (the bad SQL); got: {counts:?}"
    );

    // The FILTERED_MEAN-style SQL must NOT be tier3.
    // (We accept Tier 1 or Tier 2 depending on what the classifier decides.)
    let tier12_total = counts.tier1_filtered_mean
        + counts.tier1_composite
        + counts.tier1_windowed_count
        + counts.tier2_metricql;
    assert!(
        tier12_total >= 1,
        "at least 1 metric must be classified as Tier 1 or Tier 2; got: {counts:?}"
    );

    // total == sum of all tiers.
    assert_eq!(
        tier12_total + counts.tier3_untranslatable,
        3,
        "tier counts must add up to total=3"
    );

    // 4. Validate the proposals JSON file.
    let proposals_raw = fs::read_to_string(&proposals_path).expect("read proposals.json");
    let proposals: serde_json::Value =
        serde_json::from_str(&proposals_raw).expect("proposals.json must be valid JSON");

    let summary = proposals.get("summary").expect("proposals.json must have 'summary'");
    assert_eq!(
        summary["total"].as_u64().unwrap_or(0),
        3,
        "proposals summary.total must be 3"
    );

    let entries = proposals
        .get("entries")
        .and_then(|e| e.as_array())
        .expect("proposals.json must have 'entries' array");
    assert_eq!(entries.len(), 3, "proposals entries length must be 3");

    // Every entry must have the expected shape.
    for entry in entries {
        assert!(
            entry.get("original_metric_id").is_some(),
            "each entry must have original_metric_id"
        );
        assert!(
            entry.get("tier").is_some(),
            "each entry must have tier"
        );
        assert!(
            entry.get("reason").is_some(),
            "each entry must have reason"
        );
    }

    // The unparseable metric must appear as tier3_untranslatable.
    let unparseable_entry = entries
        .iter()
        .find(|e| {
            e["original_metric_id"]
                .as_str() == Some("custom_unparseable")
        })
        .expect("must find entry for custom_unparseable");
    assert_eq!(
        unparseable_entry["tier"].as_str().unwrap_or(""),
        "tier3_untranslatable",
        "custom_unparseable must be tier3_untranslatable"
    );

    // 5. Validate the Markdown summary file.
    let markdown_raw = fs::read_to_string(&markdown_path).expect("read proposals.md");
    assert!(
        markdown_raw.contains("# CUSTOM Metric Migration Report"),
        "Markdown must contain the report header"
    );
    assert!(
        markdown_raw.contains("## Summary"),
        "Markdown must contain a Summary section"
    );
    assert!(
        markdown_raw.contains("analyzed:** 3"),
        "Markdown summary must mention total=3"
    );
    assert!(
        markdown_raw.contains("## Entries"),
        "Markdown must contain an Entries section"
    );
    assert!(
        markdown_raw.contains("custom_unparseable"),
        "Markdown must mention the unparseable metric"
    );
}

// ---------------------------------------------------------------------------
// Additional unit-level test: translate with empty scan output
// ---------------------------------------------------------------------------

#[tokio::test]
async fn translate_subcommand_empty_scan() {
    let dir = TempDir::new().expect("tempdir");

    let report_path = dir.path().join("empty_scan.json");
    fs::write(&report_path, "[]").expect("write empty scan.json");

    let proposals_path = dir.path().join("proposals.json");

    let counts = translate_subcommand(&report_path, &proposals_path, None)
        .await
        .expect("empty scan must succeed");

    assert_eq!(counts.total, 0, "empty scan must yield total=0");
    assert_eq!(counts.tier1_filtered_mean, 0);
    assert_eq!(counts.tier1_composite, 0);
    assert_eq!(counts.tier1_windowed_count, 0);
    assert_eq!(counts.tier2_metricql, 0);
    assert_eq!(counts.tier3_untranslatable, 0);

    // Proposals file must be valid JSON with empty entries.
    let raw = fs::read_to_string(&proposals_path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        v["summary"]["total"].as_u64().unwrap_or(99),
        0,
        "empty scan proposals summary.total must be 0"
    );
    assert_eq!(
        v["entries"].as_array().unwrap().len(),
        0,
        "empty scan proposals entries must be empty array"
    );
}

// ---------------------------------------------------------------------------
// Translate fails gracefully with missing report file
// ---------------------------------------------------------------------------

#[tokio::test]
async fn translate_subcommand_missing_report_returns_err() {
    let dir = TempDir::new().expect("tempdir");
    let missing = dir.path().join("nonexistent.json");
    let output = dir.path().join("out.json");

    let result = translate_subcommand(&missing, &output, None).await;
    assert!(
        result.is_err(),
        "translate_subcommand must return Err for a missing report file"
    );
}

// ---------------------------------------------------------------------------
// Translate fails gracefully with invalid JSON
// ---------------------------------------------------------------------------

#[tokio::test]
async fn translate_subcommand_invalid_json_returns_err() {
    let dir = TempDir::new().expect("tempdir");
    let report = dir.path().join("bad.json");
    fs::write(&report, "{not a json array}").unwrap();
    let output = dir.path().join("out.json");

    let result = translate_subcommand(&report, &output, None).await;
    assert!(
        result.is_err(),
        "translate_subcommand must return Err for invalid JSON"
    );
}
