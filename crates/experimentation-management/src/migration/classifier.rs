//! AST-based Tier classifier for ADR-026 Phase 3 — Task A3.
//!
//! This module parses CUSTOM metric SQL with `sqlparser-rs` (using
//! `GenericDialect`, which covers the Spark SQL subset used by the platform)
//! and extracts a **shape hint** that downstream translators (A4 Tier 1, A5
//! Tier 2) use to decide which translation path to attempt.
//!
//! ## Design decisions
//!
//! * **`GenericDialect`** is used instead of a Spark-specific dialect because
//!   sqlparser 0.50 does not ship a `SparkDialect`.  `GenericDialect` parses
//!   all constructs present in the corpus (`INTERVAL N HOURS`, `NULLIF`, CTEs,
//!   window functions) correctly.
//!
//! * **`parse_or_tier3`** returns `Result<Statement, String>` rather than
//!   `ClassificationResult` because this module owns parsing only.  Tier
//!   resolution lives in A4/A5; keeping the parse result separate lets those
//!   translators inspect the full AST without re-parsing.
//!
//! * **`extract_shape`** is deliberately conservative.  It only returns a
//!   `Some(ShapeHint)` variant when it is confident enough to direct
//!   downstream work.  Everything ambiguous falls through to `None` so the
//!   caller (mod.rs) can emit `Tier3Untranslatable`.  (L4 / L8 locks.)

use sqlparser::ast::{
    BinaryOperator, Expr, FunctionArguments, SelectItem, SetExpr, SetOperator, Statement,
    TableFactor,
};
use sqlparser::dialect::DatabricksDialect;
use sqlparser::parser::Parser;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// High-level structural hints for downstream Tier 1 / Tier 2 translators.
///
/// Variants are ordered from most-to-least structured.  The classifier returns
/// the *most-structured* match it can confidently produce (L4).
///
/// This enum is `non_exhaustive` to allow future variants without a breaking
/// change to match arms in A4/A5.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShapeHint {
    /// `SELECT <agg_func>(<col>) FROM <events> WHERE <pred> [GROUP BY ...]`
    ///
    /// The most common Tier 1 shape; also covers simple Tier 2 cases that A4
    /// will further distinguish.  The WHERE clause does **not** contain an
    /// INTERVAL window predicate; those are `WindowedAggregation`.
    FilteredAggregation,

    /// Like `FilteredAggregation` but the JOIN condition contains a time-window
    /// predicate (`event_timestamp < exposure_ts + INTERVAL N HOURS`).
    ///
    /// Used by the WINDOWED_COUNT translator (A4).
    WindowedAggregation,

    /// Projection is an arithmetic expression over `MAX(CASE WHEN metric_id =
    /// '...' THEN metric_value END)` subexpressions — i.e. metric-summary
    /// pivoting.  Covers both COMPOSITE (Tier 1) and METRICQL arithmetic
    /// (Tier 2); the distinction is made in A4/A5.
    CompositeArithmetic,

    /// `SELECT SUM(num) / SUM(denom) FROM events WHERE ...`
    ///
    /// Distinct from `CompositeArithmetic` because the operands are direct
    /// aggregate functions on raw events rather than metric-summary pivots.
    RatioOfSums,
}

// ---------------------------------------------------------------------------
// Parse entry point
// ---------------------------------------------------------------------------

/// Parse `sql` and return the first top-level `Statement`, or an error
/// string if parsing fails or the input produces no statements (e.g.
/// comments-only input).
///
/// ## Dialect choice
///
/// `DatabricksDialect` is used because:
/// - The platform uses Delta Lake / Databricks Spark SQL.
/// - `sqlparser` 0.50 ships no `SparkDialect`; `DatabricksDialect` is the
///   closest available and correctly handles `INTERVAL '48' HOUR`, `NULLIF`,
///   CTEs, window functions, and the other constructs in the corpus.
/// - Note: unquoted `INTERVAL 48 HOURS` (no quotes, plural unit) is a
///   Databricks/Spark extension that no available dialect in v0.50 parses.
///   Such SQL will return `Err` and classify as Tier3 with a parse error,
///   which is the correct conservative behavior (L8).
///
/// ## Return value
///
/// * `Ok(Statement)` — caller can inspect the AST and call `extract_shape`.
/// * `Err(reason)` — the caller should build a
///   `ClassificationResult::Tier3Untranslatable { reason: format!("SQL parse
///   failed: {e}"), parse_error: Some(e) }`.
///
/// ## Notes
///
/// `sqlparser::Parser::parse_sql` accepts a multi-statement string.  We only
/// care about the first statement; if there are multiple, we conservatively
/// return the first and let `extract_shape` handle the rest.  (In practice
/// our CUSTOM metric SQLs are always single statements.)
pub fn parse_or_tier3(sql: &str) -> Result<Statement, String> {
    let dialect = DatabricksDialect {};
    let mut stmts =
        Parser::parse_sql(&dialect, sql).map_err(|e| format!("{e}"))?;

    // parse_sql succeeds on empty input but returns an empty vec.
    if stmts.is_empty() {
        return Err("SQL parsed to zero statements (comments-only or empty input)".into());
    }

    Ok(stmts.remove(0))
}

// ---------------------------------------------------------------------------
// Shape extractor
// ---------------------------------------------------------------------------

/// Extract a structural hint from a parsed `Statement`.
///
/// Returns `None` when no recognisable shape was found — the caller should
/// emit `Tier3Untranslatable` in that case.
///
/// This function does NOT classify into tiers; that is the responsibility of
/// A4 (Tier 1) and A5 (Tier 2).
pub fn extract_shape(stmt: &Statement) -> Option<ShapeHint> {
    let query = match stmt {
        Statement::Query(q) => q,
        _ => return None, // DDL, DML, etc.
    };

    // Recursive CTEs → no recognized shape (let A4/A5 never see them).
    // Non-recursive CTEs *could* be translatable but we conservatively skip
    // them for now (none appear in the Tier 1/2 corpus shapes).
    if query.with.is_some() {
        return None;
    }

    // UNION / EXCEPT / INTERSECT → no recognized shape.
    match query.body.as_ref() {
        SetExpr::SetOperation { op, .. } if *op == SetOperator::Union => return None,
        SetExpr::SetOperation { .. } => return None,
        _ => {}
    }

    let select = match query.body.as_ref() {
        SetExpr::Select(s) => s.as_ref(),
        _ => return None,
    };

    // LATERAL VIEWs → reject.
    if !select.lateral_views.is_empty() {
        return None;
    }

    // Subquery anywhere in WHERE → reject (conservative: L8).
    if let Some(ref where_expr) = select.selection {
        if contains_subquery(where_expr) {
            return None;
        }
    }

    // Window functions in projection → reject.
    for item in &select.projection {
        if projection_item_has_window_fn(item) {
            return None;
        }
    }

    // Non-deterministic functions (RAND, RANDOM, UUID, …) → reject.
    if projection_contains_nondeterministic(&select.projection) {
        return None;
    }

    // -----------------------------------------------------------------------
    // Shape detection — order matters: most-specific first.
    // -----------------------------------------------------------------------

    // CompositeArithmetic: projection is an arithmetic expression whose leaf
    // operands are `MAX(CASE WHEN metric_id = '...' THEN metric_value END)`
    // pivot subexpressions.  The table is metric_summaries.
    if is_metric_summary_table(&select.from) && projection_is_composite_arithmetic(&select.projection) {
        return Some(ShapeHint::CompositeArithmetic);
    }

    // WindowedAggregation: COUNT-based with INTERVAL window in JOIN ON clause.
    // The INTERVAL predicate lives in the JOIN condition, not WHERE.
    if projection_has_count_cast(&select.projection) && from_has_interval_join(&select.from) {
        return Some(ShapeHint::WindowedAggregation);
    }

    // RatioOfSums: SELECT SUM(num) / SUM(denom) FROM events WHERE ...
    // Must be checked BEFORE FilteredAggregation because SUM is also a plain
    // aggregate — FilteredAggregation would match first otherwise.
    if projection_is_ratio_of_sums(&select.projection) {
        return Some(ShapeHint::RatioOfSums);
    }

    // FilteredAggregation: AVG / COUNT / SUM / MAX / MIN over an events table
    // with optional WHERE and GROUP BY.  Covers both direct Tier 1 candidates
    // and Tier 2 candidates that A4/A5 will further distinguish.
    if projection_has_simple_aggregate(&select.projection) {
        return Some(ShapeHint::FilteredAggregation);
    }

    None
}

// ---------------------------------------------------------------------------
// Helper predicates — all private
// ---------------------------------------------------------------------------

/// True if the FROM clause references only `delta.metric_summaries` (possibly
/// aliased), indicating a metric-pivot query.
fn is_metric_summary_table(from: &[sqlparser::ast::TableWithJoins]) -> bool {
    if from.len() != 1 {
        return false;
    }
    let name = table_name_from_factor(&from[0].relation);
    name.map(|n| n.to_ascii_lowercase().contains("metric_summaries"))
        .unwrap_or(false)
}

/// Extract the base name from a simple `TableFactor::Table`.
fn table_name_from_factor(factor: &TableFactor) -> Option<String> {
    match factor {
        TableFactor::Table { name, .. } => Some(name.to_string()),
        _ => None,
    }
}

/// True if any projection item contains a `Function` with a non-null `over`
/// (window function).
fn projection_item_has_window_fn(item: &SelectItem) -> bool {
    let expr = match item {
        SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
        _ => return false,
    };
    expr_contains_window_fn(expr)
}

fn expr_contains_window_fn(expr: &Expr) -> bool {
    match expr {
        Expr::Function(f) => f.over.is_some() || fn_args_contain_window(f),
        Expr::BinaryOp { left, right, .. } => {
            expr_contains_window_fn(left) || expr_contains_window_fn(right)
        }
        Expr::UnaryOp { expr, .. } => expr_contains_window_fn(expr),
        Expr::Nested(inner) => expr_contains_window_fn(inner),
        Expr::Cast { expr, .. } => expr_contains_window_fn(expr),
        _ => false,
    }
}

fn fn_args_contain_window(f: &sqlparser::ast::Function) -> bool {
    match &f.args {
        FunctionArguments::List(list) => list.args.iter().any(|a| {
            if let sqlparser::ast::FunctionArg::Unnamed(sqlparser::ast::FunctionArgExpr::Expr(e)) =
                a
            {
                expr_contains_window_fn(e)
            } else {
                false
            }
        }),
        _ => false,
    }
}

/// True if the WHERE expression (or any sub-expression) contains a scalar
/// subquery (`IN (SELECT ...)` or `EXISTS`).
fn contains_subquery(expr: &Expr) -> bool {
    match expr {
        Expr::Subquery(_) => true,
        Expr::InSubquery { .. } => true,
        Expr::Exists { .. } => true,
        Expr::BinaryOp { left, right, .. } => {
            contains_subquery(left) || contains_subquery(right)
        }
        Expr::UnaryOp { expr, .. } => contains_subquery(expr),
        Expr::Nested(inner) => contains_subquery(inner),
        Expr::Between {
            expr, low, high, ..
        } => contains_subquery(expr) || contains_subquery(low) || contains_subquery(high),
        Expr::InList { expr, list, .. } => {
            contains_subquery(expr) || list.iter().any(contains_subquery)
        }
        Expr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => {
            operand.as_ref().map(|e| contains_subquery(e)).unwrap_or(false)
                || conditions.iter().any(contains_subquery)
                || results.iter().any(contains_subquery)
                || else_result.as_ref().map(|e| contains_subquery(e)).unwrap_or(false)
        }
        _ => false,
    }
}

/// True if any projection item contains a non-deterministic function call
/// (RAND, RANDOM, NOW, UUID, CURRENT_TIMESTAMP).
fn projection_contains_nondeterministic(items: &[SelectItem]) -> bool {
    const NONDETERMINISTIC: &[&str] = &["rand", "random", "now", "uuid", "current_timestamp"];
    items.iter().any(|item| {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => return false,
        };
        expr_contains_fn_named(expr, NONDETERMINISTIC)
    })
}

fn expr_contains_fn_named(expr: &Expr, names: &[&str]) -> bool {
    match expr {
        Expr::Function(f) => {
            let n = f.name.to_string().to_ascii_lowercase();
            if names.iter().any(|&target| n == target) {
                return true;
            }
            fn_args_any(f, |e| expr_contains_fn_named(e, names))
        }
        Expr::BinaryOp { left, right, .. } => {
            expr_contains_fn_named(left, names) || expr_contains_fn_named(right, names)
        }
        Expr::UnaryOp { expr, .. } => expr_contains_fn_named(expr, names),
        Expr::Nested(inner) => expr_contains_fn_named(inner, names),
        Expr::Cast { expr, .. } => expr_contains_fn_named(expr, names),
        _ => false,
    }
}

fn fn_args_any<F>(f: &sqlparser::ast::Function, pred: F) -> bool
where
    F: Fn(&Expr) -> bool,
{
    match &f.args {
        FunctionArguments::List(list) => list.args.iter().any(|a| {
            if let sqlparser::ast::FunctionArg::Unnamed(sqlparser::ast::FunctionArgExpr::Expr(e)) =
                a
            {
                pred(e)
            } else {
                false
            }
        }),
        _ => false,
    }
}

/// True if at least one projection item is (or wraps) a simple aggregate
/// function (AVG, COUNT, SUM, MIN, MAX) without a window clause.
fn projection_has_simple_aggregate(items: &[SelectItem]) -> bool {
    const AGGREGATES: &[&str] = &["avg", "count", "sum", "min", "max"];
    items.iter().any(|item| {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => return false,
        };
        expr_contains_plain_aggregate(expr, AGGREGATES)
    })
}

fn expr_contains_plain_aggregate(expr: &Expr, aggregates: &[&str]) -> bool {
    match expr {
        Expr::Function(f) => {
            if f.over.is_some() {
                return false; // window function — rejected elsewhere
            }
            let n = f.name.to_string().to_ascii_lowercase();
            aggregates.iter().any(|&a| n == a)
        }
        Expr::Cast { expr, .. } => expr_contains_plain_aggregate(expr, aggregates),
        Expr::Nested(inner) => expr_contains_plain_aggregate(inner, aggregates),
        Expr::BinaryOp { left, right, .. } => {
            expr_contains_plain_aggregate(left, aggregates)
                || expr_contains_plain_aggregate(right, aggregates)
        }
        _ => false,
    }
}

/// True if the single projection item (after stripping CAST) is a COUNT(*)
/// or COUNT(<col>) without a window clause.
fn projection_has_count_cast(items: &[SelectItem]) -> bool {
    items.iter().any(|item| {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => return false,
        };
        is_count_expr(expr)
    })
}

fn is_count_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Function(f) => {
            f.over.is_none() && f.name.to_string().eq_ignore_ascii_case("count")
        }
        Expr::Cast { expr, .. } => is_count_expr(expr),
        Expr::Nested(inner) => is_count_expr(inner),
        _ => false,
    }
}

/// True if any JOIN in the FROM clause has an ON condition containing an
/// `INTERVAL` literal (i.e. a time-window predicate).
fn from_has_interval_join(from: &[sqlparser::ast::TableWithJoins]) -> bool {
    use sqlparser::ast::{JoinConstraint, JoinOperator};
    from.iter().any(|twj| {
        twj.joins.iter().any(|join| {
            let constraint = match &join.join_operator {
                JoinOperator::Inner(c)
                | JoinOperator::LeftOuter(c)
                | JoinOperator::RightOuter(c)
                | JoinOperator::FullOuter(c)
                | JoinOperator::LeftSemi(c)
                | JoinOperator::RightSemi(c)
                | JoinOperator::LeftAnti(c)
                | JoinOperator::RightAnti(c) => c,
                _ => return false,
            };
            if let JoinConstraint::On(ref on_expr) = constraint {
                on_expr_contains_interval(on_expr)
            } else {
                false
            }
        })
    })
}

/// Detect an INTERVAL literal anywhere in a JOIN ON expression.
fn on_expr_contains_interval(expr: &Expr) -> bool {
    match expr {
        Expr::Interval(_) => true,
        Expr::BinaryOp { left, right, .. } => {
            on_expr_contains_interval(left) || on_expr_contains_interval(right)
        }
        Expr::UnaryOp { expr, .. } => on_expr_contains_interval(expr),
        Expr::Nested(inner) => on_expr_contains_interval(inner),
        Expr::Between {
            expr, low, high, ..
        } => {
            on_expr_contains_interval(expr)
                || on_expr_contains_interval(low)
                || on_expr_contains_interval(high)
        }
        _ => false,
    }
}

/// True if the projection contains an arithmetic expression whose leaf
/// operands are `MAX(CASE WHEN metric_id = '...' THEN metric_value END)`.
/// This pattern identifies metric-summary pivot queries.
fn projection_is_composite_arithmetic(items: &[SelectItem]) -> bool {
    // Find a non-wildcard, non-identifier projection item that contains
    // a CASE expression whose condition is `metric_id = <literal>`.
    items.iter().any(|item| {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => return false,
        };
        // We look for a MAX(CASE WHEN ...) or arithmetic over such expressions.
        expr_contains_metric_pivot(expr)
    })
}

fn expr_contains_metric_pivot(expr: &Expr) -> bool {
    match expr {
        Expr::Function(f) => {
            let name = f.name.to_string().to_ascii_lowercase();
            if name == "max" || name == "min" {
                // Check if the argument is a CASE expression.
                return fn_args_any(f, |arg| matches!(arg, Expr::Case { .. }));
            }
            if name == "nullif" {
                return fn_args_any(f, expr_contains_metric_pivot);
            }
            false
        }
        Expr::BinaryOp { left, right, op } => {
            // Arithmetic operators: +, -, *, /
            matches!(
                op,
                BinaryOperator::Plus
                    | BinaryOperator::Minus
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
            ) && (expr_contains_metric_pivot(left) || expr_contains_metric_pivot(right))
        }
        Expr::Nested(inner) => expr_contains_metric_pivot(inner),
        Expr::Cast { expr, .. } => expr_contains_metric_pivot(expr),
        _ => false,
    }
}

/// True if the *sole* meaningful projection item is `SUM(a) / SUM(b)`.
fn projection_is_ratio_of_sums(items: &[SelectItem]) -> bool {
    let agg_items: Vec<&Expr> = items
        .iter()
        .filter_map(|item| match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => Some(e),
            _ => None,
        })
        .filter(|e| !matches!(e, Expr::Identifier(_) | Expr::CompoundIdentifier(_)))
        .collect();

    agg_items.iter().any(|e| is_sum_divide_sum(e))
}

fn is_sum_divide_sum(expr: &Expr) -> bool {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Divide,
            right,
        } => is_sum_agg(left) && is_sum_agg(right),
        Expr::Nested(inner) => is_sum_divide_sum(inner),
        _ => false,
    }
}

fn is_sum_agg(expr: &Expr) -> bool {
    match expr {
        Expr::Function(f) => {
            f.over.is_none() && f.name.to_string().eq_ignore_ascii_case("sum")
        }
        Expr::Nested(inner) => is_sum_agg(inner),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Step 1 RED cases — mandated by the spec
    // -----------------------------------------------------------------------

    #[test]
    fn parse_or_tier3_valid_simple_select() {
        let result = parse_or_tier3("SELECT 1");
        assert!(
            result.is_ok(),
            "SELECT 1 must parse successfully; got: {result:?}"
        );
        match result.unwrap() {
            Statement::Query(_) => {}
            other => panic!("expected Statement::Query, got: {other:?}"),
        }
    }

    #[test]
    fn parse_or_tier3_malformed_sql_returns_err() {
        let result = parse_or_tier3("SELECT * FRO bad");
        assert!(
            result.is_err(),
            "malformed SQL must return Err; got Ok"
        );
        let reason = result.unwrap_err();
        // Error string must be non-empty (the caller will prepend "SQL parse failed:").
        assert!(
            !reason.is_empty(),
            "error reason must be non-empty"
        );
    }

    #[test]
    fn extract_shape_filtered_aggregation_count_from_events() {
        // Spec: parse_or_tier3("SELECT COUNT(*) FROM events WHERE event_type = 'foo'")
        //       → parsed, shape-classifier returns ShapeHint::FilteredAggregation
        let sql = "SELECT COUNT(*) FROM events WHERE event_type = 'foo'";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint,
            Some(ShapeHint::FilteredAggregation),
            "expected FilteredAggregation for COUNT(*) FROM events WHERE ...; got {hint:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional shape-detection cases
    // -----------------------------------------------------------------------

    #[test]
    fn extract_shape_filtered_aggregation_avg() {
        // A canonical FILTERED_MEAN corpus query (fm_mobile_watch_time).
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'video_play' AND me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(hint, Some(ShapeHint::FilteredAggregation));
    }

    #[test]
    fn extract_shape_windowed_aggregation_interval() {
        // A WINDOWED_COUNT-style query using INTERVAL '48' HOUR (quoted, singular
        // unit) which is the ANSI-compatible form that DatabricksDialect parses.
        //
        // Note: The corpus uses `INTERVAL 48 HOURS` (unquoted, plural) which is
        // a Databricks/Spark extension not supported by any dialect in sqlparser
        // 0.50.  Those corpus entries correctly fall to Tier3 parse error.
        // This test exercises the `WindowedAggregation` detection path using the
        // equivalent quoted syntax.
        let sql = "SELECT eu.user_id, eu.variant_id, CAST(COUNT(me.user_id) AS DOUBLE) AS metric_value \
                   FROM delta.exposures eu \
                   LEFT JOIN delta.metric_events me \
                     ON eu.user_id = me.user_id \
                     AND me.event_type = 'purchase_completed' \
                     AND me.event_timestamp >= eu.exposure_ts \
                     AND me.event_timestamp < eu.exposure_ts + INTERVAL '48' HOUR \
                   GROUP BY eu.user_id, eu.variant_id";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint,
            Some(ShapeHint::WindowedAggregation),
            "expected WindowedAggregation for INTERVAL-windowed COUNT; got {hint:?}"
        );
    }

    #[test]
    fn extract_shape_composite_arithmetic_metric_pivot() {
        // A COMPOSITE corpus query (composite_add_two_metrics).
        let sql = "SELECT ms.user_id, ms.variant_id, \
                     (MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                      + MAX(CASE WHEN ms.metric_id = 'session_count_metric' THEN ms.metric_value END)) \
                     AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.experiment_id = '{{ExperimentID}}' \
                     AND ms.metric_id IN ('watch_time_metric', 'session_count_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint,
            Some(ShapeHint::CompositeArithmetic),
            "expected CompositeArithmetic for MAX(CASE...) pivot; got {hint:?}"
        );
    }

    #[test]
    fn extract_shape_composite_weighted_sum() {
        // Weighted-sum COMPOSITE query (composite_weighted_sum_engagement).
        let sql = "SELECT ms.user_id, ms.variant_id, \
                     (0.7 * MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                      + 0.3 * MAX(CASE WHEN ms.metric_id = 'ctr_metric' THEN ms.metric_value END)) \
                     AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.experiment_id = '{{ExperimentID}}' \
                     AND ms.metric_id IN ('watch_time_metric', 'ctr_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint,
            Some(ShapeHint::CompositeArithmetic),
            "expected CompositeArithmetic for weighted-sum pivot; got {hint:?}"
        );
    }

    #[test]
    fn extract_shape_ratio_of_sums() {
        let sql = "SELECT SUM(revenue) / SUM(sessions) AS arpu FROM events WHERE experiment_id = 'exp1'";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint,
            Some(ShapeHint::RatioOfSums),
            "expected RatioOfSums for SUM/SUM; got {hint:?}"
        );
    }

    #[test]
    fn extract_shape_window_function_returns_none() {
        // Window functions (ROW_NUMBER OVER) must not match any shape.
        let sql = "SELECT user_id, \
                     ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY event_timestamp DESC) AS rn, \
                     duration_ms \
                   FROM delta.metric_events WHERE event_type = 'video_play'";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint, None,
            "window functions must produce no shape hint; got {hint:?}"
        );
    }

    #[test]
    fn extract_shape_union_all_returns_none() {
        let sql = "SELECT user_id, COUNT(*) AS metric_value FROM delta.metric_events \
                   WHERE event_type = 'play_start' GROUP BY user_id \
                   UNION ALL \
                   SELECT user_id, COUNT(*) AS metric_value FROM delta.metric_events \
                   WHERE event_type = 'purchase_completed' GROUP BY user_id";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint, None,
            "UNION ALL must produce no shape hint; got {hint:?}"
        );
    }

    #[test]
    fn extract_shape_recursive_cte_returns_none() {
        let sql = "WITH RECURSIVE user_chain AS (\
                     SELECT user_id, 0 AS depth FROM delta.metric_events WHERE event_type = 'referral' \
                     UNION ALL \
                     SELECT me.user_id, uc.depth + 1 \
                     FROM delta.metric_events me \
                     INNER JOIN user_chain uc ON me.referrer_user_id = uc.user_id\
                   ) \
                   SELECT user_id, MAX(depth) AS max_depth FROM user_chain GROUP BY user_id";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let hint = extract_shape(&stmt);
        assert_eq!(
            hint, None,
            "recursive CTE must produce no shape hint; got {hint:?}"
        );
    }

    #[test]
    fn parse_or_tier3_comments_only_returns_err() {
        // Comments-only SQL → parse_sql returns zero statements.
        let sql = "-- This metric was computed manually\n-- TODO: fix";
        let result = parse_or_tier3(sql);
        assert!(
            result.is_err(),
            "comments-only SQL must return Err; got Ok"
        );
    }

    #[test]
    fn parse_or_tier3_select_1_is_query_statement() {
        // Confirm the returned Statement is the Query variant.
        let stmt = parse_or_tier3("SELECT 1").unwrap();
        assert!(matches!(stmt, Statement::Query(_)));
    }
}
