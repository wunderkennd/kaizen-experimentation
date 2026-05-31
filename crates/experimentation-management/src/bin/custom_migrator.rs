//! `custom_migrator` — CLI binary for the ADR-026 Phase 3 CUSTOM-metric
//! migration workflow (#437).
//!
//! ## Subcommands
//!
//! | Subcommand  | Status        | Description                                              |
//! |-------------|---------------|----------------------------------------------------------|
//! | `scan`      | Implemented   | Fetch all CUSTOM metrics from M5; write to JSON.         |
//! | `translate` | Implemented   | Classify + translate scan output; write proposals JSON.  |
//! | `shadow`    | Stub (Phase B)| Schedule shadow computations on M3; poll for 7d results. |
//! | `apply`     | Stub (Phase C)| Apply APPROVED proposals via `M5::MigrateMetricDefinition`. |
//!
//! ## Migration workflow
//!
//! ```
//! # Step 1 — scan live M5 for all CUSTOM metrics.
//! custom_migrator scan --m5-addr http://localhost:50055 --output scan.json
//!
//! # Step 2 — classify + translate; review proposals.json and proposals.md.
//! custom_migrator translate --report scan.json --output proposals.json --markdown proposals.md
//!
//! # Step 3 — (Phase B) schedule M3 shadow computations and poll for 7 days.
//! custom_migrator shadow --proposals proposals.json --m3-addr http://localhost:50056
//!
//! # Step 4 — (Phase C) apply APPROVED proposals after operator review.
//! custom_migrator apply --proposals proposals.json --shadow-results shadow.json --confirm
//! ```

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use experimentation_management::migration::cli::translate_subcommand;
use experimentation_proto::experimentation::common::v1::MetricType;
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_client::ExperimentManagementServiceClient,
    ListMetricDefinitionsRequest,
};

// ---------------------------------------------------------------------------
// CLI shape (L2 binding)
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "custom_migrator",
    version,
    about = "ADR-026 Phase 3 CUSTOM-metric migration tool (#437)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan M5 for all CUSTOM-typed metrics and dump them to a JSON file.
    ///
    /// The output is a JSON array where each element is a serialised
    /// MetricDefinition. Pass this file to the `translate` subcommand.
    Scan {
        /// gRPC address of the M5 management service (e.g. "http://localhost:50055").
        #[arg(long)]
        m5_addr: String,

        /// Output path for the scan JSON (default: report.json).
        #[arg(long, default_value = "report.json")]
        output: PathBuf,

        /// Maximum number of metrics to fetch per page (0 = server default).
        #[arg(long, default_value_t = 0)]
        page_size: i32,
    },

    /// Classify and translate the scan output into typed migration proposals.
    ///
    /// Reads the JSON file produced by `scan`, runs `classify_and_translate`
    /// on each metric, and writes:
    ///   - A JSON proposals file (machine-readable; input for `apply`).
    ///   - An optional Markdown summary (human-readable; for operator review).
    Translate {
        /// Path to the scan output JSON produced by `custom_migrator scan`.
        #[arg(long)]
        report: PathBuf,

        /// Output path for the proposals JSON (default: proposals.json).
        #[arg(long, default_value = "proposals.json")]
        output: PathBuf,

        /// Optional output path for the Markdown summary.
        #[arg(long)]
        markdown: Option<PathBuf>,
    },

    /// Schedule M3 shadow computations and poll for 7 days of results.
    ///
    /// **NOT YET IMPLEMENTED — requires ADR-026 Phase B (M3 shadow-run RPCs).**
    Shadow {
        /// Path to the proposals JSON produced by `custom_migrator translate`.
        #[arg(long)]
        proposals: PathBuf,

        /// gRPC address of the M3 metrics service (e.g. "http://localhost:50056").
        #[arg(long)]
        m3_addr: String,

        /// How long to poll for shadow results before giving up (e.g. "7d", "168h").
        #[arg(long, default_value = "7d")]
        duration: String,
    },

    /// Apply APPROVED migration proposals to M5.
    ///
    /// **NOT YET IMPLEMENTED — requires ADR-026 Phase C (M5 MigrateMetricDefinition RPC).**
    Apply {
        /// Path to the proposals JSON produced by `custom_migrator translate`.
        #[arg(long)]
        proposals: PathBuf,

        /// Path to the shadow-results JSON produced by `custom_migrator shadow`.
        #[arg(long)]
        shadow_results: PathBuf,

        /// Print what would be applied without making any changes.
        #[arg(long, group = "mode")]
        dry_run: bool,

        /// Actually apply the APPROVED proposals. Mutually exclusive with --dry-run.
        #[arg(long, group = "mode")]
        confirm: bool,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "custom_migrator=info".into()),
        )
        .init();

    let cli = Cli::parse();

    let result = match cli.cmd {
        Cmd::Scan { m5_addr, output, page_size } => scan(&m5_addr, &output, page_size).await,
        Cmd::Translate { report, output, markdown } => {
            translate(&report, &output, markdown.as_deref()).await
        }
        Cmd::Shadow { proposals, m3_addr, duration } => {
            shadow_stub(&proposals, &m3_addr, &duration)
        }
        Cmd::Apply { proposals, shadow_results, dry_run, confirm } => {
            apply_stub(&proposals, &shadow_results, dry_run, confirm)
        }
    };

    if let Err(e) = result {
        tracing::error!(error = %e, "{:#}", e);
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// scan — fully implemented
// ---------------------------------------------------------------------------

async fn scan(m5_addr: &str, output: &Path, page_size: i32) -> Result<()> {
    info!(m5_addr, output = %output.display(), "connecting to M5 to list CUSTOM metrics");

    let mut client = ExperimentManagementServiceClient::connect(m5_addr.to_string())
        .await
        .with_context(|| format!("connecting to M5 at {m5_addr}"))?;

    // Paginate until we have all CUSTOM metrics.
    let mut all_metrics = Vec::new();
    let mut page_token = String::new();

    loop {
        let req = ListMetricDefinitionsRequest {
            type_filter: MetricType::Custom as i32,
            page_size,
            page_token: page_token.clone(),
        };

        let resp = client
            .list_metric_definitions(req)
            .await
            .context("listing CUSTOM metrics from M5")?
            .into_inner();

        let next_token = resp.next_page_token.clone();
        all_metrics.extend(resp.metrics);

        if next_token.is_empty() {
            break;
        }
        page_token = next_token;
    }

    let n = all_metrics.len();

    // Serialize each MetricDefinition via report's metric_to_json (reuse the
    // private converter by going through a one-metric render). We call
    // render_json with Tier3Untranslatable placeholders (proposal = null) so
    // that the scan output carries only the raw MetricDefinition fields —
    // exactly what the translate subcommand expects.
    //
    // Simpler: write a custom JSON array using serde_json directly on the
    // fields we know are present.
    let json_arr: Vec<serde_json::Value> = all_metrics
        .iter()
        .map(|m| {
            use experimentation_proto::experimentation::common::v1::metric_definition::TypeConfig;
            use serde_json::json;

            let mut obj = serde_json::Map::new();
            obj.insert("metric_id".to_string(), json!(m.metric_id));
            obj.insert("name".to_string(), json!(m.name));
            obj.insert("type".to_string(), json!(m.r#type));

            if !m.description.is_empty() {
                obj.insert("description".to_string(), json!(m.description));
            }
            if !m.source_event_type.is_empty() {
                obj.insert("source_event_type".to_string(), json!(m.source_event_type));
            }
            if !m.custom_sql.is_empty() {
                obj.insert("custom_sql".to_string(), json!(m.custom_sql));
            }
            if !m.metricql_expression.is_empty() {
                obj.insert("metricql_expression".to_string(), json!(m.metricql_expression));
            }

            if let Some(ref tc) = m.type_config {
                match tc {
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
                                "operator": comp.operator,
                                "operands": comp.operands.iter().map(|op| json!({
                                    "metric_id": op.metric_id,
                                    "weight": op.weight,
                                })).collect::<Vec<_>>(),
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

            serde_json::Value::Object(obj)
        })
        .collect();

    let file = File::create(output)
        .with_context(|| format!("creating output file {}", output.display()))?;
    serde_json::to_writer_pretty(file, &json_arr)
        .context("serializing CUSTOM metrics to JSON")?;

    info!(count = n, output = %output.display(), "scan complete");
    println!("Wrote {n} CUSTOM metrics to {}", output.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// translate — delegates to the lib's public subcommand function
// ---------------------------------------------------------------------------

async fn translate(
    report: &Path,
    output: &Path,
    markdown: Option<&Path>,
) -> Result<()> {
    info!(
        report = %report.display(),
        output = %output.display(),
        "translating scan output to proposals"
    );

    let counts = translate_subcommand(report, output, markdown).await?;

    info!(
        total = counts.total,
        tier1_filtered_mean = counts.tier1_filtered_mean,
        tier1_composite = counts.tier1_composite,
        tier1_windowed_count = counts.tier1_windowed_count,
        tier2_metricql = counts.tier2_metricql,
        tier3_untranslatable = counts.tier3_untranslatable,
        output = %output.display(),
        "translate complete"
    );

    println!(
        "Wrote proposals to {}  \
         (total={}, T1_FM={}, T1_COMP={}, T1_WC={}, T2_MQL={}, T3={})",
        output.display(),
        counts.total,
        counts.tier1_filtered_mean,
        counts.tier1_composite,
        counts.tier1_windowed_count,
        counts.tier2_metricql,
        counts.tier3_untranslatable,
    );

    if let Some(md) = markdown {
        println!("Markdown summary written to {}", md.display());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// shadow — stub (Phase B implements)
// ---------------------------------------------------------------------------

fn shadow_stub(proposals: &Path, m3_addr: &str, duration: &str) -> Result<()> {
    eprintln!("ERROR: shadow-run not yet implemented in this release.");
    eprintln!();
    eprintln!(
        "  proposals : {}",
        proposals.display()
    );
    eprintln!("  m3-addr   : {m3_addr}");
    eprintln!("  duration  : {duration}");
    eprintln!();
    eprintln!(
        "Requires ADR-026 Phase 3 Task B \
         (M3 ScheduleShadowComputation / GetShadowResults RPCs)."
    );
    eprintln!(
        "See docs/adrs/026-custom-metrics-layer.md §Phase-B and \
         the active plan in docs/superpowers/plans/."
    );
    std::process::exit(2);
}

// ---------------------------------------------------------------------------
// apply — stub (Phase C implements)
// ---------------------------------------------------------------------------

fn apply_stub(
    proposals: &Path,
    shadow_results: &Path,
    dry_run: bool,
    confirm: bool,
) -> Result<()> {
    eprintln!("ERROR: apply not yet implemented in this release.");
    eprintln!();
    eprintln!("  proposals      : {}", proposals.display());
    eprintln!("  shadow-results : {}", shadow_results.display());
    eprintln!("  --dry-run      : {dry_run}");
    eprintln!("  --confirm      : {confirm}");
    eprintln!();
    eprintln!(
        "Requires ADR-026 Phase 3 Task C \
         (M5 MigrateMetricDefinition RPC + APPROVED approval gate)."
    );
    eprintln!(
        "See docs/adrs/026-custom-metrics-layer.md §Phase-C and \
         the active plan in docs/superpowers/plans/."
    );
    eprintln!();
    eprintln!(
        "SAFETY NOTE: The apply subcommand is intentionally gated behind Phase C \
         to ensure M5 performs a full re-validation (MetricLookup-backed cycle \
         detection + store write) before any CUSTOM metric is irreversibly migrated. \
         Never apply proposals with ad-hoc scripts; wait for Phase C."
    );
    std::process::exit(2);
}

