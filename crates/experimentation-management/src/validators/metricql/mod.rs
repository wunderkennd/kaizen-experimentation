//! MetricQL parser/analyzer for M5 server-side validation (ADR-026 Phase 2 / #436).
//! Mirrors `services/metrics/internal/metricql/` Go implementation; corpus parity
//! enforced via `test-vectors/metricql_corpus.json`.
//!
//! ## Public entry points
//!
//! - [`validate_metricql`] — full lex + parse + semantic analysis (write-time validator).
//! - [`parse_only`] — lex + parse without semantic analysis (cycle-detector seed).

pub mod analyze;
pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod parser;

pub use analyze::AnalyzeContext;
pub use ast::{Node, Span};
pub use diagnostic::{Diagnostic, Severity};

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// ValidateContext
// ---------------------------------------------------------------------------

/// Caller context for [`validate_metricql`]. Wraps [`AnalyzeContext`] with
/// any additional config the validator may add over time.
pub struct ValidateContext<'a> {
    /// Set of metric IDs in the store. `None` = skip `@ref` existence checks.
    pub known_metric_ids: Option<&'a HashSet<String>>,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Parse + semantically analyze a MetricQL expression.
///
/// Returns the deduplicated list of `@metric_refs` the expression depends on
/// (for upstream cycle detection) on success, or a list of diagnostics
/// (line:col + message) on failure.
///
/// Error accumulation rules:
///   - Lex/parse errors short-circuit (single diagnostic) — downstream phases
///     cannot run on a malformed token stream / AST.
///   - Semantic errors accumulate — multiple unresolved refs in one expression
///     are surfaced together so operators don't play whack-a-mole.
pub fn validate_metricql(
    source: &str,
    ctx: &ValidateContext<'_>,
) -> Result<Vec<String>, Vec<Diagnostic>> {
    // Phase 1: lex.
    let tokens = lexer::tokenize(source).map_err(|e| {
        vec![Diagnostic::error(e.span, e.message)]
    })?;

    // Phase 2: parse. LexError is unreachable here (tokenize already ran), but
    // ParseError::from<LexError> exists; we only see ParseError at this stage.
    let ast = parser::parse_tokens(tokens).map_err(|e| {
        vec![Diagnostic::error(e.span, e.message)]
    })?;

    // Phase 3: semantic analysis.
    let analyze_ctx = analyze::AnalyzeContext { known_metric_ids: ctx.known_metric_ids };
    let diags = analyze::analyze(&ast, &analyze_ctx);
    if !diags.is_empty() {
        return Err(diags);
    }

    // Phase 4: extract @metric_refs for upstream cycle detection.
    Ok(analyze::extract_metric_refs(&ast))
}

/// Parse-only entry — returns the AST without running the semantic analyzer.
///
/// Used by the cycle detector (A8) where the caller only needs the extracted
/// `@metric_refs` from a stored expression. The stored expression was already
/// validated at insert time, so re-running the analyzer would be wasteful and
/// could spuriously fail if the known-set changed (e.g. a referenced metric
/// was later deleted — that is a data-integrity issue, not a parse error).
///
/// Returns a single [`Diagnostic`] (not a `Vec`) on failure because lex/parse
/// errors can only produce one root cause.
pub fn parse_only(source: &str) -> Result<Node, Diagnostic> {
    parser::parse(source).map_err(|e| Diagnostic::error(e.span, e.message))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn empty_set() -> HashSet<String> {
        HashSet::new()
    }

    fn make_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    // ── validate_metricql — happy paths ──────────────────────────────────────

    #[test]
    fn happy_aggregation_no_refs() {
        // mean(heartbeat.value) — no @metric_refs → Ok([])
        let ctx = ValidateContext { known_metric_ids: Some(&empty_set()) };
        let refs = validate_metricql("mean(heartbeat.value)", &ctx).unwrap();
        assert!(refs.is_empty(), "expected no refs, got: {refs:?}");
    }

    #[test]
    fn happy_composite_known_refs() {
        // 0.7 * @watch_time + 0.3 * @ctr — both in known set → Ok, sorted
        let set = make_set(&["watch_time", "ctr"]);
        let ctx = ValidateContext { known_metric_ids: Some(&set) };
        let refs = validate_metricql("0.7 * @watch_time + 0.3 * @ctr", &ctx).unwrap();
        assert_eq!(refs, vec!["ctr", "watch_time"], "expected sorted refs");
    }

    #[test]
    fn happy_ratio_known_refs() {
        // ratio(@a, @b) — both in known set → Ok(["a", "b"])
        let set = make_set(&["a", "b"]);
        let ctx = ValidateContext { known_metric_ids: Some(&set) };
        let refs = validate_metricql("ratio(@a, @b)", &ctx).unwrap();
        assert_eq!(refs, vec!["a", "b"]);
    }

    #[test]
    fn happy_none_context_skips_existence_check() {
        // @unknown + @ctr — None context skips existence checks.
        // The expression is a Composite at root (not a bare MetricRef), so it passes
        // the analyzer's top-level rejection gate and succeeds with refs returned.
        let ctx = ValidateContext { known_metric_ids: None };
        let ok_refs = validate_metricql("@unknown + @ctr", &ctx).unwrap();
        let mut sorted = ok_refs.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["ctr", "unknown"]);
    }

    // ── validate_metricql — sad paths ────────────────────────────────────────

    #[test]
    fn sad_lex_error_unterminated_string() {
        // mean(x.y) where p = 'mobile — unterminated string triggers LexError
        let ctx = ValidateContext { known_metric_ids: None };
        let diags = validate_metricql("mean(x.y) where p = 'mobile", &ctx).unwrap_err();
        assert_eq!(diags.len(), 1, "lex error must short-circuit to 1 diagnostic");
        let msg = &diags[0].message;
        assert!(
            msg.contains("unterminated") || msg.contains("string"),
            "expected unterminated/string in message, got: {msg:?}"
        );
    }

    #[test]
    fn sad_parse_error_unclosed_paren() {
        // mean(heartbeat.value — unclosed paren → ParseError
        let ctx = ValidateContext { known_metric_ids: None };
        let diags = validate_metricql("mean(heartbeat.value", &ctx).unwrap_err();
        assert_eq!(diags.len(), 1, "parse error must produce exactly 1 diagnostic");
        let msg = &diags[0].message;
        assert!(msg.contains("')'"), "expected ')' in message, got: {msg:?}");
    }

    #[test]
    fn sad_bare_metric_ref_at_root() {
        // @watch_time alone — bare ref at root → semantic error
        let ctx = ValidateContext { known_metric_ids: None };
        let diags = validate_metricql("@watch_time", &ctx).unwrap_err();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("bare metric reference"),
            "got: {:?}",
            diags[0].message
        );
    }

    #[test]
    fn sad_multi_semantic_error_three_unresolved() {
        // @a + @b + @c — empty known set → 3 diagnostics, one per unresolved ref
        let set = empty_set();
        let ctx = ValidateContext { known_metric_ids: Some(&set) };
        let diags = validate_metricql("@a + @b + @c", &ctx).unwrap_err();
        assert_eq!(
            diags.len(),
            3,
            "expected 3 diagnostics (one per unresolved ref), got: {diags:?}"
        );
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("@a")));
        assert!(messages.iter().any(|m| m.contains("@b")));
        assert!(messages.iter().any(|m| m.contains("@c")));
    }

    #[test]
    fn sad_lex_error_short_circuits_no_semantic_check() {
        // 'unterminated — lex error fires before semantic analysis; only 1 diagnostic.
        let ctx = ValidateContext { known_metric_ids: None };
        let diags = validate_metricql("'unterminated", &ctx).unwrap_err();
        assert_eq!(
            diags.len(),
            1,
            "lex error must short-circuit: only 1 diagnostic expected, got: {diags:?}"
        );
    }

    // ── parse_only ───────────────────────────────────────────────────────────

    #[test]
    fn parse_only_valid_aggregation() {
        // mean(x.y) → Ok(Node::Aggregation)
        let node = parse_only("mean(x.y)").unwrap();
        assert!(
            matches!(node, Node::Aggregation(_)),
            "expected Aggregation variant, got: {node:?}"
        );
    }

    #[test]
    fn parse_only_parse_error_single_diagnostic() {
        // (unclosed — parse error → Err(Diagnostic), NOT Vec
        let err = parse_only("(unclosed").unwrap_err();
        // Verify it is a single Diagnostic (type system guarantees this).
        assert!(
            err.message.contains("')'") || err.message.contains("expected"),
            "got: {:?}",
            err.message
        );
    }

    #[test]
    fn parse_only_lex_error_single_diagnostic() {
        // 'unterminated → lex error forwarded as single Diagnostic
        let err = parse_only("'unterminated").unwrap_err();
        assert!(
            err.message.contains("unterminated") || err.message.contains("string"),
            "got: {:?}",
            err.message
        );
    }

    #[test]
    fn parse_only_no_semantic_enforcement() {
        // @unknown_ref is a bare MetricRef — parse_only must NOT enforce semantic rules.
        // The analyzer would reject this at root; parse_only should return Ok.
        let node = parse_only("@unknown_ref").unwrap();
        assert!(
            matches!(node, Node::MetricRef(_)),
            "parse_only must not run semantic analysis; expected MetricRef, got: {node:?}"
        );
    }
}
