//! STARTING-gate validators for Phase 5 experiment types and metric definitions.
//!
//! See submodules for per-domain details:
//!   - [`experiment_types`]: META / SWITCHBACK / QUASI / EQUIVALENCE validators.
//!   - [`metric_definition`]: `validate_metric_definition`, `MetricLookup` trait, per-type helpers.
//!   - [`cycle`]: DFS cycle-detection for COMPOSITE and METRICQL metric graphs.
//!   - [`filter_sql`]: Positive-allowlist SQL-predicate validator.
//!   - [`metricql`]: MetricQL lexer, parser, AST, semantic analyzer, codegen.

pub mod cycle;
pub mod experiment_types;
pub mod filter_sql;
pub mod metric_definition;
pub mod metricql;

pub use experiment_types::validate_equivalence_test_config;
pub use metric_definition::{validate_metric_definition, MetricLookup};

use experiment_types::{validate_meta, validate_quasi, validate_switchback};

use experimentation_proto::experimentation::common::v1::{Experiment, ExperimentType};
use tonic::Status;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Validate type-specific STARTING preconditions.
///
/// Returns `Ok(())` if validation passes, or `Box<Status::failed_precondition>`
/// with a descriptive message on failure.
pub fn validate_starting(exp: &Experiment) -> Result<(), Box<Status>> {
    let exp_type = ExperimentType::try_from(exp.r#type).unwrap_or(ExperimentType::Unspecified);

    match exp_type {
        ExperimentType::Meta => validate_meta(exp)?,
        ExperimentType::Switchback => validate_switchback(exp)?,
        ExperimentType::Quasi => validate_quasi(exp)?,
        _ => {}
    }

    // ADR-027: the equivalence (TOST) config is orthogonal to experiment type
    // — any experiment can carry it. M5 (Rust) has no metric catalog (the
    // metric-definition RPCs are unimplemented stubs), so the primary-metric
    // type is not resolvable here; the structural rules below still apply and
    // the MEAN/RATIO gate is enforced by any caller that can resolve the type
    // (mirrors the ADR-020 M4a/M5 Delta-only constraint).
    if let Some(eq) = exp.equivalence_test.as_ref() {
        validate_equivalence_test_config(eq, None)?;
    }

    Ok(())
}
