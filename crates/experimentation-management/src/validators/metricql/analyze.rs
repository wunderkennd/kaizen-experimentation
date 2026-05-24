//! MetricQL semantic analyzer — port of
//! `services/metrics/internal/metricql/analyze.go` (ADR-026 Phase 2 / #436).
//!
//! ## Key behavioral divergence from Go
//!
//! The Go `Analyze` function short-circuits on the **first** error
//! (`return &AnalyzeError{}`). This Rust port **accumulates** all diagnostics
//! in a single AST walk and returns them as a `Vec<Diagnostic>`. An empty vec
//! means success. This is load-bearing for UX: operators see every issue at
//! once instead of fixing them one by one.
//!
//! The only early-exit rules are the top-level rejection cases (bare
//! `MetricRef` or `Literal` at root), which mirror Go's `Analyze` entry-point
//! switch and are semantically nonsensical as standalone definitions.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use super::ast::{AggFunc, Aggregation, Filter, MetricRef, Node, Ratio};
use super::diagnostic::Diagnostic;

// ---------------------------------------------------------------------------
// Identifier regex — same pattern as validators::identifier_re() but defined
// locally so this module remains self-contained (identifier_re is private).
// ---------------------------------------------------------------------------

fn identifier_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[a-z_][a-z0-9_]*$")
            .expect("identifier regex is a compile-time constant")
    })
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Context supplied to the semantic analyzer.
///
/// Only metric-ID existence is checked here. An event catalog (for
/// `event_type`/`field` validation) is deferred to a future phase.
pub struct AnalyzeContext<'a> {
    /// Set of metric IDs that exist in M5's store. If `None`, `@metric_ref`
    /// existence checks are skipped — M5 may pass `None` at creation time and
    /// enforce the check only on update.
    pub known_metric_ids: Option<&'a HashSet<String>>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Walk the AST and collect **all** semantic diagnostics in a single pass.
///
/// Returns an empty `Vec` on success.
///
/// Mirrors Go `Analyze(root Node, ctx AnalyzeContext) error` but collects
/// every violation rather than returning on the first one.
pub fn analyze(root: &Node, ctx: &AnalyzeContext) -> Vec<Diagnostic> {
    // Top-level rejection: bare @ref or number is grammar-legal but
    // semantically nonsensical as a standalone metric definition.
    // These are immediate — no point walking further.
    match root {
        Node::MetricRef(m) => {
            return vec![Diagnostic::error(
                m.span.clone(),
                "a metric definition must be an aggregation or arithmetic expression, \
                 not a bare metric reference",
            )];
        }
        Node::Literal(l) => {
            return vec![Diagnostic::error(
                l.span.clone(),
                "a metric definition must be an aggregation or arithmetic expression, \
                 not a bare literal",
            )];
        }
        _ => {}
    }

    let mut diags = Vec::new();
    analyze_node(root, ctx, &mut diags);
    diags
}

/// Walk the AST and return deduplicated `@metric_ref` IDs.
///
/// Mirrors `metricql.CollectMetricRefs` from the Go side. Used by A5 (entry
/// point) and A8 (cycle detection seed). Returns IDs in sorted order for
/// determinism.
pub fn extract_metric_refs(root: &Node) -> Vec<String> {
    let mut seen = HashSet::new();
    collect_refs(root, &mut seen);
    let mut out: Vec<String> = seen.into_iter().collect();
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Internal recursive walker
// ---------------------------------------------------------------------------

fn analyze_node(n: &Node, ctx: &AnalyzeContext, diags: &mut Vec<Diagnostic>) {
    match n {
        Node::Aggregation(a) => analyze_aggregation(a, ctx, diags),
        Node::Composite(c) => {
            analyze_node(&c.left, ctx, diags);
            analyze_node(&c.right, ctx, diags);
        }
        Node::Negate(neg) => analyze_node(&neg.operand, ctx, diags),
        Node::MetricRef(m) => analyze_metric_ref(m, ctx, diags),
        Node::Literal(_) => {
            // Literals carry no analyzer-relevant state; the parser already
            // guarantees they are valid finite f64s.
        }
        Node::Ratio(r) => analyze_ratio(r, ctx, diags),
    }
}

fn analyze_aggregation(a: &Aggregation, _ctx: &AnalyzeContext, diags: &mut Vec<Diagnostic>) {
    // Source.event_type: required, must match identifier regex.
    if !identifier_re().is_match(&a.source.event_type) {
        diags.push(Diagnostic::error(
            a.source.span.clone(),
            format!(
                "invalid event_type {:?}: must match [a-z_][a-z0-9_]*",
                a.source.event_type
            ),
        ));
    }

    // Source.field: optional, must match identifier regex when present.
    if !a.source.field.is_empty() && !identifier_re().is_match(&a.source.field) {
        diags.push(Diagnostic::error(
            a.source.span.clone(),
            format!(
                "invalid field {:?}: must match [a-z_][a-z0-9_]*",
                a.source.field
            ),
        ));
    }

    // Percentile: defense-in-depth — parser already enforces (0, 100).
    if a.func == AggFunc::Percentile && (a.percentile <= 0.0 || a.percentile >= 100.0) {
        diags.push(Diagnostic::error(
            a.span.clone(),
            format!("percentile must be in (0, 100), got {}", a.percentile),
        ));
    }

    // Count / Proportion aggregate over event presence, not a value field.
    // A field argument is meaningless and rejected here.
    // (Mirrors Go's Devin BUG-0001/0002 lock-in from PR #559.)
    if (a.func == AggFunc::Count || a.func == AggFunc::Proportion) && !a.source.field.is_empty() {
        let fn_name = agg_func_name(a.func);
        diags.push(Diagnostic::error(
            a.source.span.clone(),
            format!(
                "{fn_name} aggregation operates on event presence; \
                 remove the .{field} field reference \
                 (e.g., {fn_name}({event_type}) not {fn_name}({event_type}.{field}))",
                fn_name = fn_name,
                field = a.source.field,
                event_type = a.source.event_type,
            ),
        ));
    }

    // Mean / Sum / CountDistinct / Percentile require a value field.
    if matches!(a.func, AggFunc::Mean | AggFunc::Sum | AggFunc::CountDistinct | AggFunc::Percentile)
        && a.source.field.is_empty()
    {
        let fn_name = agg_func_name(a.func);
        diags.push(Diagnostic::error(
            a.source.span.clone(),
            format!(
                "{fn_name} aggregation requires a value field \
                 (e.g., {fn_name}({event_type}.<field>))",
                fn_name = fn_name,
                event_type = a.source.event_type,
            ),
        ));
    }

    // Filter predicates: validate field identifiers.
    if let Some(filter) = &a.filter {
        analyze_filter(filter, diags);
    }

    // Window: u32 type makes n > 0 structurally guaranteed; WindowUnit is a
    // closed enum — both Go defensive checks are unreachable in Rust.
    // No diagnostics emitted here.
    let _ = &a.window; // explicit no-op — acknowledges the field is considered
}

fn analyze_filter(filter: &Filter, diags: &mut Vec<Diagnostic>) {
    for pred in &filter.predicates {
        if !pred.field.namespace.is_empty()
            && !identifier_re().is_match(&pred.field.namespace)
        {
            diags.push(Diagnostic::error(
                pred.span.clone(),
                format!(
                    "invalid field namespace {:?}: must match [a-z_][a-z0-9_]*",
                    pred.field.namespace
                ),
            ));
        }
        if !identifier_re().is_match(&pred.field.name) {
            diags.push(Diagnostic::error(
                pred.span.clone(),
                format!(
                    "invalid field name {:?}: must match [a-z_][a-z0-9_]*",
                    pred.field.name
                ),
            ));
        }
    }
}

fn analyze_ratio(r: &Ratio, ctx: &AnalyzeContext, diags: &mut Vec<Diagnostic>) {
    analyze_metric_ref(&r.numerator, ctx, diags);
    analyze_metric_ref(&r.denominator, ctx, diags);
}

fn analyze_metric_ref(m: &MetricRef, ctx: &AnalyzeContext, diags: &mut Vec<Diagnostic>) {
    if !identifier_re().is_match(&m.id) {
        diags.push(Diagnostic::error(
            m.span.clone(),
            format!(
                "invalid metric reference @{}: must match [a-z_][a-z0-9_]*",
                m.id
            ),
        ));
        // If the id is syntactically invalid, skip the existence check —
        // it can't be in the store anyway.
        return;
    }
    if let Some(known) = ctx.known_metric_ids {
        if !known.contains(&m.id) {
            diags.push(Diagnostic::error(
                m.span.clone(),
                format!(
                    "unknown metric reference @{} (not found in metric definitions store)",
                    m.id
                ),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// extract_metric_refs helper
// ---------------------------------------------------------------------------

fn collect_refs(n: &Node, seen: &mut HashSet<String>) {
    match n {
        Node::Aggregation(_) => {
            // Aggregations have no metric refs by construction.
        }
        Node::Composite(c) => {
            collect_refs(&c.left, seen);
            collect_refs(&c.right, seen);
        }
        Node::Negate(neg) => collect_refs(&neg.operand, seen),
        Node::MetricRef(m) => {
            seen.insert(m.id.clone());
        }
        Node::Literal(_) => {}
        Node::Ratio(r) => {
            seen.insert(r.numerator.id.clone());
            seen.insert(r.denominator.id.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn agg_func_name(f: AggFunc) -> &'static str {
    match f {
        AggFunc::Mean => "mean",
        AggFunc::Sum => "sum",
        AggFunc::Count => "count",
        AggFunc::CountDistinct => "count_distinct",
        AggFunc::Proportion => "proportion",
        AggFunc::Percentile => "percentile",
        AggFunc::Unknown => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::ast::{
        AggFunc, Aggregation, ArithOp, Composite, FieldRef, Filter, Literal, MetricRef, Node,
        Negate, Op, Predicate, Source, Span, Value, ValueKind,
    };
    use super::super::parser::parse;
    use super::{analyze, extract_metric_refs, AnalyzeContext};
    use std::collections::HashSet;

    fn span() -> Span {
        Span::new(0, 1)
    }

    fn ctx_none() -> AnalyzeContext<'static> {
        AnalyzeContext { known_metric_ids: None }
    }

    /// Build a HashSet from a slice of &str. The set must be bound in the
    /// caller's scope so it outlives the AnalyzeContext that borrows it.
    fn make_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    // -------------------------------------------------------------------------
    // Happy path (analyze returns empty Vec)
    // -------------------------------------------------------------------------

    #[test]
    fn valid_mean_aggregation() {
        let node = parse("mean(heartbeat.value)").unwrap();
        let diags = analyze(&node, &ctx_none());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn valid_composite_with_known_refs() {
        let node = parse("0.7 * @watch_time + 0.3 * @ctr").unwrap();
        let set = make_set(&["watch_time", "ctr"]);
        let ctx = AnalyzeContext { known_metric_ids: Some(&set) };
        let diags = analyze(&node, &ctx);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn valid_ratio_with_known_refs() {
        let node = parse("ratio(@logins, @signups)").unwrap();
        let set = make_set(&["logins", "signups"]);
        let ctx = AnalyzeContext { known_metric_ids: Some(&set) };
        let diags = analyze(&node, &ctx);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn valid_count_with_filter() {
        let node = parse("count(login) where success = 'true'").unwrap();
        let diags = analyze(&node, &ctx_none());
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn no_known_metric_ids_context_skips_existence_check() {
        // @unresolved_ref is not in any set — but ctx has no set → no diagnostic.
        let node = parse("@unresolved_ref + @another").unwrap();
        let diags = analyze(&node, &ctx_none());
        assert!(diags.is_empty(), "no-set context must skip existence checks");
    }

    // -------------------------------------------------------------------------
    // Sad path — single diagnostics
    // -------------------------------------------------------------------------

    #[test]
    fn bare_metric_ref_at_root_rejected() {
        let node = parse("@watch_time").unwrap();
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("bare metric reference"),
            "message: {}",
            diags[0].message
        );
    }

    #[test]
    fn bare_literal_at_root_rejected() {
        let node = parse("0.95").unwrap();
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("bare literal"),
            "message: {}",
            diags[0].message
        );
    }

    #[test]
    fn unresolved_single_ref() {
        // @unknown is not in the known set; @ctr is — only 1 diagnostic.
        let node = parse("@unknown + @ctr").unwrap();
        let set = make_set(&["ctr"]);
        let ctx = AnalyzeContext { known_metric_ids: Some(&set) };
        let diags = analyze(&node, &ctx);
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {diags:?}");
        assert!(diags[0].message.contains("@unknown"), "message: {}", diags[0].message);
    }

    #[test]
    fn count_with_field_rejected() {
        // count(login.foo) — field on a count aggregation.
        let node = parse("count(login.foo)").unwrap();
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {diags:?}");
        assert!(
            diags[0].message.contains("operates on event presence"),
            "message: {}",
            diags[0].message
        );
        assert!(diags[0].message.contains("count"), "message: {}", diags[0].message);
    }

    #[test]
    fn mean_without_field_rejected() {
        // mean(heartbeat) — no field on a mean aggregation.
        let node = parse("mean(heartbeat)").unwrap();
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {diags:?}");
        assert!(
            diags[0].message.contains("requires a value field"),
            "message: {}",
            diags[0].message
        );
        assert!(diags[0].message.contains("mean"), "message: {}", diags[0].message);
    }

    #[test]
    fn invalid_event_type_via_constructed_ast() {
        // Cannot round-trip "BadType" through the parser (lexer rejects uppercase).
        // Construct the AST directly to exercise the analyzer's rule.
        let node = Node::Aggregation(Aggregation {
            func: AggFunc::Count,
            percentile: 0.0,
            source: Source {
                event_type: "BadType".to_string(),
                field: String::new(),
                span: span(),
            },
            filter: None,
            window: None,
            span: span(),
        });
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {diags:?}");
        assert!(
            diags[0].message.contains("invalid event_type"),
            "message: {}",
            diags[0].message
        );
    }

    #[test]
    fn invalid_filter_field_namespace_via_constructed_ast() {
        // Construct an AST with an invalid namespace.
        let node = Node::Aggregation(Aggregation {
            func: AggFunc::Count,
            percentile: 0.0,
            source: Source {
                event_type: "login".to_string(),
                field: String::new(),
                span: span(),
            },
            filter: Some(Filter {
                predicates: vec![Predicate {
                    field: FieldRef {
                        namespace: "Bad".to_string(), // uppercase — invalid
                        name: "x".to_string(),
                    },
                    operator: Op::Eq,
                    value: Value {
                        kind: ValueKind::String("y".to_string()),
                        span: span(),
                    },
                    span: span(),
                }],
                span: span(),
            }),
            window: None,
            span: span(),
        });
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {diags:?}");
        assert!(
            diags[0].message.contains("invalid field namespace"),
            "message: {}",
            diags[0].message
        );
    }

    // -------------------------------------------------------------------------
    // Multi-error accumulation (CRITICAL: proves no-short-circuit behavior)
    // -------------------------------------------------------------------------

    #[test]
    fn multiple_unresolved_refs_all_reported() {
        // Three refs, none in known set → 3 diagnostics.
        let node = parse("@unknown1 + @unknown2 * @unknown3").unwrap();
        let set = make_set(&[]); // empty
        let ctx = AnalyzeContext { known_metric_ids: Some(&set) };
        let diags = analyze(&node, &ctx);
        assert_eq!(
            diags.len(),
            3,
            "expected 3 diagnostics (one per unresolved ref), got: {diags:?}"
        );
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("@unknown1")));
        assert!(messages.iter().any(|m| m.contains("@unknown2")));
        assert!(messages.iter().any(|m| m.contains("@unknown3")));
    }

    #[test]
    fn mixed_errors_both_reported() {
        // Composite: @unresolved (not in set) + mean(event) (missing field).
        // Expect 2 diagnostics total.
        let node = Node::Composite(Box::new(Composite {
            op: ArithOp::Add,
            left: Box::new(Node::MetricRef(MetricRef {
                id: "unresolved".to_string(),
                span: span(),
            })),
            right: Box::new(Node::Aggregation(Aggregation {
                func: AggFunc::Mean,
                percentile: 0.0,
                source: Source {
                    event_type: "heartbeat".to_string(),
                    field: String::new(), // missing field → diagnostic
                    span: span(),
                },
                filter: None,
                window: None,
                span: span(),
            })),
            span: span(),
        }));
        let set = make_set(&[]); // empty known set
        let ctx = AnalyzeContext { known_metric_ids: Some(&set) };
        let diags = analyze(&node, &ctx);
        assert_eq!(
            diags.len(),
            2,
            "expected 2 diagnostics (unresolved ref + missing field), got: {diags:?}"
        );
        assert!(diags.iter().any(|d| d.message.contains("@unresolved")));
        assert!(diags.iter().any(|d| d.message.contains("requires a value field")));
    }

    // -------------------------------------------------------------------------
    // extract_metric_refs
    // -------------------------------------------------------------------------

    #[test]
    fn refs_empty_for_pure_aggregation() {
        let node = parse("mean(heartbeat.value)").unwrap();
        let refs = extract_metric_refs(&node);
        assert!(refs.is_empty(), "aggregation has no metric refs: {refs:?}");
    }

    #[test]
    fn refs_composite_two_distinct() {
        let node = parse("0.7 * @a + 0.3 * @b").unwrap();
        let mut refs = extract_metric_refs(&node);
        refs.sort();
        assert_eq!(refs, vec!["a", "b"]);
    }

    #[test]
    fn refs_ratio_both_arms() {
        let node = parse("ratio(@num, @den)").unwrap();
        let mut refs = extract_metric_refs(&node);
        refs.sort();
        assert_eq!(refs, vec!["den", "num"]);
    }

    #[test]
    fn refs_deduplicates() {
        // @a + @a → ["a"] (not ["a", "a"])
        let node = parse("@a + @a").unwrap();
        let refs = extract_metric_refs(&node);
        assert_eq!(refs, vec!["a"]);
    }

    #[test]
    fn refs_nested_arithmetic_multiple() {
        // (@a + @b) * @c - @a → deduplicated, sorted: ["a", "b", "c"]
        let node = parse("(@a + @b) * @c - @a").unwrap();
        let mut refs = extract_metric_refs(&node);
        refs.sort();
        assert_eq!(refs, vec!["a", "b", "c"]);
    }

    #[test]
    fn refs_negate_passthrough() {
        // Construct -@x + @y via AST directly (parser produces Negate wrapping MetricRef).
        let node = Node::Composite(Box::new(Composite {
            op: ArithOp::Add,
            left: Box::new(Node::Negate(Box::new(Negate {
                operand: Node::MetricRef(MetricRef { id: "x".to_string(), span: span() }),
                span: span(),
            }))),
            right: Box::new(Node::MetricRef(MetricRef { id: "y".to_string(), span: span() })),
            span: span(),
        }));
        let mut refs = extract_metric_refs(&node);
        refs.sort();
        assert_eq!(refs, vec!["x", "y"]);
    }

    #[test]
    fn refs_literal_contributes_nothing() {
        let node = Node::Literal(Literal { value: 3.14, span: span() });
        let refs = extract_metric_refs(&node);
        assert!(refs.is_empty());
    }

    // -------------------------------------------------------------------------
    // Additional coverage
    // -------------------------------------------------------------------------

    #[test]
    fn proportion_with_field_rejected() {
        // proportion(event.field) — proportion also forbids a field.
        let node = Node::Aggregation(Aggregation {
            func: AggFunc::Proportion,
            percentile: 0.0,
            source: Source {
                event_type: "purchase".to_string(),
                field: "amount".to_string(),
                span: span(),
            },
            filter: None,
            window: None,
            span: span(),
        });
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("operates on event presence"));
        assert!(diags[0].message.contains("proportion"));
    }

    #[test]
    fn sum_without_field_rejected() {
        let node = Node::Aggregation(Aggregation {
            func: AggFunc::Sum,
            percentile: 0.0,
            source: Source {
                event_type: "revenue".to_string(),
                field: String::new(),
                span: span(),
            },
            filter: None,
            window: None,
            span: span(),
        });
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("requires a value field"));
        assert!(diags[0].message.contains("sum"));
    }

    #[test]
    fn count_distinct_without_field_rejected() {
        let node = Node::Aggregation(Aggregation {
            func: AggFunc::CountDistinct,
            percentile: 0.0,
            source: Source {
                event_type: "purchase".to_string(),
                field: String::new(),
                span: span(),
            },
            filter: None,
            window: None,
            span: span(),
        });
        let diags = analyze(&node, &ctx_none());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("requires a value field"));
        assert!(diags[0].message.contains("count_distinct"));
    }
}
