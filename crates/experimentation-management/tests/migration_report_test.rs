//! Integration test for the migration report generator (ADR-026 Phase 3 #437).
//!
//! Tests:
//! - JSON report generation matches golden file
//! - Markdown summary generation matches golden file
//! - All tier outcomes are represented

use std::fs;
use std::path::PathBuf;

use experimentation_management::migration::report::{
    render_json, render_markdown, ReportEntry,
};
use experimentation_management::migration::ClassificationResult;
use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig, CompositeConfig, CompositeOperand, CompositeOperator,
    FilteredMeanConfig, MetricDefinition, MetricType, WindowedCountConfig,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn testdata_dir() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir).join("tests/testdata")
}

fn read_golden(name: &str) -> String {
    let path = testdata_dir().join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("read golden file at {}: {e}", path.display());
    })
}

fn write_golden(name: &str, content: &str) {
    let path = testdata_dir().join(name);
    fs::write(&path, content)
        .unwrap_or_else(|e| panic!("write golden file at {}: {e}", path.display()));
}

// ---------------------------------------------------------------------------
// Fixture creation
// ---------------------------------------------------------------------------

fn fixture_filtered_mean_metric() -> MetricDefinition {
    MetricDefinition {
        metric_id: "mobile_watch_time".to_string(),
        name: "Mobile Watch Time (migrated)".to_string(),
        r#type: MetricType::FilteredMean as i32,
        source_event_type: "video_play".to_string(),
        type_config: Some(TypeConfig::FilteredMean(FilteredMeanConfig {
            filter_sql: "platform = 'mobile'".to_string(),
            value_column: "duration_ms".to_string(),
        })),
        ..Default::default()
    }
}

fn fixture_composite_metric() -> MetricDefinition {
    MetricDefinition {
        metric_id: "engagement_lift".to_string(),
        name: "Engagement Lift (migrated)".to_string(),
        r#type: MetricType::Composite as i32,
        type_config: Some(TypeConfig::Composite(CompositeConfig {
            operands: vec![
                CompositeOperand {
                    metric_id: "sessions".to_string(),
                    weight: 0.0,
                },
                CompositeOperand {
                    metric_id: "watch_time".to_string(),
                    weight: 0.0,
                },
            ],
            operator: CompositeOperator::Add as i32,
        })),
        ..Default::default()
    }
}

fn fixture_windowed_count_metric() -> MetricDefinition {
    MetricDefinition {
        metric_id: "rebuffers_7d".to_string(),
        name: "Rebuffers in 7 Days (migrated)".to_string(),
        r#type: MetricType::WindowedCount as i32,
        type_config: Some(TypeConfig::WindowedCount(WindowedCountConfig {
            event_type: "rebuffer_event".to_string(),
            filter_sql: "".to_string(),
            window_hours: 168,
        })),
        ..Default::default()
    }
}

fn fixture_metricql_metric() -> MetricDefinition {
    MetricDefinition {
        metric_id: "revenue_per_user".to_string(),
        name: "Revenue Per User (migrated)".to_string(),
        r#type: MetricType::Metricql as i32,
        metricql_expression: "@total_revenue / @user_count".to_string(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Golden-file test
// ---------------------------------------------------------------------------

#[test]
fn report_golden_file_json() {
    let entries = vec![
        ReportEntry {
            original_metric_id: "mobile_watch_time".to_string(),
            result: ClassificationResult::Tier1Filtered {
                proposal: fixture_filtered_mean_metric(),
                reason: "matches FILTERED_MEAN shape with column-allowlist filter".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "engagement_lift".to_string(),
            result: ClassificationResult::Tier1Composite {
                proposal: fixture_composite_metric(),
                reason: "operand references resolved, no cycle detected".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "rebuffers_7d".to_string(),
            result: ClassificationResult::Tier1WindowedCount {
                proposal: fixture_windowed_count_metric(),
                reason: "matches WINDOWED_COUNT shape with valid window_hours=168".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "revenue_per_user".to_string(),
            result: ClassificationResult::Tier2Metricql {
                proposal: fixture_metricql_metric(),
                reason: "translatable to MetricQL with two metric refs".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "custom_metric".to_string(),
            result: ClassificationResult::Tier3Untranslatable {
                reason: "matches no known translator shape; manual review required".to_string(),
                parse_error: None,
            },
        },
    ];

    let json_output = render_json(&entries).expect("render_json should succeed");

    // For the first run, create the golden file.
    // Subsequent runs compare against it.
    let golden_path = testdata_dir().join("report.golden.json");
    if !golden_path.exists() {
        eprintln!(
            "Creating golden file at {}. Inspect carefully before committing.",
            golden_path.display()
        );
        write_golden("report.golden.json", &json_output);
    }

    let golden = read_golden("report.golden.json");

    // Parse both as JSON to allow for field-order differences
    let actual_json: serde_json::Value = serde_json::from_str(&json_output)
        .expect("actual output must be valid JSON");
    let expected_json: serde_json::Value = serde_json::from_str(&golden)
        .expect("golden file must be valid JSON");

    assert_eq!(
        actual_json, expected_json,
        "JSON report does not match golden file"
    );
}

#[test]
fn report_golden_file_markdown() {
    let entries = vec![
        ReportEntry {
            original_metric_id: "mobile_watch_time".to_string(),
            result: ClassificationResult::Tier1Filtered {
                proposal: fixture_filtered_mean_metric(),
                reason: "matches FILTERED_MEAN shape with column-allowlist filter".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "engagement_lift".to_string(),
            result: ClassificationResult::Tier1Composite {
                proposal: fixture_composite_metric(),
                reason: "operand references resolved, no cycle detected".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "rebuffers_7d".to_string(),
            result: ClassificationResult::Tier1WindowedCount {
                proposal: fixture_windowed_count_metric(),
                reason: "matches WINDOWED_COUNT shape with valid window_hours=168".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "revenue_per_user".to_string(),
            result: ClassificationResult::Tier2Metricql {
                proposal: fixture_metricql_metric(),
                reason: "translatable to MetricQL with two metric refs".to_string(),
            },
        },
        ReportEntry {
            original_metric_id: "custom_metric".to_string(),
            result: ClassificationResult::Tier3Untranslatable {
                reason: "matches no known translator shape; manual review required".to_string(),
                parse_error: None,
            },
        },
    ];

    let md_output = render_markdown(&entries);

    // For the first run, create the golden file.
    // Subsequent runs compare against it.
    let golden_path = testdata_dir().join("report.golden.md");
    if !golden_path.exists() {
        eprintln!(
            "Creating golden file at {}. Inspect carefully before committing.",
            golden_path.display()
        );
        write_golden("report.golden.md", &md_output);
    }

    let golden = read_golden("report.golden.md");

    assert_eq!(
        md_output, golden,
        "Markdown report does not match golden file"
    );
}
