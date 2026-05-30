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

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use experimentation_proto::experimentation::common::v1::MetricDefinition;

use crate::validators::MetricLookup;
use classifier::ShapeHint;
use tier1::Tier;

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
/// This function is `async` because Tier 1 translation requires an async
/// `validate_metric_definition` round-trip (operand existence + cycle detection
/// for COMPOSITE types).
///
/// Returns `Tier3Untranslatable` for anything that doesn't fit a known shape.
/// The operator-visible report (Task A6) distinguishes:
///   - "did not parse" (`parse_error: Some(...)`)
///   - "parsed but no matching pattern" (`parse_error: None`, generic reason)
///   - "matched but failed M5 `validate_metric_definition` round-trip" (Tier 3)
///
/// ## Parameters
///
/// * `custom_sql` — the raw CUSTOM SQL to classify.
/// * `original` — the source `MetricDefinition` (CUSTOM type); metadata
///   fields are copied into proposals.
/// * `lookup` — a `MetricLookup` for the validation round-trip. For
///   FILTERED_MEAN and WINDOWED_COUNT, an empty lookup is sufficient.
///   For COMPOSITE, the lookup must contain the operand metric IDs so
///   that `validate_composite` passes.
pub async fn classify_and_translate<L>(
    custom_sql: &str,
    original: &MetricDefinition,
    lookup: &L,
) -> ClassificationResult
where
    L: MetricLookup + ?Sized,
{
    // Short-circuit: whitespace-only or empty SQL.
    if custom_sql.trim().is_empty() {
        return ClassificationResult::Tier3Untranslatable {
            reason: "empty SQL".to_string(),
            parse_error: None,
        };
    }

    // Step 1: parse with sqlparser (DatabricksDialect for Databricks/Spark SQL).
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

    // Step 3: for Tier 1 shapes, attempt translation + validation round-trip.
    if let Some(ref s) = shape {
        if matches!(
            s,
            ShapeHint::FilteredAggregation
                | ShapeHint::WindowedAggregation
                | ShapeHint::CompositeArithmetic
        ) {
            if let Some(proposal) =
                tier1::translate(&stmt, s.clone(), original, lookup).await
            {
                return match proposal.tier {
                    Tier::Filtered => ClassificationResult::Tier1Filtered {
                        proposal: proposal.metric,
                        reason: proposal.reason,
                    },
                    Tier::Composite => ClassificationResult::Tier1Composite {
                        proposal: proposal.metric,
                        reason: proposal.reason,
                    },
                    Tier::WindowedCount => ClassificationResult::Tier1WindowedCount {
                        proposal: proposal.metric,
                        reason: proposal.reason,
                    },
                };
            }
        }
    }

    // Step 4 (A5): Tier 2 (METRICQL) translator — attempt on any recognized shape
    // that Tier 1 rejected, or on RatioOfSums which Tier 1 explicitly skips.
    if let Some(ref s) = shape {
        if matches!(
            s,
            ShapeHint::FilteredAggregation
                | ShapeHint::WindowedAggregation
                | ShapeHint::CompositeArithmetic
                | ShapeHint::RatioOfSums
        ) {
            if let Some(proposal) =
                tier2::translate(&stmt, s.clone(), original, lookup).await
            {
                return ClassificationResult::Tier2Metricql {
                    proposal: proposal.metric,
                    reason: proposal.reason,
                };
            }
        }
    }

    // Tier 3: classified shape but no translator matched, or shape is None.
    ClassificationResult::Tier3Untranslatable {
        reason: match shape {
            Some(h) => format!("shape {h:?} recognized but no translator matched"),
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
    use crate::migration::test_support::EmptyLookup;

    fn dummy_custom_metric() -> MetricDefinition {
        MetricDefinition {
            metric_id: "test-custom-metric".to_string(),
            name: "Test Custom Metric".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn smoke_empty_sql_returns_tier3_with_empty_reason() {
        let lookup = EmptyLookup;
        let result = classify_and_translate("", &dummy_custom_metric(), &lookup).await;
        match result {
            ClassificationResult::Tier3Untranslatable { reason, parse_error } => {
                assert_eq!(reason, "empty SQL", "expected reason 'empty SQL', got: {reason:?}");
                assert!(parse_error.is_none(), "parse_error must be None for empty SQL");
            }
            _ => panic!("expected Tier3Untranslatable for empty SQL"),
        }
    }

    #[tokio::test]
    async fn non_empty_sql_returns_tier3_shape_recognized() {
        // After A3 lands, parsed SQL produces a shape hint but still falls
        // through to Tier3 (A4/A5 translators not yet implemented).
        // NOTE: after A4 lands, "SELECT AVG(value) FROM events" now classifies
        // as FilteredAggregation but translate returns None (no event_type in WHERE),
        // so it still falls through to Tier3.
        let lookup = EmptyLookup;
        let result =
            classify_and_translate("SELECT AVG(value) FROM events", &dummy_custom_metric(), &lookup).await;
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
                // Reason must mention "shape" (recognized) or "no translator" or "no match"
                let ok = reason.contains("shape") || reason.contains("SQL did not match")
                    || reason.contains("no translator");
                assert!(
                    ok,
                    "reason must mention shape hint or no-match; got: {reason:?}"
                );
            }
            _ => panic!("expected Tier3Untranslatable for this input"),
        }
    }

    #[tokio::test]
    async fn valid_filtered_mean_sql_returns_tier1_filtered() {
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'video_play' AND me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = MetricDefinition {
            metric_id: "test-fm".to_string(),
            name: "Test FM".to_string(),
            r#type: experimentation_proto::experimentation::common::v1::MetricType::Custom as i32,
            ..Default::default()
        };
        let lookup = EmptyLookup;
        let result = classify_and_translate(sql, &original, &lookup).await;
        assert!(
            matches!(result, ClassificationResult::Tier1Filtered { .. }),
            "expected Tier1Filtered"
        );
    }
}
