//! CUSTOM-metric migration module for ADR-026 Phase 3 (#437).
//!
//! Classifies a CUSTOM-typed `MetricDefinition` (whose `custom_sql` field holds
//! legacy Spark SQL) into one of three tiers and, for Tier 1/2, produces a
//! translation proposal using AST-based rewriting via `sqlparser-rs`.
//!
//! ## Tier classification (L4 conservative-by-default)
//!
//! | Tier | Target type      | Description                                      |
//! |------|-----------------|--------------------------------------------------|
//! | 1    | FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT | Direct structural match — safe to auto-migrate |
//! | 2    | METRICQL        | Translatable via MetricQL expression emitter    |
//! | 3    | Untranslatable  | No matching pattern; requires operator review   |
//!
//! Misclassification as Tier 3 is benign; wrong translation is catastrophic.
//! When in doubt, emit `Tier3Untranslatable`.

pub mod classifier;
pub mod report;
pub mod tier1;
pub mod tier2;

use experimentation_proto::experimentation::common::v1::MetricDefinition;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of classifying and (where possible) translating a CUSTOM metric.
pub enum ClassificationResult {
    /// SQL matches a FILTERED_MEAN structural pattern; proposal ready for review.
    Tier1Filtered { proposal: MetricDefinition, reason: String },
    /// SQL matches a COMPOSITE structural pattern; proposal ready for review.
    Tier1Composite { proposal: MetricDefinition, reason: String },
    /// SQL matches a WINDOWED_COUNT structural pattern; proposal ready for review.
    Tier1WindowedCount { proposal: MetricDefinition, reason: String },
    /// SQL is translatable to a MetricQL expression; proposal ready for review.
    Tier2Metricql { proposal: MetricDefinition, reason: String },
    /// SQL could not be reliably classified or translated.
    ///
    /// `parse_error` is `Some` when the SQL failed to parse (Spark dialect);
    /// `None` when it parsed but matched no known pattern.
    Tier3Untranslatable { reason: String, parse_error: Option<String> },
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Classify a CUSTOM metric and (if possible) produce a translation proposal.
///
/// Returns `Tier3Untranslatable` for anything that doesn't fit a known shape.
/// The operator-visible report (Task A6) distinguishes:
///   - "did not parse" (`parse_error: Some(...)`)
///   - "parsed but no matching pattern" (`parse_error: None`, generic reason)
///   - "matched but failed M5 `validate_metricql` round-trip" (A5 concern)
pub fn classify_and_translate(
    custom_sql: &str,
    _original: &MetricDefinition,
) -> ClassificationResult {
    // Short-circuit: whitespace-only or empty SQL.
    if custom_sql.trim().is_empty() {
        return ClassificationResult::Tier3Untranslatable {
            reason: "empty SQL".to_string(),
            parse_error: None,
        };
    }

    // Step 1: parse with sqlparser (GenericDialect ≈ Spark SQL subset).
    let stmt = match classifier::parse_or_tier3(custom_sql) {
        Ok(s) => s,
        Err(e) => {
            return ClassificationResult::Tier3Untranslatable {
                reason: format!("SQL parse failed: {e}"),
                parse_error: Some(e),
            };
        }
    };

    // Step 2: extract a structural shape hint from the AST.
    let shape = classifier::extract_shape(&stmt);

    // Steps 3+: A4 (Tier 1) and A5 (Tier 2) will branch on `shape` here.
    // Until those tasks land, all parsed SQL still returns Tier 3, but now
    // with an informative reason instead of the scaffold stub message.
    ClassificationResult::Tier3Untranslatable {
        reason: match shape {
            Some(h) => format!("shape {h:?} recognized but no translator implemented yet"),
            None => "SQL did not match any known translator shape".into(),
        },
        parse_error: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_custom_metric() -> MetricDefinition {
        MetricDefinition {
            metric_id: "test-custom-metric".to_string(),
            name: "Test Custom Metric".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn smoke_empty_sql_returns_tier3_with_empty_reason() {
        let result = classify_and_translate("", &dummy_custom_metric());
        match result {
            ClassificationResult::Tier3Untranslatable { reason, parse_error } => {
                assert_eq!(reason, "empty SQL", "expected reason 'empty SQL', got: {reason:?}");
                assert!(parse_error.is_none(), "parse_error must be None for empty SQL");
            }
            _ => panic!("expected Tier3Untranslatable for empty SQL"),
        }
    }

    #[test]
    fn non_empty_sql_returns_tier3_shape_recognized() {
        // After A3 lands, parsed SQL produces a shape hint but still falls
        // through to Tier3 (A4/A5 translators not yet implemented).
        let result =
            classify_and_translate("SELECT AVG(value) FROM events", &dummy_custom_metric());
        match result {
            ClassificationResult::Tier3Untranslatable { reason, parse_error } => {
                // Must NOT be the old scaffold stub message.
                assert_ne!(
                    reason, "no patterns implemented yet",
                    "A3 must replace the scaffold stub; got: {reason:?}"
                );
                // Must be Tier3 with no parse error (it parsed successfully).
                assert!(
                    parse_error.is_none(),
                    "valid SQL must have parse_error = None; got: {parse_error:?}"
                );
                // Reason must mention "shape" (recognized) or "no translator"
                // (not recognized) — either is correct Tier3 post-A3.
                let ok = reason.contains("shape") || reason.contains("SQL did not match");
                assert!(
                    ok,
                    "reason must mention shape hint or no-match; got: {reason:?}"
                );
            }
            _ => panic!("expected Tier3Untranslatable for unimplemented translators"),
        }
    }
}
