//! `custom_migrator` — CLI binary for the ADR-026 Phase 3 CUSTOM-metric
//! migration workflow (#437).
//!
//! ## Subcommands
//!
//! | Subcommand  | Status        | Description                                              |
//! |-------------|---------------|----------------------------------------------------------|
//! | `scan`      | Implemented   | Fetch all CUSTOM metrics from M5; write to JSON.         |
//! | `translate` | Implemented   | Classify + translate scan output; write proposals JSON.  |
//! | `shadow`    | Implemented   | Schedule shadow computations on M3; poll until APPROVED. |
//! | `apply`     | Implemented   | Apply APPROVED proposals via `M5::MigrateMetricDefinition`. |
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
//! # Step 3 — schedule M3 shadow computations and poll until APPROVED / budget exhausts.
//! custom_migrator shadow --proposals proposals.json --m3-addr http://localhost:50056 \
//!     --duration 7d --output shadow.json
//!
//! # Step 4a — preview the migrations without mutating M5.
//! custom_migrator apply --shadow-results shadow.json --m5-addr http://localhost:50055 --dry-run
//!
//! # Step 4b — apply the APPROVED proposals after operator review.
//! custom_migrator apply --shadow-results shadow.json --m5-addr http://localhost:50055 \
//!     --operator alice@example.com --confirm
//! ```

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use experimentation_management::migration::cli::{json_to_metric_definition, translate_subcommand};
use experimentation_proto::experimentation::common::v1::{MetricDefinition, MetricType};
use experimentation_proto::experimentation::management::v1::{
    experiment_management_service_client::ExperimentManagementServiceClient,
    ListMetricDefinitionsRequest, MigrateMetricDefinitionRequest,
};
use experimentation_proto::experimentation::metrics::v1::{
    metric_computation_service_client::MetricComputationServiceClient,
    GetShadowResultsRequest, PromoteShadowResultRequest, ScheduleShadowComputationRequest,
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

    /// Schedule M3 shadow computations and poll until APPROVED / REJECTED.
    ///
    /// Reads the proposals JSON produced by `translate`, filters to Tier 1
    /// (FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT) and Tier 2 (METRICQL)
    /// entries, and for each one:
    ///
    ///   1. Calls M3 `ScheduleShadowComputation` with the candidate metric.
    ///   2. Polls `PromoteShadowResult` on a cadence (default 60s, overridable
    ///      via `CUSTOM_MIGRATOR_POLL_INTERVAL_SECS`) until the run reaches a
    ///      terminal state (APPROVED / REJECTED) or `--duration` exhausts.
    ///   3. Snapshots `days_within_tolerance` / `total_days` from
    ///      `GetShadowResults` for the audit trail.
    ///
    /// The 7-consecutive-days-within-tolerance gate is enforced by M3 inside
    /// `PromoteShadowResult` (see PR #580); the migrator does not double-check.
    ///
    /// Tier 3 entries are non-translatable and are not shadow-run.
    ///
    /// Writes the per-proposal outcomes as `ShadowOutput` JSON to `--output`,
    /// which is the direct input to the `apply` subcommand.
    Shadow {
        /// Path to the proposals JSON produced by `custom_migrator translate`.
        #[arg(long)]
        proposals: PathBuf,

        /// gRPC address of the M3 metrics service (e.g. "http://localhost:50056").
        #[arg(long)]
        m3_addr: String,

        /// How long to poll for shadow results before giving up (e.g. "7d", "168h",
        /// "30m", "60s"). Cannot be zero.
        #[arg(long, default_value = "7d")]
        duration: String,

        /// Output path for the shadow-outcome JSON consumed by `apply`
        /// (default: shadow.json).
        #[arg(long, default_value = "shadow.json")]
        output: PathBuf,
    },

    /// Apply APPROVED migration proposals to M5.
    ///
    /// Reads the `ShadowOutput` JSON produced by `custom_migrator shadow`,
    /// filters to outcomes with `status == APPROVED` and a non-empty
    /// `result_id`, and for each surviving outcome calls M5's
    /// `MigrateMetricDefinition` RPC (ADR-026 Phase 3 — Lock L7 two-step apply).
    ///
    /// Operator workflow:
    ///
    ///   1. `--dry-run` first — prints the planned migrations as a table
    ///      and exits 0 without mutating M5.
    ///   2. `--confirm --operator <name>` — sequentially applies each
    ///      APPROVED outcome via `MigrateMetricDefinition`. Per-outcome
    ///      failures DO NOT abort the run (apply-as-much-as-possible)
    ///      but DO drive the final exit code to 1.
    ///
    /// `--dry-run` and `--confirm` are mutually exclusive (one must be set).
    ///
    /// Exit codes: `0` (all OK / nothing to apply), `1` (at least one
    /// migration failed), `2` (fatal — couldn't read input or connect to M5).
    Apply {
        /// Path to the proposals JSON produced by `custom_migrator translate`.
        /// Kept for cross-reference / audit; the binding input is `--shadow-results`.
        #[arg(long)]
        proposals: Option<PathBuf>,

        /// Path to the `ShadowOutput` JSON produced by `custom_migrator shadow`.
        /// APPROVED outcomes here drive the migrations applied to M5.
        #[arg(long)]
        shadow_results: PathBuf,

        /// gRPC address of the M5 management service (e.g. "http://localhost:50055").
        #[arg(long)]
        m5_addr: String,

        /// Operator identifier written to M5's `metric_migrations` audit row
        /// (e.g. user email or service id). REQUIRED when `--confirm` is set;
        /// optional for `--dry-run`.
        #[arg(long)]
        operator: Option<String>,

        /// Print the planned migrations as a table and exit without mutating M5.
        /// Mutually exclusive with --confirm.
        #[arg(long, group = "mode")]
        dry_run: bool,

        /// Actually apply the APPROVED migrations. Requires --operator.
        /// Mutually exclusive with --dry-run.
        #[arg(long, group = "mode")]
        confirm: bool,

        /// Optional path for a sidecar audit JSON (`ApplyOutput`) capturing
        /// per-outcome results. Defaults to `<shadow_results>.apply.json`
        /// in `--confirm` mode; suppressed in `--dry-run` mode.
        #[arg(long)]
        output: Option<PathBuf>,
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

    let exit_code = match cli.cmd {
        Cmd::Scan { m5_addr, output, page_size } => {
            run_or_log(scan(&m5_addr, &output, page_size).await)
        }
        Cmd::Translate { report, output, markdown } => {
            run_or_log(translate(&report, &output, markdown.as_deref()).await)
        }
        Cmd::Shadow { proposals, m3_addr, duration, output } => {
            // The shadow subcommand owns its own exit-code semantics
            // (0 = any APPROVED, 1 = none APPROVED, 2 = fatal). See
            // `shadow_subcommand` for the contract.
            match shadow_subcommand(&proposals, &m3_addr, &duration, &output).await {
                Ok(code) => code,
                Err(e) => {
                    tracing::error!(error = %e, "{:#}", e);
                    2
                }
            }
        }
        Cmd::Apply {
            proposals,
            shadow_results,
            m5_addr,
            operator,
            dry_run,
            confirm,
            output,
        } => {
            // The apply subcommand owns its exit-code semantics (0 OK / 1
            // per-outcome failures / 2 fatal). See `apply_subcommand` for
            // the contract.
            match apply_subcommand(
                proposals.as_deref(),
                &shadow_results,
                &m5_addr,
                operator.as_deref(),
                dry_run,
                confirm,
                output.as_deref(),
            )
            .await
            {
                Ok(code) => code,
                Err(e) => {
                    tracing::error!(error = %e, "{:#}", e);
                    2
                }
            }
        }
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

/// Map a `Result<()>` to a process exit code, logging on failure.
fn run_or_log(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!(error = %e, "{:#}", e);
            1
        }
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
// shadow — orchestrates M3 ScheduleShadowComputation / PromoteShadowResult
// ---------------------------------------------------------------------------
//
// Locked-plan binding: L3 (Equivalence verification). The 7-consecutive-days
// gate is enforced by M3 inside PromoteShadowResult (PR #580) — the migrator's
// only job is to drive the schedule → poll → promote workflow and snapshot the
// outcomes into the JSON that the apply subcommand (T3) consumes.

/// Default polling interval between successive PromoteShadowResult calls.
///
/// Overridable via the `CUSTOM_MIGRATOR_POLL_INTERVAL_SECS` env var so
/// integration tests can run with a sub-second cadence. The variable parses
/// as a non-zero u64 number of seconds; invalid values fall back to the default
/// with a warning.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Environment variable that overrides `DEFAULT_POLL_INTERVAL`. Tests set this
/// to a small value (e.g. "1") so the polling loop ticks quickly.
const POLL_INTERVAL_ENV: &str = "CUSTOM_MIGRATOR_POLL_INTERVAL_SECS";

/// Migrator-side status that means `ScheduleShadowComputation` itself failed
/// (network / NotFound / etc.). Distinct from M3-side statuses (PENDING /
/// RUNNING / APPROVED / REJECTED / FAILED) which are reported verbatim.
const STATUS_SCHEDULING_FAILED: &str = "SCHEDULING_FAILED";

// ---------------------------------------------------------------------------
// ShadowOutput JSON shape — T3's apply subcommand consumes this directly
// ---------------------------------------------------------------------------

/// One outcome record per Tier-1/Tier-2 proposal that was shadow-run.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ShadowOutcome {
    /// ID of the original CUSTOM metric being migrated.
    pub original_metric_id: String,
    /// `shadow_id` assigned by M3 (UUID). Empty when scheduling itself failed.
    pub shadow_id: String,
    /// `result_id` from `PromoteShadowResult` — non-empty only when status is
    /// `APPROVED`. T3's apply subcommand feeds this into
    /// `MigrateMetricDefinitionRequest.shadow_run_result_id`.
    pub result_id: String,
    /// Terminal status. One of: `APPROVED`, `REJECTED`, `PENDING`, `FAILED`
    /// (M3-side), `SCHEDULING_FAILED` (migrator-side).
    pub status: String,
    /// Human-readable explanation. From `PromoteShadowResult.reason` for
    /// M3-side statuses, or the migrator's own error message for
    /// `SCHEDULING_FAILED` / budget-exhaust.
    pub reason: String,
    /// Snapshot of M3's `days_within_tolerance` at the time of the final
    /// poll. Diagnostic only; the L3 gate is enforced by M3.
    pub days_within_tolerance: i32,
    /// Snapshot of M3's `total_days` at the time of the final poll.
    pub total_days: i32,
    /// The candidate `MetricDefinition` being shadowed, encoded via
    /// `migration::report::metric_to_json` for round-trip with
    /// `migration::cli::json_to_metric_definition`. Echoed here so the
    /// `apply` subcommand does not have to re-load `proposals.json`.
    pub candidate_metric: serde_json::Value,
}

/// Top-level shape of the shadow-output JSON file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ShadowOutput {
    /// RFC 3339 timestamp captured before the first scheduling call.
    pub schedule_started_at: String,
    /// RFC 3339 timestamp captured after writing the last outcome.
    pub schedule_completed_at: String,
    /// One entry per Tier-1/Tier-2 proposal. Tier-3 proposals from
    /// `proposals.json` are skipped (logged at WARN) and absent here.
    pub outcomes: Vec<ShadowOutcome>,
}

// ---------------------------------------------------------------------------
// Shadow subcommand entry point
// ---------------------------------------------------------------------------

/// Run the shadow workflow against M3. Returns the process exit code:
///
///   * `0` — at least one outcome is APPROVED.
///   * `1` — no APPROVED outcomes (all REJECTED / PENDING / FAILED /
///     SCHEDULING_FAILED, or no Tier-1/Tier-2 proposals to run).
///   * `2` — fatal error before writing the output (couldn't read proposals,
///     couldn't connect to M3, etc.). Surface via `Err` so `main()` can log.
async fn shadow_subcommand(
    proposals_path: &Path,
    m3_addr: &str,
    duration_str: &str,
    output_path: &Path,
) -> Result<i32> {
    info!(
        proposals = %proposals_path.display(),
        m3_addr,
        duration = duration_str,
        output = %output_path.display(),
        "starting shadow-run workflow"
    );

    let budget = parse_duration(duration_str)
        .with_context(|| format!("parsing --duration '{duration_str}'"))?;
    let poll_interval = poll_interval_from_env();

    // 1. Read proposals.json (produced by translate_subcommand).
    let raw = std::fs::read_to_string(proposals_path)
        .with_context(|| format!("reading proposals JSON from {}", proposals_path.display()))?;
    let proposals_json: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("parsing proposals JSON from {}", proposals_path.display()))?;

    // 2. Filter to Tier 1 / Tier 2 (Tier 3 logged + skipped).
    let candidates = extract_shadow_candidates(&proposals_json)?;
    let total_candidates = candidates.len();
    info!(total = total_candidates, "candidates to shadow-run");

    if total_candidates == 0 {
        warn!("no Tier-1/Tier-2 proposals found; writing empty ShadowOutput");
    }

    // 3. Connect to M3.
    let mut client = MetricComputationServiceClient::connect(m3_addr.to_string())
        .await
        .with_context(|| format!("connecting to M3 at {m3_addr}"))?;

    let schedule_started_at = rfc3339_now();
    let mut outcomes = Vec::with_capacity(total_candidates);

    // 4. Sequentially shadow each candidate. Operator-driven cadence is fine.
    for cand in candidates {
        let outcome = run_one_shadow(&mut client, cand, budget, poll_interval).await;
        outcomes.push(outcome);
    }

    let schedule_completed_at = rfc3339_now();

    // 5. Write the shadow output.
    let out = ShadowOutput {
        schedule_started_at,
        schedule_completed_at,
        outcomes,
    };
    let json = serde_json::to_string_pretty(&out)
        .context("serializing ShadowOutput to JSON")?;
    std::fs::write(output_path, json)
        .with_context(|| format!("writing ShadowOutput to {}", output_path.display()))?;

    // 6. Log a one-line summary per outcome and compute exit code.
    let approved = log_summary(&out.outcomes);
    info!(
        total = out.outcomes.len(),
        approved,
        output = %output_path.display(),
        "shadow-run complete"
    );

    // L3 contract: 0 if any APPROVED, 1 otherwise.
    Ok(if approved > 0 { 0 } else { 1 })
}

// ---------------------------------------------------------------------------
// Candidate extraction (proposals.json → Vec<ShadowCandidate>)
// ---------------------------------------------------------------------------

/// One Tier-1/Tier-2 proposal lined up for shadow-run.
struct ShadowCandidate {
    original_metric_id: String,
    tier: String,
    /// The proto MetricDefinition reconstructed from `proposals.json`.
    candidate_metric: MetricDefinition,
    /// The raw JSON encoding of the candidate (echoed into `ShadowOutcome`
    /// without an extra serialization round-trip).
    candidate_json: serde_json::Value,
}

/// Pull every Tier-1/Tier-2 entry out of `proposals.json` and reconstruct the
/// candidate `MetricDefinition`. Tier-3 entries are logged at WARN and dropped.
///
/// `proposals.json` shape: `{"summary": {...}, "entries": [{ "original_metric_id",
/// "tier", "reason", "proposal", "parse_error" }]}` (see
/// `migration::report::render_json`).
fn extract_shadow_candidates(value: &serde_json::Value) -> Result<Vec<ShadowCandidate>> {
    let entries = value
        .get("entries")
        .and_then(|v| v.as_array())
        .context("proposals JSON missing required `entries` array")?;

    let mut candidates = Vec::new();
    for entry in entries {
        let original_metric_id = entry
            .get("original_metric_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tier = entry
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !is_shadow_eligible_tier(&tier) {
            warn!(
                original_metric_id = %original_metric_id,
                tier = %tier,
                "skipping non-Tier-1/Tier-2 proposal (Tier 3 stays CUSTOM)"
            );
            continue;
        }

        let proposal = entry
            .get("proposal")
            .filter(|v| !v.is_null())
            .with_context(|| {
                format!(
                    "proposal field missing for {} (tier {})",
                    original_metric_id, tier
                )
            })?;

        let candidate_metric = json_to_metric_definition(proposal).with_context(|| {
            format!("decoding candidate MetricDefinition for {}", original_metric_id)
        })?;

        candidates.push(ShadowCandidate {
            original_metric_id,
            tier,
            candidate_metric,
            candidate_json: proposal.clone(),
        });
    }
    Ok(candidates)
}

/// Tier name from `proposals.json`. Tier 1 = FILTERED_MEAN / COMPOSITE /
/// WINDOWED_COUNT, Tier 2 = METRICQL. Tier 3 = untranslatable.
fn is_shadow_eligible_tier(tier: &str) -> bool {
    matches!(
        tier,
        "tier1_filtered_mean"
            | "tier1_composite"
            | "tier1_windowed_count"
            | "tier2_metricql"
    )
}

// ---------------------------------------------------------------------------
// Single-proposal shadow loop
// ---------------------------------------------------------------------------

/// Drive `ScheduleShadowComputation` → poll `PromoteShadowResult` → snapshot
/// `GetShadowResults` for one candidate. Bounded by `budget` (wall-clock).
async fn run_one_shadow(
    client: &mut MetricComputationServiceClient<tonic::transport::Channel>,
    cand: ShadowCandidate,
    budget: Duration,
    poll_interval: Duration,
) -> ShadowOutcome {
    let ShadowCandidate {
        original_metric_id,
        tier,
        candidate_metric,
        candidate_json,
    } = cand;
    info!(
        original_metric_id = %original_metric_id,
        tier = %tier,
        "scheduling shadow computation"
    );

    // (a) ScheduleShadowComputation — capture shadow_id or fail this outcome.
    let shadow_id = match client
        .schedule_shadow_computation(ScheduleShadowComputationRequest {
            original_metric_id: original_metric_id.clone(),
            candidate_metric: Some(candidate_metric),
        })
        .await
    {
        Ok(resp) => resp.into_inner().shadow_id,
        Err(status) => {
            warn!(
                original_metric_id = %original_metric_id,
                code = ?status.code(),
                message = %status.message(),
                "ScheduleShadowComputation failed"
            );
            return ShadowOutcome {
                original_metric_id,
                shadow_id: String::new(),
                result_id: String::new(),
                status: STATUS_SCHEDULING_FAILED.to_string(),
                reason: format!(
                    "ScheduleShadowComputation failed ({:?}): {}",
                    status.code(),
                    status.message()
                ),
                days_within_tolerance: 0,
                total_days: 0,
                candidate_metric: candidate_json,
            };
        }
    };

    info!(
        original_metric_id = %original_metric_id,
        shadow_id = %shadow_id,
        "shadow scheduled; polling for terminal status"
    );

    // (b) Poll PromoteShadowResult until terminal or budget exhausted.
    let started = Instant::now();
    let (status, result_id, reason) = poll_until_terminal(
        client,
        &shadow_id,
        budget,
        poll_interval,
        started,
    )
    .await;

    // (c) Snapshot diagnostic counters from GetShadowResults at the end.
    let (days_within_tolerance, total_days) =
        snapshot_progress(client, &shadow_id).await;

    info!(
        original_metric_id = %original_metric_id,
        shadow_id = %shadow_id,
        status = %status,
        days_within_tolerance,
        total_days,
        "shadow run finished"
    );

    ShadowOutcome {
        original_metric_id,
        shadow_id,
        result_id,
        status,
        reason,
        days_within_tolerance,
        total_days,
        candidate_metric: candidate_json,
    }
}

/// Call `PromoteShadowResult` on a cadence until M3 returns APPROVED /
/// REJECTED or the wall-clock budget exhausts. Returns `(status, result_id,
/// reason)`.
///
/// `PENDING` means "M3 has not yet collected 7 days of within-tolerance data";
/// we just wait. `result_id` is empty unless `status == APPROVED`.
async fn poll_until_terminal(
    client: &mut MetricComputationServiceClient<tonic::transport::Channel>,
    shadow_id: &str,
    budget: Duration,
    poll_interval: Duration,
    started: Instant,
) -> (String, String, String) {
    loop {
        match client
            .promote_shadow_result(PromoteShadowResultRequest {
                shadow_id: shadow_id.to_string(),
            })
            .await
        {
            Ok(resp) => {
                let inner = resp.into_inner();
                match inner.status.as_str() {
                    "APPROVED" => {
                        return (inner.status, inner.result_id, inner.reason);
                    }
                    "REJECTED" => {
                        return (inner.status, String::new(), inner.reason);
                    }
                    "PENDING" => {
                        // Fall through to sleep.
                    }
                    other => {
                        // Unexpected M3-side status (FAILED, RUNNING leaking,
                        // etc.) — surface verbatim and stop polling.
                        return (
                            other.to_string(),
                            String::new(),
                            inner.reason,
                        );
                    }
                }
            }
            Err(status) => {
                warn!(
                    shadow_id,
                    code = ?status.code(),
                    message = %status.message(),
                    "PromoteShadowResult returned an error; will retry until budget exhausts"
                );
            }
        }

        if started.elapsed() >= budget {
            return (
                "PENDING".to_string(),
                String::new(),
                format!(
                    "budget exhausted before APPROVED/REJECTED ({}s of {}s elapsed)",
                    started.elapsed().as_secs(),
                    budget.as_secs(),
                ),
            );
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Snapshot `days_within_tolerance` / `total_days` from `GetShadowResults`
/// for the audit trail. Non-fatal — returns zeros on error.
async fn snapshot_progress(
    client: &mut MetricComputationServiceClient<tonic::transport::Channel>,
    shadow_id: &str,
) -> (i32, i32) {
    match client
        .get_shadow_results(GetShadowResultsRequest {
            shadow_id: shadow_id.to_string(),
        })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            (inner.days_within_tolerance, inner.total_days)
        }
        Err(status) => {
            warn!(
                shadow_id,
                code = ?status.code(),
                message = %status.message(),
                "GetShadowResults snapshot failed; using zeros"
            );
            (0, 0)
        }
    }
}

/// Log a one-line summary per outcome and return the count of APPROVED.
fn log_summary(outcomes: &[ShadowOutcome]) -> usize {
    let mut approved = 0usize;
    for o in outcomes {
        info!(
            original_metric_id = %o.original_metric_id,
            status = %o.status,
            days_within_tolerance = o.days_within_tolerance,
            total_days = o.total_days,
            reason = %o.reason,
            "outcome"
        );
        if o.status == "APPROVED" {
            approved += 1;
        }
    }
    approved
}

// ---------------------------------------------------------------------------
// Helpers: duration parsing, env override, RFC3339 timestamps
// ---------------------------------------------------------------------------

/// Parse a duration like "7d", "168h", "30m", or "60s" into a `Duration`.
/// Returns an error for empty input, zero-magnitude values, or unrecognized
/// suffixes. Intentionally narrow: this is an operator-facing CLI flag with
/// a documented set of units, not a general-purpose parser.
fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        anyhow::bail!("empty duration");
    }
    let (num, unit) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit())
            .with_context(|| format!("duration '{s}' has no unit suffix"))?,
    );
    let n: u64 = num
        .parse()
        .with_context(|| format!("invalid duration magnitude '{num}'"))?;
    if n == 0 {
        anyhow::bail!("duration must be > 0");
    }
    let secs = match unit {
        "s" => n,
        "m" => n.checked_mul(60).context("duration overflow")?,
        "h" => n.checked_mul(3_600).context("duration overflow")?,
        "d" => n.checked_mul(86_400).context("duration overflow")?,
        other => anyhow::bail!(
            "unrecognized duration unit '{other}' (expected one of s, m, h, d)"
        ),
    };
    Ok(Duration::from_secs(secs))
}

/// Read the poll interval from `CUSTOM_MIGRATOR_POLL_INTERVAL_SECS`, falling
/// back to `DEFAULT_POLL_INTERVAL`. Invalid / zero values are warned and
/// replaced with the default.
fn poll_interval_from_env() -> Duration {
    match std::env::var(POLL_INTERVAL_ENV) {
        Ok(s) => match s.parse::<u64>() {
            Ok(n) if n > 0 => Duration::from_secs(n),
            _ => {
                warn!(
                    env = POLL_INTERVAL_ENV,
                    value = %s,
                    "invalid value; using default poll interval"
                );
                DEFAULT_POLL_INTERVAL
            }
        },
        Err(_) => DEFAULT_POLL_INTERVAL,
    }
}

/// Current wall-clock time, formatted as RFC 3339. Used purely for audit
/// metadata in `ShadowOutput`. `chrono` is already a workspace dep.
fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ---------------------------------------------------------------------------
// apply — Lock L7 two-step apply (ADR-026 Phase 3 / #437)
// ---------------------------------------------------------------------------
//
// Reads the ShadowOutput JSON produced by `shadow`, filters to APPROVED
// outcomes, and calls M5's MigrateMetricDefinition for each one (added in
// commit c259513). The handler re-validates the APPROVED state with M3
// internally; we never bypass that gate.
//
// `--dry-run` prints a tabular plan without touching M5; `--confirm`
// (paired with `--operator`) actually mutates and exits non-zero on any
// per-outcome failure.

/// One outcome record per APPROVED shadow result that the apply subcommand
/// attempted to migrate. Written to the sidecar `ApplyOutput` JSON for the
/// audit trail referenced by the operator runbook.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ApplyOutcome {
    /// ID of the original CUSTOM metric being migrated.
    pub original_metric_id: String,
    /// ID of the replacement metric (echoed from
    /// `MigrateMetricDefinitionResponse.new_metric_id`). Empty on failure.
    pub new_metric_id: String,
    /// `shadow_run_result_id` fed into MigrateMetricDefinition. Copied from
    /// the input `ShadowOutcome.result_id` regardless of success.
    pub shadow_run_result_id: String,
    /// `metric_migrations.migration_id` returned by M5 on success. Empty on
    /// failure.
    pub migration_id: String,
    /// RFC 3339 server-side timestamp from `MigrateMetricDefinitionResponse.
    /// applied_at`. Empty on failure.
    pub applied_at: String,
    /// `OK` on success; otherwise the canonical tonic Code as a string
    /// (`INVALID_ARGUMENT`, `FAILED_PRECONDITION`, `UNAVAILABLE`, ...).
    pub status: String,
    /// On success: empty. On failure: the verbatim Status message from M5.
    pub error: String,
}

/// Top-level audit-JSON written next to the input shadow results in
/// `--confirm` mode. Captures the operator, when the apply ran, and one row
/// per APPROVED outcome that was attempted.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ApplyOutput {
    /// RFC 3339 timestamp captured before the first MigrateMetricDefinition call.
    pub apply_started_at: String,
    /// RFC 3339 timestamp captured after the last outcome was processed.
    pub apply_completed_at: String,
    /// Operator identifier (from `--operator`) recorded for the audit trail.
    pub operator: String,
    /// Path to the input `--shadow-results` file (for cross-reference).
    pub shadow_results_path: String,
    /// One entry per APPROVED outcome that was attempted.
    pub outcomes: Vec<ApplyOutcome>,
    /// Convenience aggregates so a downstream consumer doesn't have to scan
    /// the `outcomes` array. `total_attempted == succeeded + failed`.
    pub total_attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// Run the apply workflow. Returns the process exit code:
///
///   * `0` — every APPROVED outcome was applied successfully (or there were
///     no APPROVED outcomes to apply; this is treated as a no-op, not an
///     error, since operators may run apply prophylactically).
///   * `1` — at least one per-outcome MigrateMetricDefinition call failed.
///     Successful migrations are still committed; the audit JSON captures
///     which ones succeeded and which didn't.
///   * `2` — fatal (couldn't read input JSON, couldn't connect to M5,
///     --confirm without --operator). Surface via `Err` so `main()` logs.
async fn apply_subcommand(
    proposals_path: Option<&Path>,
    shadow_results_path: &Path,
    m5_addr: &str,
    operator: Option<&str>,
    dry_run: bool,
    confirm: bool,
    output_path: Option<&Path>,
) -> Result<i32> {
    info!(
        proposals = ?proposals_path.map(|p| p.display().to_string()),
        shadow_results = %shadow_results_path.display(),
        m5_addr,
        dry_run,
        confirm,
        "starting apply workflow"
    );

    // (0) clap's group="mode" already enforces mutual exclusion; require one.
    if !dry_run && !confirm {
        anyhow::bail!(
            "one of --dry-run or --confirm is required \
             (--dry-run previews, --confirm mutates)"
        );
    }

    // (0b) --confirm requires --operator. Without an operator the audit row
    // would be ambiguous; refuse before opening any RPC channel.
    if confirm && operator.map(str::trim).unwrap_or("").is_empty() {
        anyhow::bail!(
            "--operator is required when --confirm is set \
             (the operator identifier is written to M5's metric_migrations \
             audit row; refusing to apply with an empty operator)"
        );
    }

    // (1) Read the ShadowOutput JSON.
    let raw = std::fs::read_to_string(shadow_results_path).with_context(|| {
        format!(
            "reading shadow results JSON from {}",
            shadow_results_path.display()
        )
    })?;
    let shadow: ShadowOutput = serde_json::from_str(&raw).with_context(|| {
        format!(
            "parsing shadow results JSON from {}",
            shadow_results_path.display()
        )
    })?;
    info!(
        total_outcomes = shadow.outcomes.len(),
        "loaded ShadowOutput"
    );

    // (2) Filter to APPROVED outcomes with a non-empty result_id, logging
    // each skip by its status so the operator can see why everything that
    // didn't run was excluded.
    let (approved, skipped) = partition_outcomes(&shadow.outcomes);
    log_skipped_outcomes(&skipped);

    // (3) Short-circuit: nothing to apply is not an error.
    if approved.is_empty() {
        warn!(
            "no APPROVED outcomes to apply (skipped={}); exiting 0",
            skipped.len()
        );
        println!(
            "No APPROVED outcomes to apply in {} (skipped {} non-APPROVED).",
            shadow_results_path.display(),
            skipped.len()
        );
        return Ok(0);
    }

    // (4) --dry-run: print plan and exit. Never connect to M5.
    if dry_run {
        print_dry_run_plan(&approved);
        return Ok(0);
    }

    // (5) --confirm: connect to M5 and apply each APPROVED outcome.
    let operator_str = operator
        .expect("operator required for --confirm; checked above")
        .trim()
        .to_string();

    let mut client = ExperimentManagementServiceClient::connect(m5_addr.to_string())
        .await
        .with_context(|| format!("connecting to M5 at {m5_addr}"))?;

    let apply_started_at = rfc3339_now();
    let mut outcomes: Vec<ApplyOutcome> = Vec::with_capacity(approved.len());
    let mut succeeded: usize = 0;
    let mut failed: usize = 0;

    // Apply-as-much-as-possible: per-outcome failures don't abort the run
    // (operators want partial progress to land), but DO drive exit code 1
    // at the end so CI / wrappers can detect incomplete applies.
    for cand in &approved {
        let outcome = apply_one(&mut client, cand, &operator_str).await;
        if outcome.status == "OK" {
            succeeded += 1;
            info!(
                original_metric_id = %outcome.original_metric_id,
                new_metric_id = %outcome.new_metric_id,
                migration_id = %outcome.migration_id,
                applied_at = %outcome.applied_at,
                "migration applied"
            );
        } else {
            failed += 1;
            warn!(
                original_metric_id = %outcome.original_metric_id,
                shadow_run_result_id = %outcome.shadow_run_result_id,
                status = %outcome.status,
                error = %outcome.error,
                "migration failed"
            );
        }
        outcomes.push(outcome);
    }

    let apply_completed_at = rfc3339_now();

    // (6) Write the sidecar audit JSON. Default to <shadow>.apply.json so
    // operators don't have to remember to pass --output.
    let audit_path = output_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_audit_path(shadow_results_path));
    let total_attempted = outcomes.len();
    let audit = ApplyOutput {
        apply_started_at,
        apply_completed_at,
        operator: operator_str,
        shadow_results_path: shadow_results_path.display().to_string(),
        outcomes,
        total_attempted,
        succeeded,
        failed,
    };
    let audit_json = serde_json::to_string_pretty(&audit)
        .context("serializing ApplyOutput audit JSON")?;
    std::fs::write(&audit_path, audit_json).with_context(|| {
        format!("writing ApplyOutput audit JSON to {}", audit_path.display())
    })?;

    // (7) One-line summary and exit code.
    println!(
        "Applied {}/{} APPROVED outcomes ({} failed). Audit JSON: {}",
        succeeded, total_attempted, failed, audit_path.display()
    );
    info!(
        succeeded,
        failed,
        total_attempted,
        audit = %audit_path.display(),
        "apply complete"
    );

    Ok(if failed > 0 { 1 } else { 0 })
}

/// Partition outcomes into `(approved, skipped)`. `approved` keeps only the
/// outcomes that are safe to apply: `status == APPROVED` AND non-empty
/// `result_id` (an APPROVED status without a result_id is malformed input —
/// the shadow workflow only populates result_id on APPROVED, but defending
/// here closes the gap if a hand-edited file ever shows up).
fn partition_outcomes(
    outcomes: &[ShadowOutcome],
) -> (Vec<ShadowOutcome>, Vec<ShadowOutcome>) {
    let mut approved = Vec::new();
    let mut skipped = Vec::new();
    for o in outcomes {
        if o.status == "APPROVED" && !o.result_id.is_empty() {
            approved.push(o.clone());
        } else {
            skipped.push(o.clone());
        }
    }
    (approved, skipped)
}

/// Log each skipped outcome at WARN with its status, so the operator can
/// see what was excluded and why without having to grep the input JSON.
fn log_skipped_outcomes(skipped: &[ShadowOutcome]) {
    for o in skipped {
        if o.status == "APPROVED" {
            // APPROVED but empty result_id — input malformed; flag loudly.
            warn!(
                original_metric_id = %o.original_metric_id,
                "skipping outcome: APPROVED status but empty result_id (malformed input)"
            );
        } else {
            info!(
                original_metric_id = %o.original_metric_id,
                status = %o.status,
                reason = %o.reason,
                "skipping non-APPROVED outcome"
            );
        }
    }
}

/// Print the planned migrations as a simple pipe-separated table. Operators
/// run `--dry-run` first; the output is meant to be human-skimmable, not
/// machine-parseable (the sidecar JSON is the machine-readable artifact).
fn print_dry_run_plan(approved: &[ShadowOutcome]) {
    println!("DRY RUN — would apply {} APPROVED migration(s):", approved.len());
    println!(
        "  {:<32} | {:<32} | {:<14} | {:<36} | {:<6}",
        "old_metric_id", "new_metric_id", "type", "shadow_run_result_id", "days"
    );
    println!(
        "  {:-<32}-+-{:-<32}-+-{:-<14}-+-{:-<36}-+-{:-<6}",
        "", "", "", "", ""
    );
    for o in approved {
        let new_id = o
            .candidate_metric
            .get("metric_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let type_label = describe_candidate_type(&o.candidate_metric);
        let days = if o.total_days > 0 {
            format!("{}/{}", o.days_within_tolerance, o.total_days)
        } else {
            "-".to_string()
        };
        println!(
            "  {:<32} | {:<32} | {:<14} | {:<36} | {:<6}",
            o.original_metric_id, new_id, type_label, o.result_id, days
        );
    }
    println!();
    println!("DRY RUN — no migrations applied. Re-run with --confirm --operator <name> to apply.");
}

/// Human-readable label for the candidate metric's type, derived from the
/// echoed `candidate_metric` JSON (which is in `migration::report::metric_to_json`
/// shape — keys are `filtered_mean` / `composite` / `windowed_count` /
/// `metricql_expression` / `custom_sql`).
fn describe_candidate_type(candidate: &serde_json::Value) -> String {
    if candidate.get("filtered_mean").is_some() {
        return "FILTERED_MEAN".to_string();
    }
    if candidate.get("composite").is_some() {
        return "COMPOSITE".to_string();
    }
    if candidate.get("windowed_count").is_some() {
        return "WINDOWED_COUNT".to_string();
    }
    if candidate
        .get("metricql_expression")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        return "METRICQL".to_string();
    }
    // Fall back to the numeric `type` field if it's present and recognised.
    match candidate.get("type").and_then(|v| v.as_i64()) {
        Some(1) => "MEAN".to_string(),
        Some(2) => "RATIO".to_string(),
        Some(3) => "PROPORTION".to_string(),
        Some(4) => "COUNT".to_string(),
        Some(5) => "RATE".to_string(),
        Some(6) => "CUSTOM".to_string(),
        Some(other) => format!("type={other}"),
        None => "unknown".to_string(),
    }
}

/// Apply one APPROVED outcome via `MigrateMetricDefinition`. Returns an
/// `ApplyOutcome` capturing success or failure verbatim — never panics, so
/// the outer loop can continue with the next outcome.
async fn apply_one(
    client: &mut ExperimentManagementServiceClient<tonic::transport::Channel>,
    outcome: &ShadowOutcome,
    operator: &str,
) -> ApplyOutcome {
    // (a) Re-decode the candidate MetricDefinition from the echoed JSON. We
    // intentionally use the same helper as the shadow subcommand
    // (`json_to_metric_definition`) so this round-trips with the JSON shape
    // written by `metric_to_json`. A decode failure here is a malformed
    // ShadowOutput input — we surface it as InvalidArgument-style failure
    // for this outcome rather than aborting the whole apply run.
    let new_metric = match json_to_metric_definition(&outcome.candidate_metric) {
        Ok(m) => m,
        Err(e) => {
            return ApplyOutcome {
                original_metric_id: outcome.original_metric_id.clone(),
                new_metric_id: String::new(),
                shadow_run_result_id: outcome.result_id.clone(),
                migration_id: String::new(),
                applied_at: String::new(),
                status: "INVALID_INPUT".to_string(),
                error: format!("decoding candidate_metric: {e:#}"),
            };
        }
    };

    // (b) Build and send the request.
    let req = MigrateMetricDefinitionRequest {
        old_metric_id: outcome.original_metric_id.clone(),
        new_metric: Some(new_metric),
        shadow_run_result_id: outcome.result_id.clone(),
        operator: operator.to_string(),
    };

    match client.migrate_metric_definition(req).await {
        Ok(resp) => {
            let inner = resp.into_inner();
            ApplyOutcome {
                original_metric_id: outcome.original_metric_id.clone(),
                new_metric_id: inner.new_metric_id,
                shadow_run_result_id: outcome.result_id.clone(),
                migration_id: inner.migration_id,
                applied_at: inner.applied_at,
                status: "OK".to_string(),
                error: String::new(),
            }
        }
        Err(status) => ApplyOutcome {
            original_metric_id: outcome.original_metric_id.clone(),
            new_metric_id: String::new(),
            shadow_run_result_id: outcome.result_id.clone(),
            migration_id: String::new(),
            applied_at: String::new(),
            status: format!("{:?}", status.code()).to_uppercase(),
            error: status.message().to_string(),
        },
    }
}

/// Default sidecar audit path: `<shadow>.apply.json`. Mirrors the convention
/// `<shadow>.apply.json` referenced in the operator runbook.
fn default_audit_path(shadow_results_path: &Path) -> PathBuf {
    let mut s = shadow_results_path.as_os_str().to_owned();
    s.push(".apply.json");
    PathBuf::from(s)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::Mutex;

    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;
    use tonic::{Code, Request, Response, Status};

    use experimentation_proto::experimentation::metrics::v1::{
        metric_computation_service_server::{
            MetricComputationService, MetricComputationServiceServer,
        },
        CompileMetricqlPreviewRequest, CompileMetricqlPreviewResponse,
        ComputeGuardrailMetricsRequest, ComputeMetricsRequest, ComputeMetricsResponse,
        ExportNotebookRequest, ExportNotebookResponse, GetQueryLogRequest, GetQueryLogResponse,
        GetShadowResultsResponse, PromoteShadowResultResponse,
        ScheduleShadowComputationResponse,
    };

    // -----------------------------------------------------------------------
    // Pure-function unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_duration_accepts_seconds_minutes_hours_days() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7_200));
        assert_eq!(parse_duration("7d").unwrap(), Duration::from_secs(604_800));
    }

    #[test]
    fn parse_duration_rejects_empty_zero_and_unknown_units() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("0s").is_err(), "zero must be rejected");
        assert!(parse_duration("0d").is_err());
        assert!(parse_duration("10x").is_err(), "unknown unit must be rejected");
        assert!(parse_duration("abc").is_err(), "non-numeric prefix must fail");
        assert!(parse_duration("10").is_err(), "missing unit suffix must fail");
    }

    #[test]
    fn is_shadow_eligible_tier_accepts_tier1_and_tier2_only() {
        assert!(is_shadow_eligible_tier("tier1_filtered_mean"));
        assert!(is_shadow_eligible_tier("tier1_composite"));
        assert!(is_shadow_eligible_tier("tier1_windowed_count"));
        assert!(is_shadow_eligible_tier("tier2_metricql"));
        assert!(!is_shadow_eligible_tier("tier3_untranslatable"));
        assert!(!is_shadow_eligible_tier(""));
        assert!(!is_shadow_eligible_tier("tier1_bogus"));
    }

    #[test]
    fn log_summary_counts_only_approved() {
        let outcomes = vec![
            outcome("a", "APPROVED"),
            outcome("b", "REJECTED"),
            outcome("c", "PENDING"),
            outcome("d", "APPROVED"),
            outcome("e", "SCHEDULING_FAILED"),
        ];
        assert_eq!(log_summary(&outcomes), 2);
    }

    #[test]
    fn extract_shadow_candidates_filters_tier3_and_preserves_proposals() {
        let proposals_json = serde_json::json!({
            "summary": { "total": 3 },
            "entries": [
                proposal_entry("m_fm", "tier1_filtered_mean", true),
                proposal_entry("m_t3", "tier3_untranslatable", false),
                proposal_entry("m_mql", "tier2_metricql", true),
            ]
        });

        let candidates = extract_shadow_candidates(&proposals_json).unwrap();
        let ids: Vec<&str> = candidates
            .iter()
            .map(|c| c.original_metric_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m_fm", "m_mql"], "tier3 must be filtered out");

        // candidate_json should be the same shape as the proposals' "proposal"
        // field (so apply can re-decode with json_to_metric_definition).
        let first = &candidates[0];
        assert_eq!(
            first.candidate_json.get("metric_id").and_then(|v| v.as_str()),
            Some("m_fm")
        );
        // candidate_metric is the decoded proto MetricDefinition.
        assert_eq!(first.candidate_metric.metric_id, "m_fm");
    }

    #[test]
    fn extract_shadow_candidates_errors_on_missing_proposal_for_tier1() {
        // Tier 1 entry with a null proposal would be malformed input; surface
        // an error rather than silently dropping it.
        let proposals_json = serde_json::json!({
            "entries": [{
                "original_metric_id": "broken",
                "tier": "tier1_filtered_mean",
                "reason": "x",
                "proposal": null,
                "parse_error": null
            }]
        });
        assert!(extract_shadow_candidates(&proposals_json).is_err());
    }

    #[test]
    fn extract_shadow_candidates_errors_on_missing_entries_array() {
        let proposals_json = serde_json::json!({ "summary": { "total": 0 } });
        assert!(extract_shadow_candidates(&proposals_json).is_err());
    }

    // -----------------------------------------------------------------------
    // Mock M3 — programmable Schedule / Promote / GetResults responses
    // -----------------------------------------------------------------------

    /// Sequence of `PromoteShadowResultResponse` payloads to return per call,
    /// keyed by shadow_id. Each Vec is consumed front-to-back; the final
    /// element repeats if calls exceed the vec length (useful for PENDING
    /// budget-exhaust). If a shadow_id has no entry, the mock returns
    /// `PENDING` indefinitely.
    type PromoteScript = HashMap<String, Vec<PromoteShadowResultResponse>>;

    /// Per-shadow_id `GetShadowResults` snapshot used by `snapshot_progress`.
    type GetResultsMap = HashMap<String, GetShadowResultsResponse>;

    /// Behavior knob for `ScheduleShadowComputation`. The "next id" counter
    /// hands out deterministic shadow_ids `s-1`, `s-2`, ... unless the test
    /// has injected a forced-error code for a specific call number (1-based).
    #[derive(Default)]
    struct ScheduleBehavior {
        /// Map from call ordinal (1-based) → tonic error code to return.
        force_errors: HashMap<u32, (Code, String)>,
        next_id: u32,
    }

    struct MockM3 {
        schedule: Mutex<ScheduleBehavior>,
        promote: Mutex<PromoteScript>,
        get_results: Mutex<GetResultsMap>,
        promote_call_count: Mutex<HashMap<String, u32>>,
    }

    #[tonic::async_trait]
    impl MetricComputationService for MockM3 {
        async fn schedule_shadow_computation(
            &self,
            req: Request<ScheduleShadowComputationRequest>,
        ) -> Result<Response<ScheduleShadowComputationResponse>, Status> {
            let mut sb = self.schedule.lock().unwrap();
            sb.next_id += 1;
            let call_num = sb.next_id;
            if let Some((code, msg)) = sb.force_errors.remove(&call_num) {
                return Err(Status::new(code, msg));
            }
            let _ = req.into_inner(); // intentionally ignore body in tests
            let shadow_id = format!("s-{}", call_num);
            Ok(Response::new(ScheduleShadowComputationResponse { shadow_id }))
        }

        async fn promote_shadow_result(
            &self,
            req: Request<PromoteShadowResultRequest>,
        ) -> Result<Response<PromoteShadowResultResponse>, Status> {
            let inner = req.into_inner();
            let id = inner.shadow_id.clone();
            let mut count = self.promote_call_count.lock().unwrap();
            *count.entry(id.clone()).or_insert(0) += 1;
            drop(count);

            let mut script = self.promote.lock().unwrap();
            if let Some(seq) = script.get_mut(&id) {
                let resp = if seq.len() > 1 {
                    seq.remove(0)
                } else if let Some(last) = seq.first() {
                    last.clone()
                } else {
                    pending_response("no script entries")
                };
                return Ok(Response::new(resp));
            }
            // Default: PENDING forever.
            Ok(Response::new(pending_response("no script for this id")))
        }

        async fn get_shadow_results(
            &self,
            req: Request<GetShadowResultsRequest>,
        ) -> Result<Response<GetShadowResultsResponse>, Status> {
            let inner = req.into_inner();
            let map = self.get_results.lock().unwrap();
            if let Some(snapshot) = map.get(&inner.shadow_id) {
                Ok(Response::new(snapshot.clone()))
            } else {
                Ok(Response::new(GetShadowResultsResponse {
                    shadow_id: inner.shadow_id,
                    status: "PENDING".into(),
                    rows: vec![],
                    days_within_tolerance: 0,
                    total_days: 0,
                }))
            }
        }

        // ---- Stubs for the rest of the trait surface ----
        async fn compute_metrics(
            &self,
            _req: Request<ComputeMetricsRequest>,
        ) -> Result<Response<ComputeMetricsResponse>, Status> {
            Err(Status::unimplemented("stub"))
        }
        async fn compute_guardrail_metrics(
            &self,
            _req: Request<ComputeGuardrailMetricsRequest>,
        ) -> Result<Response<ComputeMetricsResponse>, Status> {
            Err(Status::unimplemented("stub"))
        }
        async fn export_notebook(
            &self,
            _req: Request<ExportNotebookRequest>,
        ) -> Result<Response<ExportNotebookResponse>, Status> {
            Err(Status::unimplemented("stub"))
        }
        async fn get_query_log(
            &self,
            _req: Request<GetQueryLogRequest>,
        ) -> Result<Response<GetQueryLogResponse>, Status> {
            Err(Status::unimplemented("stub"))
        }
        async fn compile_metricql_preview(
            &self,
            _req: Request<CompileMetricqlPreviewRequest>,
        ) -> Result<Response<CompileMetricqlPreviewResponse>, Status> {
            Err(Status::unimplemented("stub"))
        }
    }

    fn pending_response(reason: &str) -> PromoteShadowResultResponse {
        PromoteShadowResultResponse {
            result_id: String::new(),
            status: "PENDING".into(),
            reason: reason.into(),
        }
    }

    /// Spawn a mock M3 listening on a random port. Returns the bound address
    /// and a handle to the mock for test-side script mutation.
    async fn spawn_mock_m3(mock: MockM3) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            Server::builder()
                .add_service(MetricComputationServiceServer::new(mock))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        tokio::task::yield_now().await;
        addr
    }

    // -----------------------------------------------------------------------
    // Workflow tests with mock M3
    // -----------------------------------------------------------------------

    /// Build a proposals.json `Value` with one Tier-1 entry per id.
    fn build_proposals(ids_and_tiers: &[(&str, &str)]) -> serde_json::Value {
        let entries: Vec<serde_json::Value> = ids_and_tiers
            .iter()
            .map(|(id, tier)| {
                proposal_entry(id, tier, !tier.starts_with("tier3_"))
            })
            .collect();
        serde_json::json!({
            "summary": { "total": entries.len() },
            "entries": entries,
        })
    }

    /// Construct a proposals.json entry with a minimal proposal payload that
    /// `json_to_metric_definition` can decode. For Tier-3 entries
    /// `with_proposal=false` drops the proposal field (matches real shape).
    fn proposal_entry(
        metric_id: &str,
        tier: &str,
        with_proposal: bool,
    ) -> serde_json::Value {
        if with_proposal {
            serde_json::json!({
                "original_metric_id": metric_id,
                "tier": tier,
                "reason": format!("test reason for {}", metric_id),
                "proposal": {
                    "metric_id": metric_id,
                    "name": format!("{} name", metric_id),
                    "type": 1
                },
                "parse_error": null,
            })
        } else {
            serde_json::json!({
                "original_metric_id": metric_id,
                "tier": tier,
                "reason": "tier3 reason",
                "proposal": null,
                "parse_error": "parse failed",
            })
        }
    }

    fn outcome(id: &str, status: &str) -> ShadowOutcome {
        ShadowOutcome {
            original_metric_id: id.to_string(),
            shadow_id: format!("s-{}", id),
            result_id: String::new(),
            status: status.to_string(),
            reason: String::new(),
            days_within_tolerance: 0,
            total_days: 0,
            candidate_metric: serde_json::json!({}),
        }
    }

    /// Build mock M3 state and a single proposals.json file in a temp dir.
    async fn setup_workflow(
        mock: MockM3,
        proposals_value: serde_json::Value,
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf, String) {
        let addr = spawn_mock_m3(mock).await;
        let dir = tempfile::TempDir::new().unwrap();
        let proposals_path = dir.path().join("proposals.json");
        std::fs::write(
            &proposals_path,
            serde_json::to_string_pretty(&proposals_value).unwrap(),
        )
        .unwrap();
        let output_path = dir.path().join("shadow.json");
        (
            dir,
            proposals_path,
            output_path,
            format!("http://{addr}"),
        )
    }

    /// Read+parse the shadow output file from disk.
    fn read_shadow_output(path: &std::path::Path) -> ShadowOutput {
        let raw = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    /// Ensure the shadow workflow polls every second instead of every minute
    /// so PENDING budgets drain in test time.
    ///
    /// Because environment variables are process-global, simultaneous tests
    /// would race. We use a `OnceLock` to set the var exactly once for the
    /// whole test binary — every test in this module then observes the same
    /// (small) poll interval. This is safe because no test in this module
    /// needs a *different* poll cadence.
    fn ensure_fast_polling() {
        use std::sync::OnceLock;
        static INIT: OnceLock<()> = OnceLock::new();
        INIT.get_or_init(|| {
            // SAFETY: set_var happens before any thread reads
            // POLL_INTERVAL_ENV (poll_interval_from_env is only invoked
            // inside shadow_subcommand, which test bodies call after this).
            std::env::set_var(POLL_INTERVAL_ENV, "1");
        });
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn happy_path_two_proposals_both_approved() {
        ensure_fast_polling();

        let mut promote = PromoteScript::new();
        promote.insert(
            "s-1".to_string(),
            vec![PromoteShadowResultResponse {
                result_id: "s-1".into(),
                status: "APPROVED".into(),
                reason: String::new(),
            }],
        );
        promote.insert(
            "s-2".to_string(),
            vec![PromoteShadowResultResponse {
                result_id: "s-2".into(),
                status: "APPROVED".into(),
                reason: String::new(),
            }],
        );

        let mut get_results = GetResultsMap::new();
        get_results.insert(
            "s-1".to_string(),
            GetShadowResultsResponse {
                shadow_id: "s-1".into(),
                status: "APPROVED".into(),
                rows: vec![],
                days_within_tolerance: 7,
                total_days: 7,
            },
        );
        get_results.insert(
            "s-2".to_string(),
            GetShadowResultsResponse {
                shadow_id: "s-2".into(),
                status: "APPROVED".into(),
                rows: vec![],
                days_within_tolerance: 7,
                total_days: 7,
            },
        );

        let mock = MockM3 {
            schedule: Mutex::new(ScheduleBehavior::default()),
            promote: Mutex::new(promote),
            get_results: Mutex::new(get_results),
            promote_call_count: Mutex::new(HashMap::new()),
        };

        let proposals = build_proposals(&[
            ("m_a", "tier1_filtered_mean"),
            ("m_b", "tier2_metricql"),
        ]);
        let (_dir, proposals_path, output_path, m3_url) =
            setup_workflow(mock, proposals).await;

        let exit_code = shadow_subcommand(&proposals_path, &m3_url, "30s", &output_path)
            .await
            .unwrap();
        assert_eq!(exit_code, 0, "any APPROVED should yield exit code 0");

        let out = read_shadow_output(&output_path);
        assert_eq!(out.outcomes.len(), 2);
        for o in &out.outcomes {
            assert_eq!(o.status, "APPROVED");
            assert!(!o.result_id.is_empty(), "APPROVED outcomes must carry result_id");
            assert_eq!(o.days_within_tolerance, 7);
            assert_eq!(o.total_days, 7);
        }
        // candidate_metric round-trips back to a MetricDefinition without panic.
        for o in &out.outcomes {
            let m = json_to_metric_definition(&o.candidate_metric).unwrap();
            assert_eq!(m.metric_id, o.original_metric_id);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejected_outcome_propagates_reason_and_empty_result_id() {
        ensure_fast_polling();

        let mut promote = PromoteScript::new();
        promote.insert(
            "s-1".to_string(),
            vec![PromoteShadowResultResponse {
                result_id: String::new(),
                status: "REJECTED".into(),
                reason: "diff_abs exceeded tolerance on day 4".into(),
            }],
        );

        let mock = MockM3 {
            schedule: Mutex::new(ScheduleBehavior::default()),
            promote: Mutex::new(promote),
            get_results: Mutex::new(GetResultsMap::new()),
            promote_call_count: Mutex::new(HashMap::new()),
        };

        let proposals = build_proposals(&[("m_x", "tier1_composite")]);
        let (_dir, proposals_path, output_path, m3_url) =
            setup_workflow(mock, proposals).await;

        let exit_code = shadow_subcommand(&proposals_path, &m3_url, "10s", &output_path)
            .await
            .unwrap();
        assert_eq!(exit_code, 1, "no APPROVED => exit 1");

        let out = read_shadow_output(&output_path);
        assert_eq!(out.outcomes.len(), 1);
        let o = &out.outcomes[0];
        assert_eq!(o.status, "REJECTED");
        assert!(o.result_id.is_empty(), "REJECTED must not carry result_id");
        assert!(
            o.reason.contains("diff_abs"),
            "rejection reason should propagate; got: {}",
            o.reason
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pending_outcome_when_budget_exhausts() {
        ensure_fast_polling();

        // No script entry => mock returns PENDING forever.
        let mock = MockM3 {
            schedule: Mutex::new(ScheduleBehavior::default()),
            promote: Mutex::new(PromoteScript::new()),
            get_results: Mutex::new(GetResultsMap::new()),
            promote_call_count: Mutex::new(HashMap::new()),
        };

        let proposals = build_proposals(&[("m_p", "tier1_filtered_mean")]);
        let (_dir, proposals_path, output_path, m3_url) =
            setup_workflow(mock, proposals).await;

        // Budget = 2s, poll cadence = 1s. Budget exhausts after a couple of
        // polls.
        let exit_code = shadow_subcommand(&proposals_path, &m3_url, "2s", &output_path)
            .await
            .unwrap();
        assert_eq!(exit_code, 1, "no APPROVED => exit 1");

        let out = read_shadow_output(&output_path);
        assert_eq!(out.outcomes.len(), 1);
        let o = &out.outcomes[0];
        assert_eq!(o.status, "PENDING");
        assert!(o.result_id.is_empty());
        assert!(
            o.reason.to_lowercase().contains("budget"),
            "reason should mention budget; got: {}",
            o.reason
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn scheduling_failure_is_recorded_but_does_not_abort_remaining_proposals() {
        ensure_fast_polling();

        // First Schedule call fails; second succeeds and is APPROVED.
        let mut schedule_behavior = ScheduleBehavior::default();
        schedule_behavior.force_errors.insert(
            1,
            (Code::Unavailable, "M3 ECONNREFUSED simulated".into()),
        );

        let mut promote = PromoteScript::new();
        // The second Schedule call hands out shadow_id "s-2" (next_id=2).
        promote.insert(
            "s-2".to_string(),
            vec![PromoteShadowResultResponse {
                result_id: "s-2".into(),
                status: "APPROVED".into(),
                reason: String::new(),
            }],
        );

        let mock = MockM3 {
            schedule: Mutex::new(schedule_behavior),
            promote: Mutex::new(promote),
            get_results: Mutex::new(GetResultsMap::new()),
            promote_call_count: Mutex::new(HashMap::new()),
        };

        let proposals = build_proposals(&[
            ("m_fail", "tier1_filtered_mean"),
            ("m_ok", "tier2_metricql"),
        ]);
        let (_dir, proposals_path, output_path, m3_url) =
            setup_workflow(mock, proposals).await;

        let exit_code = shadow_subcommand(&proposals_path, &m3_url, "10s", &output_path)
            .await
            .unwrap();
        assert_eq!(exit_code, 0, "second proposal APPROVED => exit 0");

        let out = read_shadow_output(&output_path);
        assert_eq!(out.outcomes.len(), 2);

        let failed = out.outcomes.iter().find(|o| o.original_metric_id == "m_fail").unwrap();
        assert_eq!(failed.status, STATUS_SCHEDULING_FAILED);
        assert!(failed.shadow_id.is_empty(), "no shadow_id when scheduling failed");
        assert!(failed.result_id.is_empty());
        assert!(
            failed.reason.contains("ScheduleShadowComputation failed"),
            "scheduling-failed reason should mention the RPC; got: {}",
            failed.reason
        );

        let ok = out.outcomes.iter().find(|o| o.original_metric_id == "m_ok").unwrap();
        assert_eq!(ok.status, "APPROVED");
        assert_eq!(ok.result_id, "s-2");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tier3_proposals_are_skipped_with_warning() {
        ensure_fast_polling();

        // Only the Tier-1 entry gets a script entry; Tier-3 is dropped before
        // any RPC call.
        let mut promote = PromoteScript::new();
        promote.insert(
            "s-1".to_string(),
            vec![PromoteShadowResultResponse {
                result_id: "s-1".into(),
                status: "APPROVED".into(),
                reason: String::new(),
            }],
        );

        let mock = MockM3 {
            schedule: Mutex::new(ScheduleBehavior::default()),
            promote: Mutex::new(promote),
            get_results: Mutex::new(GetResultsMap::new()),
            promote_call_count: Mutex::new(HashMap::new()),
        };

        let proposals = build_proposals(&[
            ("m_keep", "tier1_filtered_mean"),
            ("m_skip", "tier3_untranslatable"),
        ]);
        let (_dir, proposals_path, output_path, m3_url) =
            setup_workflow(mock, proposals).await;

        let exit_code = shadow_subcommand(&proposals_path, &m3_url, "10s", &output_path)
            .await
            .unwrap();
        assert_eq!(exit_code, 0);

        let out = read_shadow_output(&output_path);
        assert_eq!(out.outcomes.len(), 1, "tier3 entry must not appear in outcomes");
        let o = &out.outcomes[0];
        assert_eq!(o.original_metric_id, "m_keep");
        assert_eq!(o.status, "APPROVED");
        // No outcome references the skipped id.
        assert!(out
            .outcomes
            .iter()
            .all(|o| o.original_metric_id != "m_skip"));
    }

    // -----------------------------------------------------------------------
    // apply — pure-function unit tests
    // -----------------------------------------------------------------------

    fn approved_outcome(id: &str, result_id: &str) -> ShadowOutcome {
        ShadowOutcome {
            original_metric_id: id.to_string(),
            shadow_id: format!("s-{}", id),
            result_id: result_id.to_string(),
            status: "APPROVED".to_string(),
            reason: String::new(),
            days_within_tolerance: 7,
            total_days: 7,
            // Minimal candidate_metric that round-trips through
            // `json_to_metric_definition` — same shape `proposal_entry` uses.
            candidate_metric: serde_json::json!({
                "metric_id": format!("{}_v2", id),
                "name": format!("{} v2", id),
                "type": 1,
            }),
        }
    }

    #[test]
    fn partition_outcomes_keeps_only_approved_with_result_id() {
        let outcomes = vec![
            approved_outcome("a", "uuid-a"),
            outcome("b", "REJECTED"),
            outcome("c", "PENDING"),
            outcome("d", "SCHEDULING_FAILED"),
            // APPROVED with empty result_id is malformed — must be skipped.
            {
                let mut o = approved_outcome("malformed", "");
                o.result_id = String::new();
                o
            },
        ];
        let (approved, skipped) = partition_outcomes(&outcomes);
        assert_eq!(approved.len(), 1, "only the valid APPROVED outcome survives");
        assert_eq!(approved[0].original_metric_id, "a");
        assert_eq!(skipped.len(), 4, "everything else is skipped");
    }

    #[test]
    fn describe_candidate_type_handles_tier1_and_tier2() {
        assert_eq!(
            describe_candidate_type(&serde_json::json!({"filtered_mean": {}})),
            "FILTERED_MEAN"
        );
        assert_eq!(
            describe_candidate_type(&serde_json::json!({"composite": {}})),
            "COMPOSITE"
        );
        assert_eq!(
            describe_candidate_type(&serde_json::json!({"windowed_count": {}})),
            "WINDOWED_COUNT"
        );
        assert_eq!(
            describe_candidate_type(
                &serde_json::json!({"metricql_expression": "mean(x.v)"})
            ),
            "METRICQL"
        );
        // Empty metricql_expression should NOT count as METRICQL — fall
        // through to numeric `type`.
        assert_eq!(
            describe_candidate_type(
                &serde_json::json!({"metricql_expression": "", "type": 1})
            ),
            "MEAN"
        );
        assert_eq!(
            describe_candidate_type(&serde_json::json!({})),
            "unknown"
        );
    }

    #[test]
    fn default_audit_path_appends_apply_json_suffix() {
        assert_eq!(
            default_audit_path(Path::new("shadow.json")),
            PathBuf::from("shadow.json.apply.json")
        );
        assert_eq!(
            default_audit_path(Path::new("/tmp/run-42/shadow.json")),
            PathBuf::from("/tmp/run-42/shadow.json.apply.json")
        );
    }

    #[test]
    fn cli_rejects_dry_run_and_confirm_together() {
        // clap's group="mode" enforces this at parse time. We verify by
        // calling try_parse_from with both flags and asserting an error.
        // (Cli doesn't derive Debug, so we match on `is_err` + render manually.)
        let result = Cli::try_parse_from([
            "custom_migrator",
            "apply",
            "--shadow-results", "/tmp/s.json",
            "--m5-addr", "http://localhost:50055",
            "--dry-run",
            "--confirm",
        ]);
        let err = match result {
            Ok(_) => panic!("--dry-run and --confirm must be mutually exclusive"),
            Err(e) => e,
        };
        let rendered = err.to_string();
        assert!(
            rendered.contains("cannot be used with") || rendered.contains("mutually exclusive"),
            "error should mention mutual exclusion; got: {rendered}"
        );
    }

    // -----------------------------------------------------------------------
    // Mock M5 — programmable MigrateMetricDefinition responses
    // -----------------------------------------------------------------------
    //
    // The mock implements the full `ExperimentManagementService` trait so it
    // can back a real tonic server, but only `migrate_metric_definition`
    // returns useful responses. The other 26 RPCs panic via `unimplemented`
    // (we never call them from the apply path; any unexpected call is a
    // test bug we want to surface loudly).

    /// Programmed result for one MigrateMetricDefinition call. Tests push a
    /// sequence into `MockM5.migrate_script`; the mock pops front-to-back.
    /// If the script empties before a call, the mock returns `Code::Internal`
    /// with a "no script entries" message — preserves the test's intent
    /// (don't silently fall through to a meaningless Ok).
    enum MigrateResult {
        Ok {
            new_metric_id: String,
            migration_id: String,
            applied_at: String,
        },
        Err(Code, String),
    }

    struct MockM5 {
        migrate_script: Mutex<Vec<MigrateResult>>,
        migrate_calls: Mutex<Vec<MigrateMetricDefinitionRequest>>,
    }

    impl MockM5 {
        fn new(script: Vec<MigrateResult>) -> Self {
            Self {
                migrate_script: Mutex::new(script),
                migrate_calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[tonic::async_trait]
    impl experimentation_proto::experimentation::management::v1::experiment_management_service_server::ExperimentManagementService
        for MockM5
    {
        async fn migrate_metric_definition(
            &self,
            request: Request<MigrateMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::MigrateMetricDefinitionResponse>, Status>
        {
            let req = request.into_inner();
            self.migrate_calls.lock().unwrap().push(req.clone());
            let mut script = self.migrate_script.lock().unwrap();
            if script.is_empty() {
                return Err(Status::new(
                    Code::Internal,
                    "MockM5 migrate_script exhausted",
                ));
            }
            match script.remove(0) {
                MigrateResult::Ok {
                    new_metric_id,
                    migration_id,
                    applied_at,
                } => Ok(Response::new(
                    experimentation_proto::experimentation::management::v1::MigrateMetricDefinitionResponse {
                        new_metric_id,
                        migration_id,
                        applied_at,
                    },
                )),
                MigrateResult::Err(code, msg) => Err(Status::new(code, msg)),
            }
        }

        // ---- Stubs for the rest of the trait surface ----
        // We never call these from the apply workflow; any unexpected call
        // is a test-design bug. `unimplemented!` panics with a clear message,
        // which will fail the test loudly rather than silently returning
        // bogus data.

        async fn create_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::CreateExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: create_experiment")
        }
        async fn get_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::GetExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: get_experiment")
        }
        async fn update_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::UpdateExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: update_experiment")
        }
        async fn list_experiments(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::ListExperimentsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ListExperimentsResponse>, Status> {
            unimplemented!("MockM5 stub: list_experiments")
        }
        async fn start_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::StartExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: start_experiment")
        }
        async fn conclude_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::ConcludeExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: conclude_experiment")
        }
        async fn archive_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::ArchiveExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: archive_experiment")
        }
        async fn pause_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::PauseExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: pause_experiment")
        }
        async fn resume_experiment(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::ResumeExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            unimplemented!("MockM5 stub: resume_experiment")
        }
        async fn create_metric_definition(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::CreateMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::MetricDefinition>, Status> {
            unimplemented!("MockM5 stub: create_metric_definition")
        }
        async fn get_metric_definition(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::GetMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::MetricDefinition>, Status> {
            unimplemented!("MockM5 stub: get_metric_definition")
        }
        async fn list_metric_definitions(
            &self,
            _r: Request<ListMetricDefinitionsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ListMetricDefinitionsResponse>, Status> {
            unimplemented!("MockM5 stub: list_metric_definitions")
        }
        async fn create_layer(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::CreateLayerRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Layer>, Status> {
            unimplemented!("MockM5 stub: create_layer")
        }
        async fn get_layer(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::GetLayerRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Layer>, Status> {
            unimplemented!("MockM5 stub: get_layer")
        }
        async fn get_layer_allocations(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::GetLayerAllocationsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::GetLayerAllocationsResponse>, Status> {
            unimplemented!("MockM5 stub: get_layer_allocations")
        }
        async fn create_targeting_rule(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::CreateTargetingRuleRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::TargetingRule>, Status> {
            unimplemented!("MockM5 stub: create_targeting_rule")
        }
        async fn create_surrogate_model(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::CreateSurrogateModelRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::SurrogateModelConfig>, Status> {
            unimplemented!("MockM5 stub: create_surrogate_model")
        }
        async fn list_surrogate_models(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::ListSurrogateModelsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ListSurrogateModelsResponse>, Status> {
            unimplemented!("MockM5 stub: list_surrogate_models")
        }
        async fn get_surrogate_calibration(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::GetSurrogateCalibrationRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::SurrogateModelConfig>, Status> {
            unimplemented!("MockM5 stub: get_surrogate_calibration")
        }
        async fn trigger_surrogate_recalibration(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::TriggerSurrogateRecalibrationRequest>,
        ) -> Result<Response<()>, Status> {
            unimplemented!("MockM5 stub: trigger_surrogate_recalibration")
        }
        type StreamConfigUpdatesStream = std::pin::Pin<
            Box<
                dyn tokio_stream::Stream<
                        Item = Result<
                            experimentation_proto::experimentation::management::v1::ConfigUpdateEvent,
                            Status,
                        >,
                    > + Send,
            >,
        >;
        async fn stream_config_updates(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::StreamConfigUpdatesRequest>,
        ) -> Result<Response<Self::StreamConfigUpdatesStream>, Status> {
            unimplemented!("MockM5 stub: stream_config_updates")
        }
        async fn get_portfolio_allocation(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::GetPortfolioAllocationRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::GetPortfolioAllocationResponse>, Status> {
            unimplemented!("MockM5 stub: get_portfolio_allocation")
        }
        async fn validate_metricql(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::ValidateMetricqlRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ValidateMetricqlResponse>, Status> {
            unimplemented!("MockM5 stub: validate_metricql")
        }
        async fn preview_metric_definition(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::PreviewMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::PreviewMetricDefinitionResponse>, Status> {
            unimplemented!("MockM5 stub: preview_metric_definition")
        }
    }

    /// Spawn a mock M5 server. Returns the bound address and an Arc handle
    /// to the mock so tests can inspect the recorded calls.
    async fn spawn_mock_m5(mock: MockM5) -> (SocketAddr, std::sync::Arc<MockM5>) {
        use experimentation_proto::experimentation::management::v1::experiment_management_service_server::ExperimentManagementServiceServer;
        let arc = std::sync::Arc::new(mock);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // tonic's generated server takes ownership; clone via Arc indirection.
        let server_handle = ArcMockM5(arc.clone());
        tokio::spawn(async move {
            Server::builder()
                .add_service(ExperimentManagementServiceServer::new(server_handle))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        tokio::task::yield_now().await;
        (addr, arc)
    }

    /// Thin newtype wrapper so we can implement the service trait on an
    /// `Arc<MockM5>` (tonic's generated `*ServiceServer::new` needs an owned
    /// `impl Service`).
    struct ArcMockM5(std::sync::Arc<MockM5>);

    #[tonic::async_trait]
    impl experimentation_proto::experimentation::management::v1::experiment_management_service_server::ExperimentManagementService
        for ArcMockM5
    {
        async fn migrate_metric_definition(
            &self,
            request: Request<MigrateMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::MigrateMetricDefinitionResponse>, Status> {
            self.0.migrate_metric_definition(request).await
        }
        async fn create_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::CreateExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.create_experiment(r).await
        }
        async fn get_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::GetExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.get_experiment(r).await
        }
        async fn update_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::UpdateExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.update_experiment(r).await
        }
        async fn list_experiments(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::ListExperimentsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ListExperimentsResponse>, Status> {
            self.0.list_experiments(r).await
        }
        async fn start_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::StartExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.start_experiment(r).await
        }
        async fn conclude_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::ConcludeExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.conclude_experiment(r).await
        }
        async fn archive_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::ArchiveExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.archive_experiment(r).await
        }
        async fn pause_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::PauseExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.pause_experiment(r).await
        }
        async fn resume_experiment(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::ResumeExperimentRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Experiment>, Status> {
            self.0.resume_experiment(r).await
        }
        async fn create_metric_definition(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::CreateMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::MetricDefinition>, Status> {
            self.0.create_metric_definition(r).await
        }
        async fn get_metric_definition(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::GetMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::MetricDefinition>, Status> {
            self.0.get_metric_definition(r).await
        }
        async fn list_metric_definitions(
            &self,
            r: Request<ListMetricDefinitionsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ListMetricDefinitionsResponse>, Status> {
            self.0.list_metric_definitions(r).await
        }
        async fn create_layer(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::CreateLayerRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Layer>, Status> {
            self.0.create_layer(r).await
        }
        async fn get_layer(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::GetLayerRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::Layer>, Status> {
            self.0.get_layer(r).await
        }
        async fn get_layer_allocations(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::GetLayerAllocationsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::GetLayerAllocationsResponse>, Status> {
            self.0.get_layer_allocations(r).await
        }
        async fn create_targeting_rule(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::CreateTargetingRuleRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::TargetingRule>, Status> {
            self.0.create_targeting_rule(r).await
        }
        async fn create_surrogate_model(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::CreateSurrogateModelRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::SurrogateModelConfig>, Status> {
            self.0.create_surrogate_model(r).await
        }
        async fn list_surrogate_models(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::ListSurrogateModelsRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ListSurrogateModelsResponse>, Status> {
            self.0.list_surrogate_models(r).await
        }
        async fn get_surrogate_calibration(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::GetSurrogateCalibrationRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::common::v1::SurrogateModelConfig>, Status> {
            self.0.get_surrogate_calibration(r).await
        }
        async fn trigger_surrogate_recalibration(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::TriggerSurrogateRecalibrationRequest>,
        ) -> Result<Response<()>, Status> {
            self.0.trigger_surrogate_recalibration(r).await
        }
        type StreamConfigUpdatesStream = std::pin::Pin<
            Box<
                dyn tokio_stream::Stream<
                        Item = Result<
                            experimentation_proto::experimentation::management::v1::ConfigUpdateEvent,
                            Status,
                        >,
                    > + Send,
            >,
        >;
        async fn stream_config_updates(
            &self,
            _r: Request<experimentation_proto::experimentation::management::v1::StreamConfigUpdatesRequest>,
        ) -> Result<Response<Self::StreamConfigUpdatesStream>, Status> {
            unimplemented!("ArcMockM5: stream_config_updates")
        }
        async fn get_portfolio_allocation(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::GetPortfolioAllocationRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::GetPortfolioAllocationResponse>, Status> {
            self.0.get_portfolio_allocation(r).await
        }
        async fn validate_metricql(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::ValidateMetricqlRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::ValidateMetricqlResponse>, Status> {
            self.0.validate_metricql(r).await
        }
        async fn preview_metric_definition(
            &self,
            r: Request<experimentation_proto::experimentation::management::v1::PreviewMetricDefinitionRequest>,
        ) -> Result<Response<experimentation_proto::experimentation::management::v1::PreviewMetricDefinitionResponse>, Status> {
            self.0.preview_metric_definition(r).await
        }
    }

    /// Write a `ShadowOutput` JSON to a temp dir and return the paths.
    fn write_shadow_output_to_disk(
        outcomes: Vec<ShadowOutcome>,
    ) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("shadow.json");
        let out = ShadowOutput {
            schedule_started_at: "2026-01-01T00:00:00Z".into(),
            schedule_completed_at: "2026-01-01T01:00:00Z".into(),
            outcomes,
        };
        std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
        (dir, path)
    }

    // -----------------------------------------------------------------------
    // Workflow tests
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread")]
    async fn apply_dry_run_prints_plan_and_makes_no_rpc_calls() {
        // 1 APPROVED + 1 REJECTED. dry_run must not call M5.
        let outcomes = vec![
            approved_outcome("m_a", "result-1"),
            outcome("m_r", "REJECTED"),
        ];
        let (_dir, shadow_path) = write_shadow_output_to_disk(outcomes);

        // Mock with an empty script — if dry_run calls migrate, it would
        // get back "exhausted" and the call would still be recorded.
        let mock = MockM5::new(vec![]);
        let (addr, arc) = spawn_mock_m5(mock).await;

        let exit_code = apply_subcommand(
            None,
            &shadow_path,
            &format!("http://{addr}"),
            None,
            true,  // dry_run
            false, // confirm
            None,
        )
        .await
        .expect("dry_run should never return Err");

        assert_eq!(exit_code, 0, "dry_run exits 0");
        assert_eq!(
            arc.migrate_calls.lock().unwrap().len(),
            0,
            "dry_run must not call MigrateMetricDefinition"
        );
        // No sidecar audit file in dry_run mode.
        assert!(
            !default_audit_path(&shadow_path).exists(),
            "dry_run must not write the audit sidecar"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn apply_confirm_applies_all_approved_outcomes() {
        let outcomes = vec![
            approved_outcome("m_a", "result-1"),
            approved_outcome("m_b", "result-2"),
            outcome("m_skip", "REJECTED"),
        ];
        let (_dir, shadow_path) = write_shadow_output_to_disk(outcomes);

        let script = vec![
            MigrateResult::Ok {
                new_metric_id: "m_a_v2".into(),
                migration_id: "mig-1".into(),
                applied_at: "2026-01-01T01:00:00Z".into(),
            },
            MigrateResult::Ok {
                new_metric_id: "m_b_v2".into(),
                migration_id: "mig-2".into(),
                applied_at: "2026-01-01T01:00:01Z".into(),
            },
        ];
        let mock = MockM5::new(script);
        let (addr, arc) = spawn_mock_m5(mock).await;

        let exit_code = apply_subcommand(
            None,
            &shadow_path,
            &format!("http://{addr}"),
            Some("test@example.com"),
            false, // dry_run
            true,  // confirm
            None,
        )
        .await
        .expect("apply_subcommand returns Ok with code 0 on success");

        assert_eq!(exit_code, 0, "all APPROVED outcomes succeeded => exit 0");
        let calls = arc.migrate_calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "must call migrate exactly twice");
        // Verify the request fields: operator, shadow_run_result_id, old/new ids.
        assert_eq!(calls[0].operator, "test@example.com");
        assert_eq!(calls[0].old_metric_id, "m_a");
        assert_eq!(calls[0].shadow_run_result_id, "result-1");
        assert_eq!(
            calls[0].new_metric.as_ref().map(|m| m.metric_id.as_str()),
            Some("m_a_v2")
        );
        assert_eq!(calls[1].old_metric_id, "m_b");
        assert_eq!(calls[1].shadow_run_result_id, "result-2");

        // Audit JSON written to the default path with both outcomes recorded.
        let audit_path = default_audit_path(&shadow_path);
        let audit_raw = std::fs::read_to_string(&audit_path).unwrap();
        let audit: ApplyOutput = serde_json::from_str(&audit_raw).unwrap();
        assert_eq!(audit.total_attempted, 2);
        assert_eq!(audit.succeeded, 2);
        assert_eq!(audit.failed, 0);
        assert_eq!(audit.operator, "test@example.com");
        assert!(audit.outcomes.iter().all(|o| o.status == "OK"));
        assert_eq!(audit.outcomes[0].new_metric_id, "m_a_v2");
        assert_eq!(audit.outcomes[0].migration_id, "mig-1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn apply_confirm_continues_on_partial_failure_and_exits_nonzero() {
        let outcomes = vec![
            approved_outcome("m_ok", "result-1"),
            approved_outcome("m_fail", "result-2"),
        ];
        let (_dir, shadow_path) = write_shadow_output_to_disk(outcomes);

        let script = vec![
            MigrateResult::Ok {
                new_metric_id: "m_ok_v2".into(),
                migration_id: "mig-1".into(),
                applied_at: "2026-01-01T01:00:00Z".into(),
            },
            MigrateResult::Err(
                Code::FailedPrecondition,
                "shadow run result must be APPROVED but is REJECTED".into(),
            ),
        ];
        let mock = MockM5::new(script);
        let (addr, arc) = spawn_mock_m5(mock).await;

        let exit_code = apply_subcommand(
            None,
            &shadow_path,
            &format!("http://{addr}"),
            Some("test@example.com"),
            false,
            true,
            None,
        )
        .await
        .expect("partial failure should not raise — exit code carries the result");

        assert_eq!(exit_code, 1, "any per-outcome failure => exit 1");
        let calls = arc.migrate_calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "second outcome must still be attempted (apply-as-much-as-possible)"
        );

        let audit_path = default_audit_path(&shadow_path);
        let audit: ApplyOutput =
            serde_json::from_str(&std::fs::read_to_string(&audit_path).unwrap()).unwrap();
        assert_eq!(audit.total_attempted, 2);
        assert_eq!(audit.succeeded, 1);
        assert_eq!(audit.failed, 1);
        let ok = audit.outcomes.iter().find(|o| o.status == "OK").unwrap();
        assert_eq!(ok.original_metric_id, "m_ok");
        let bad = audit.outcomes.iter().find(|o| o.status != "OK").unwrap();
        assert_eq!(bad.original_metric_id, "m_fail");
        assert_eq!(bad.status, "FAILEDPRECONDITION");
        assert!(
            bad.error.contains("REJECTED"),
            "error message propagated: {}",
            bad.error
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn apply_confirm_without_operator_fails_before_any_rpc() {
        let outcomes = vec![approved_outcome("m_a", "result-1")];
        let (_dir, shadow_path) = write_shadow_output_to_disk(outcomes);

        let mock = MockM5::new(vec![]);
        let (addr, arc) = spawn_mock_m5(mock).await;

        let result = apply_subcommand(
            None,
            &shadow_path,
            &format!("http://{addr}"),
            None, // no operator
            false,
            true, // confirm
            None,
        )
        .await;

        let err = result.expect_err("must reject --confirm without --operator");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("operator"),
            "error must mention --operator; got: {msg}"
        );
        assert_eq!(
            arc.migrate_calls.lock().unwrap().len(),
            0,
            "must not call M5 when --operator is missing"
        );
        // Also: empty/whitespace --operator string is rejected.
        let result_ws = apply_subcommand(
            None,
            &shadow_path,
            &format!("http://{addr}"),
            Some("   "),
            false,
            true,
            None,
        )
        .await;
        assert!(result_ws.is_err(), "whitespace-only operator must be rejected");
        assert_eq!(arc.migrate_calls.lock().unwrap().len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn apply_with_no_approved_outcomes_is_a_noop_exit_zero() {
        let outcomes = vec![
            outcome("a", "REJECTED"),
            outcome("b", "PENDING"),
            outcome("c", "SCHEDULING_FAILED"),
        ];
        let (_dir, shadow_path) = write_shadow_output_to_disk(outcomes);

        let mock = MockM5::new(vec![]);
        let (addr, arc) = spawn_mock_m5(mock).await;

        let exit_code = apply_subcommand(
            None,
            &shadow_path,
            &format!("http://{addr}"),
            Some("test@example.com"),
            false,
            true, // confirm
            None,
        )
        .await
        .expect("no APPROVED is not an error");

        assert_eq!(exit_code, 0, "no APPROVED => exit 0 (no-op)");
        assert_eq!(
            arc.migrate_calls.lock().unwrap().len(),
            0,
            "no RPC calls when nothing to apply"
        );
        // Also: no audit file should be written when there's nothing to apply.
        assert!(
            !default_audit_path(&shadow_path).exists(),
            "no audit file when no APPROVED outcomes"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn apply_confirm_with_custom_output_path_writes_audit_there() {
        // Spot-check that --output overrides the default <shadow>.apply.json.
        let outcomes = vec![approved_outcome("m_a", "result-1")];
        let (dir, shadow_path) = write_shadow_output_to_disk(outcomes);

        let script = vec![MigrateResult::Ok {
            new_metric_id: "m_a_v2".into(),
            migration_id: "mig-1".into(),
            applied_at: "2026-01-01T01:00:00Z".into(),
        }];
        let mock = MockM5::new(script);
        let (addr, _arc) = spawn_mock_m5(mock).await;

        let custom_audit = dir.path().join("audit.json");
        let exit_code = apply_subcommand(
            None,
            &shadow_path,
            &format!("http://{addr}"),
            Some("test@example.com"),
            false,
            true,
            Some(&custom_audit),
        )
        .await
        .unwrap();
        assert_eq!(exit_code, 0);
        assert!(custom_audit.exists(), "audit JSON at the --output path");
        assert!(
            !default_audit_path(&shadow_path).exists(),
            "default audit path must be empty when --output is set"
        );
    }
}
