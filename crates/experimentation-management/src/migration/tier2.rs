//! Tier 2 translator for ADR-026 Phase 3 — Task A5.
//!
//! Converts a parsed SQL `Statement` (which Tier 1 rejected) into a MetricQL
//! expression string for the `METRICQL` metric type.
//!
//! ## Shapes handled
//!
//! | Shape hint            | MetricQL output                                      |
//! |-----------------------|------------------------------------------------------|
//! | `CompositeArithmetic` | Arithmetic over `@metric_ref` nodes                  |
//! | `RatioOfSums`         | `ratio(@n, @d)` if both can be expressed as @-refs   |
//! | `FilteredAggregation` | `mean/count/sum(event.field) where <pred>`           |
//! | `WindowedAggregation` | `count(event) where <pred> within N hours of exposure`|
//!
//! ## Round-trip validation (L8)
//!
//! After synthesizing the candidate MetricQL string:
//!   1. `validate_metricql(&expr, &ctx)` — synchronous parse + semantic check
//!      with `known_metric_ids: None` (existence check is deferred to the
//!      full `validate_metric_definition` round-trip below).
//!   2. `validate_metric_definition(&candidate, lookup).await` — async M5
//!      validator; catches existence + cycle issues (mirrors A4's pattern,
//!      prevents the PR-#567 bug class).
//!
//! Returns `None` if either check fails (conservative L4/L8 — fall through
//! to Tier 3).

use sqlparser::ast::{
    BinaryOperator, Expr, FunctionArguments, SelectItem, Statement,
};

use experimentation_proto::experimentation::common::v1::{MetricDefinition, MetricType};

use crate::validators::{
    metricql::{validate_metricql, ValidateContext},
    validate_metric_definition, MetricLookup,
};

use super::classifier::ShapeHint;
use super::tier1::{
    collect_and_predicates, extract_select, find_join_on_expr,
    from_is_events_with_optional_exposures_join, from_is_exposures_with_metric_events_join,
    strip_table_alias, unwrap_nested,
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A successful Tier 2 translation proposal.
pub struct Tier2Proposal {
    pub metric: MetricDefinition,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Attempt to translate `stmt` (Tier 1 rejected) into a METRICQL
/// `MetricDefinition`.
///
/// Returns `Some(proposal)` if and only if:
///   1. The AST matches a known Tier 2 pattern.
///   2. The synthesized MetricQL string passes `validate_metricql`.
///   3. The full `MetricDefinition` passes `validate_metric_definition`.
///
/// Returns `None` in all other cases — caller falls through to Tier 3.
pub async fn translate<L>(
    stmt: &Statement,
    shape: ShapeHint,
    original: &MetricDefinition,
    lookup: &L,
) -> Option<Tier2Proposal>
where
    L: MetricLookup + ?Sized,
{
    let (metricql_expr, reason) = match shape {
        ShapeHint::CompositeArithmetic => {
            let expr = translate_composite_arithmetic(stmt)?;
            (expr, "CompositeArithmetic → @-ref arithmetic".to_string())
        }
        ShapeHint::RatioOfSums => {
            let expr = translate_ratio_of_sums(stmt)?;
            (expr, "RatioOfSums → ratio(@n, @d)".to_string())
        }
        ShapeHint::FilteredAggregation => {
            let expr = translate_filtered_aggregation(stmt)?;
            (expr, "FilteredAggregation → MetricQL aggregation".to_string())
        }
        ShapeHint::WindowedAggregation => {
            let expr = translate_windowed_aggregation(stmt)?;
            (expr, "WindowedAggregation → MetricQL windowed count".to_string())
        }
    };

    // Round-trip 1: MetricQL parse + semantic analysis (existence check skipped
    // — `None` context allows forward-references; the full M5 validator below
    // handles existence via the lookup).
    let ctx = ValidateContext { known_metric_ids: None };
    if validate_metricql(&metricql_expr, &ctx).is_err() {
        return None;
    }

    // Build the candidate MetricDefinition.
    let candidate = build_metricql_metric(original, metricql_expr);

    // Round-trip 2: full M5 validate_metric_definition (PR-#567 guard).
    if validate_metric_definition(&candidate, lookup).await.is_err() {
        return None;
    }

    Some(Tier2Proposal {
        metric: candidate,
        reason,
    })
}

// ---------------------------------------------------------------------------
// CompositeArithmetic → MetricQL arithmetic over @metric_ref
// ---------------------------------------------------------------------------
//
// Tier 1 already handles COMPOSITE shapes where the operands map to a single
// CompositeOperator and all operands exist in the lookup. Tier 2 picks up:
//   - Expressions that Tier 1 rejected because the lookup didn't have the IDs
//   - Complex arithmetic (e.g. 3-operand weighted sum, subtract, multiply,
//     divide) where the expression isn't a clean Tier 1 CompositeOperator
//
// We emit the arithmetic expression in MetricQL's infix notation:
//   MAX(CASE WHEN metric_id = 'x' THEN metric_value END) → @x
//   0.7 * @a + 0.3 * @b (literal * @ref or @ref * literal)
//   @a - @b, @a * @b, @a / @b, (@a + @b) / @c, etc.

fn translate_composite_arithmetic(stmt: &Statement) -> Option<String> {
    let select = extract_select(stmt)?;
    let expr = find_metric_value_expr_t2(&select.projection)?;
    let expr = unwrap_nested(expr);
    emit_metricql_expr(expr)
}

/// Find the projection item aliased as `metric_value`, or the last
/// non-trivial expression.
fn find_metric_value_expr_t2(projection: &[SelectItem]) -> Option<&Expr> {
    for item in projection {
        if let SelectItem::ExprWithAlias { expr, alias } = item {
            if alias.value.eq_ignore_ascii_case("metric_value") {
                return Some(expr);
            }
        }
    }
    for item in projection.iter().rev() {
        match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => {
                if !matches!(e, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
                    return Some(e);
                }
            }
            _ => continue,
        }
    }
    None
}

/// Recursively emit a MetricQL arithmetic expression from the SQL AST.
///
/// `MAX(CASE WHEN metric_id = 'x' THEN metric_value END)` → `@x`
/// `0.7 * <pivot>` → `0.7 * @x`
/// `<pivot> + <pivot>` → `@a + @b`
/// `<pivot> / NULLIF(<pivot>, 0)` → `@a / @b`
///
/// Returns `None` if the expression contains anything we can't express
/// in MetricQL (subqueries, function calls other than pivot pattern, etc.).
fn emit_metricql_expr(expr: &Expr) -> Option<String> {
    let expr = unwrap_nested(expr);

    // Try pivot extraction first: MAX(CASE WHEN metric_id = 'x' THEN metric_value END)
    if let Some(metric_id) = extract_pivot_metric_id_t2(expr) {
        return Some(format!("@{metric_id}"));
    }

    // Number literal (used in weighted sums)
    if let Some(n) = extract_number_literal_t2(expr) {
        return Some(format_number(n));
    }

    // Binary arithmetic
    if let Expr::BinaryOp { left, op, right } = expr {
        let l_expr = unwrap_nested(left);
        let r_expr = unwrap_nested(right);

        // Handle A / NULLIF(B, 0) → A / B
        let (actual_left, actual_right, actual_op) =
            if let BinaryOperator::Divide = op {
                let unwrapped_right = unwrap_nullif(r_expr);
                (l_expr, unwrapped_right, op)
            } else {
                (l_expr, r_expr, op)
            };

        let op_str = match actual_op {
            BinaryOperator::Plus => "+",
            BinaryOperator::Minus => "-",
            BinaryOperator::Multiply => "*",
            BinaryOperator::Divide => "/",
            _ => return None,
        };

        let left_str = emit_metricql_expr(actual_left)?;
        let right_str = emit_metricql_expr(actual_right)?;

        // Check if we need parentheses — for nested expressions involving
        // lower-precedence operators wrapped in higher-precedence context.
        // Emit parens around `+` or `-` sub-expressions when they appear as
        // EITHER operand of `*` or `/`. Devin PR #577 round-1 🔴 finding: the
        // previous version checked only the right operand, so an input like
        // `(@a + @b) * @c` lost its parens via `unwrap_nested(left)` and was
        // emitted as `@a + @b * @c` — same syntax, different arithmetic. The
        // round-trip validate_metricql check at the proposal boundary did NOT
        // catch this because the wrong output is syntactically valid; only L3
        // shadow-run would notice the value drift downstream. Static check
        // closes the gap before the proposal ever leaves the translator.
        let inner_is_additive = |e: &Expr| {
            matches!(
                e,
                Expr::BinaryOp { op: BinaryOperator::Plus, .. }
                    | Expr::BinaryOp { op: BinaryOperator::Minus, .. }
            )
        };
        let is_mul_or_div = matches!(actual_op, BinaryOperator::Multiply | BinaryOperator::Divide);
        let needs_left_parens = is_mul_or_div && inner_is_additive(actual_left);
        let needs_right_parens = is_mul_or_div && inner_is_additive(actual_right);

        let left_out = if needs_left_parens {
            format!("({left_str})")
        } else {
            left_str
        };
        let right_out = if needs_right_parens {
            format!("({right_str})")
        } else {
            right_str
        };

        return Some(format!("{left_out} {op_str} {right_out}"));
    }

    None
}

/// Unwrap `NULLIF(<expr>, 0)` → `<expr>` (for DIVIDE pivot pattern).
fn unwrap_nullif(expr: &Expr) -> &Expr {
    let expr = unwrap_nested(expr);
    if let Expr::Function(f) = expr {
        if f.name.to_string().eq_ignore_ascii_case("nullif") {
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(inner),
                )) = list.args.first()
                {
                    return unwrap_nested(inner);
                }
            }
        }
    }
    expr
}

/// Extract `metric_id` from `MAX(CASE WHEN alias.metric_id = 'x' THEN alias.metric_value END)`
/// or `MIN(CASE ...)`.
fn extract_pivot_metric_id_t2(expr: &Expr) -> Option<String> {
    let expr = unwrap_nested(expr);
    if let Expr::Function(f) = expr {
        let fname = f.name.to_string().to_ascii_lowercase();
        if fname == "max" || fname == "min" {
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(inner),
                )) = list.args.first()
                {
                    return extract_case_metric_id_t2(inner);
                }
            }
        }
        if fname == "nullif" {
            // NULLIF(MAX(...), 0) → unwrap to MAX(...)
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(inner),
                )) = list.args.first()
                {
                    return extract_pivot_metric_id_t2(inner);
                }
            }
        }
    }
    None
}

fn extract_case_metric_id_t2(expr: &Expr) -> Option<String> {
    if let Expr::Case { operand: None, conditions, .. } = expr {
        if let Some(cond) = conditions.first() {
            return extract_field_eq_literal(cond, "metric_id");
        }
    }
    None
}

/// `alias.field = 'literal'` → `Some("literal")` when field name (stripped of alias) == col.
fn extract_field_eq_literal(expr: &Expr, col: &str) -> Option<String> {
    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::Eq,
        right,
    } = expr
    {
        let col_name = strip_table_alias(left)?;
        if col_name == col {
            if let Expr::Value(sqlparser::ast::Value::SingleQuotedString(s)) = right.as_ref() {
                return Some(s.clone());
            }
        }
    }
    None
}

fn extract_number_literal_t2(expr: &Expr) -> Option<f64> {
    if let Expr::Value(sqlparser::ast::Value::Number(n, _)) = expr {
        return n.parse::<f64>().ok();
    }
    None
}

/// Format a float: if it has no fractional part and fits in i64, emit as integer.
/// Otherwise use up to 10 decimal places (stripping trailing zeros).
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        // Emit enough precision, strip trailing zeros after decimal point.
        let s = format!("{:.10}", n);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// RatioOfSums → MetricQL ratio(@n, @d)
// ---------------------------------------------------------------------------
//
// Pattern: SELECT SUM(num_col) / SUM(denom_col) FROM events WHERE ...
//
// Conservative (L4): we only emit `ratio(@n, @d)` if both SUM arguments can
// be expressed as @-ref metric IDs (i.e., are pivot expressions referencing
// metric_summaries). If the SUM args are plain column references (not pivot
// patterns), we return None → Tier 3 (raw aggregate can't be expressed as
// ratio(@n, @d) — those are @-refs to stored metric definitions, not raw fields).

fn translate_ratio_of_sums(stmt: &Statement) -> Option<String> {
    let select = extract_select(stmt)?;

    // Find the SUM/SUM expression in projection.
    let ratio_expr = find_sum_div_sum_expr(&select.projection)?;
    let ratio_expr = unwrap_nested(ratio_expr);

    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::Divide,
        right,
    } = ratio_expr
    {
        let num_id = extract_sum_pivot_id(unwrap_nested(left))?;
        let den_id = extract_sum_pivot_id(unwrap_nested(right))?;
        return Some(format!("ratio(@{num_id}, @{den_id})"));
    }

    None
}

fn find_sum_div_sum_expr(projection: &[SelectItem]) -> Option<&Expr> {
    for item in projection {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => continue,
        };
        if is_sum_div_sum(unwrap_nested(expr)) {
            return Some(expr);
        }
    }
    None
}

fn is_sum_div_sum(expr: &Expr) -> bool {
    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::Divide,
        right,
    } = expr
    {
        is_sum_fn(unwrap_nested(left)) && is_sum_fn(unwrap_nested(right))
    } else {
        false
    }
}

fn is_sum_fn(expr: &Expr) -> bool {
    if let Expr::Function(f) = expr {
        f.name.to_string().eq_ignore_ascii_case("sum")
    } else {
        false
    }
}

/// Extract a metric ID from inside `SUM(MAX(CASE WHEN metric_id = 'x' ...))`.
/// If the SUM arg is a pivot expression, extract the ID. Otherwise return None
/// (raw field reference — can't be a @-ref).
fn extract_sum_pivot_id(expr: &Expr) -> Option<String> {
    if let Expr::Function(f) = expr {
        if f.name.to_string().eq_ignore_ascii_case("sum") {
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(inner),
                )) = list.args.first()
                {
                    return extract_pivot_metric_id_t2(unwrap_nested(inner));
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// FilteredAggregation → MetricQL aggregation expression
// ---------------------------------------------------------------------------
//
// Handles shapes that Tier 1 rejected:
//   - COUNT(*) aggregate → `count(<event_type>)` (no .field on count)
//   - AVG(<alias>.<field>) → `mean(<event_type>.<field>)`
//   - CAST(MAX(CASE WHEN event_type = 'X' THEN 1 ELSE 0 END) AS DOUBLE) → `proportion(X)`
//   - Filter predicates that Tier 1's allowlist rejected (IN lists, OR, etc.)
//
// Emits: `<agg>(source) [where <predicate>]`
//
// The MetricQL grammar's filter uses `and` (lowercase) not `AND`.
// IN lists use `[...]` not `(...)` in MetricQL.

fn translate_filtered_aggregation(stmt: &Statement) -> Option<String> {
    let select = extract_select(stmt)?;

    // Guard: FROM clause must be `metric_events [JOIN exposures]`.
    // Queries with additional joins (e.g. user_profiles, content_catalog) bring
    // predicates from non-events tables that we cannot safely serialize as
    // MetricQL filter predicates — that would silently drop the table context
    // (PR-#567 bug class). Reject conservatively (L4/L8).
    if !from_is_events_with_optional_exposures_join(&select.from) {
        return None;
    }

    // Extract the aggregation source and function from the projection.
    let AggInfo { func_name, source } = extract_agg_info(&select.projection)?;

    // Proportion is special: the event_type lives inside the CASE expression (source),
    // not in the WHERE clause. The WHERE contains only the filter predicates.
    // Must be handled BEFORE the count/proportion source-not-empty guard below.
    if func_name == "proportion" {
        // `source` holds the event_type extracted from the CASE expression.
        // Devin PR #577 round-1 🚩 finding: if the WHERE clause ALSO has an
        // event_type predicate, we must either strip it (redundant — same
        // value as the CASE) or reject (contradictory — different value).
        // The count/mean/sum path below already does this via
        // split_where_for_metricql; mirror that hygiene for proportion so
        // future corpus entries can't trigger the latent bug class.
        let filter_str = if let Some(where_expr) = select.selection.as_ref() {
            let preds = collect_and_predicates(where_expr);
            let mut filter_preds: Vec<&Expr> = Vec::new();
            for pred in &preds {
                if let Some(et) = extract_event_type_val(pred) {
                    if et != source {
                        // Contradictory: CASE says event_type = source, WHERE
                        // says event_type = something_else. The two filters
                        // would never produce rows together; reject to Tier 3
                        // rather than emit semantically-broken MetricQL.
                        return None;
                    }
                    // Redundant: WHERE event_type = source, same as CASE.
                    // Strip so the emitted filter doesn't repeat the constraint.
                    continue;
                }
                filter_preds.push(pred);
            }
            if filter_preds.is_empty() {
                None
            } else {
                let parts: Option<Vec<String>> = filter_preds
                    .iter()
                    .map(|p| serialize_metricql_predicate(p))
                    .collect();
                Some(parts?.join(" and "))
            }
        } else {
            None
        };
        return match filter_str {
            Some(f) => Some(format!("proportion({source}) where {f}")),
            None => Some(format!("proportion({source})")),
        };
    }

    // For count: source must be empty (MetricQL rejects count(event.field)).
    if func_name == "count" && !source.is_empty() {
        return None;
    }

    // For mean/count/sum: extract event_type and remaining predicates from WHERE.
    let where_expr = select.selection.as_ref()?;
    let (event_type, remaining_preds) = split_where_for_metricql(where_expr)?;

    // Build source string: `event_type` or `event_type.field`
    let source_str = if source.is_empty() {
        event_type.clone()
    } else {
        format!("{event_type}.{source}")
    };

    // Build MetricQL filter string from remaining predicates.
    let filter_str = if remaining_preds.is_empty() {
        None
    } else {
        let parts: Option<Vec<String>> = remaining_preds
            .iter()
            .map(|p| serialize_metricql_predicate(p))
            .collect();
        Some(parts?.join(" and "))
    };

    let expr = match filter_str {
        Some(f) => format!("{func_name}({source_str}) where {f}"),
        None => format!("{func_name}({source_str})"),
    };

    Some(expr)
}

// (build_filter_str_from_where_preds removed in PR #577 round-1 🚩 fix.
// Its only caller was the proportion path, which now inlines the per-predicate
// loop to also strip redundant event_type predicates. The count/mean/sum
// path uses split_where_for_metricql + serialize_metricql_predicate directly.)

struct AggInfo {
    func_name: String,
    /// Empty string for functions that don't take a field (count, proportion).
    source: String,
}

/// Extract the aggregation function and field from the projection.
///
/// Handles:
///   - `AVG(alias.col)` → `mean`, `col`
///   - `COUNT(*)` or `COUNT(alias.col)` → `count`, ``
///   - `CAST(MAX(CASE WHEN event_type = 'X' THEN 1 ELSE 0 END) AS DOUBLE)` → `proportion`, ``
fn extract_agg_info(projection: &[SelectItem]) -> Option<AggInfo> {
    for item in projection {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => continue,
        };
        if let Some(info) = try_extract_agg(expr) {
            return Some(info);
        }
    }
    None
}

fn try_extract_agg(expr: &Expr) -> Option<AggInfo> {
    let inner = unwrap_nested(expr);

    // AVG(alias.col)
    if let Expr::Function(f) = inner {
        let fname = f.name.to_string().to_ascii_lowercase();
        match fname.as_str() {
            "avg" => {
                let col = extract_fn_single_col(f)?;
                return Some(AggInfo { func_name: "mean".to_string(), source: col });
            }
            "count" => {
                // count(*) → no field; count(col) → no field for MetricQL
                return Some(AggInfo { func_name: "count".to_string(), source: String::new() });
            }
            "sum" => {
                let col = extract_fn_single_col(f)?;
                return Some(AggInfo { func_name: "sum".to_string(), source: col });
            }
            _ => {}
        }
    }

    // CAST(<inner> AS DOUBLE) — unwrap CAST and recurse
    if let Expr::Cast { expr: inner_cast, .. } = inner {
        // CAST(MAX(CASE WHEN event_type = 'X' THEN 1 ELSE 0 END) AS DOUBLE)
        // → proportion(X)
        if let Some(prop_event) = try_extract_proportion(unwrap_nested(inner_cast)) {
            return Some(AggInfo {
                func_name: "proportion".to_string(),
                source: prop_event,
            });
        }
        // Also try regular aggregate inside CAST
        return try_extract_agg(inner_cast);
    }

    None
}

/// Try to extract the event_type from:
/// `MAX(CASE WHEN <alias>.event_type = 'X' THEN 1 ELSE 0 END)` or
/// `MAX(CASE WHEN event_type = 'X' THEN 1 ELSE 0 END)`
fn try_extract_proportion(expr: &Expr) -> Option<String> {
    let expr = unwrap_nested(expr);
    if let Expr::Function(f) = expr {
        let fname = f.name.to_string().to_ascii_lowercase();
        if fname == "max" || fname == "min" {
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(case_expr),
                )) = list.args.first()
                {
                    return extract_proportion_event_type(case_expr);
                }
            }
        }
    }
    None
}

/// From `CASE WHEN event_type = 'X' THEN 1 ELSE 0 END`, extract 'X'.
fn extract_proportion_event_type(expr: &Expr) -> Option<String> {
    if let Expr::Case { operand: None, conditions, results, else_result: Some(_), } = expr {
        // Must be exactly: WHEN event_type = 'X' THEN 1 ELSE 0
        if conditions.len() != 1 {
            return None;
        }
        // Result must be a literal 1
        if let Some(result) = results.first() {
            if !matches!(result, Expr::Value(sqlparser::ast::Value::Number(n, _)) if n == "1") {
                return None;
            }
        }
        // Condition must be event_type = 'X'
        let cond = &conditions[0];
        return extract_field_eq_literal(cond, "event_type");
    }
    None
}

/// Extract the single column argument from a function call like `AVG(alias.col)`.
fn extract_fn_single_col(f: &sqlparser::ast::Function) -> Option<String> {
    if let FunctionArguments::List(list) = &f.args {
        if let Some(sqlparser::ast::FunctionArg::Unnamed(
            sqlparser::ast::FunctionArgExpr::Expr(inner),
        )) = list.args.first()
        {
            return strip_table_alias(inner);
        }
        // COUNT(*) wildcard
        if let Some(sqlparser::ast::FunctionArg::Unnamed(
            sqlparser::ast::FunctionArgExpr::Wildcard,
        )) = list.args.first()
        {
            return Some(String::new());
        }
        // COUNT with empty list
        if list.args.is_empty() {
            return Some(String::new());
        }
    }
    None
}

/// Split WHERE into event_type + remaining predicates (same as tier1, but also
/// returns the event_type for use as the MetricQL source field).
fn split_where_for_metricql<'a>(
    expr: &'a Expr,
) -> Option<(String, Vec<&'a Expr>)> {
    let preds = collect_and_predicates(expr);
    let mut event_type = None;
    let mut remaining: Vec<&'a Expr> = Vec::new();

    for pred in &preds {
        if let Some(et) = extract_event_type_val(pred) {
            if event_type.is_none() {
                event_type = Some(et);
                continue;
            }
        }
        remaining.push(pred);
    }

    Some((event_type?, remaining))
}

fn extract_event_type_val(expr: &Expr) -> Option<String> {
    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::Eq,
        right,
    } = expr
    {
        let col = strip_table_alias(left)?;
        if col == "event_type" {
            if let Expr::Value(sqlparser::ast::Value::SingleQuotedString(s)) = right.as_ref() {
                return Some(s.clone());
            }
        }
    }
    None
}

/// Serialize a SQL predicate expression into MetricQL filter syntax.
///
/// MetricQL uses:
///   - `field = 'value'`, `field != 'value'`, `field < N`, etc.
///   - `field in ['a', 'b', 'c']`   (square brackets, not parentheses)
///   - `and` (lowercase) — caller joins with " and "
///
/// Returns `None` for unserializable predicates (LIKE, BETWEEN, subqueries,
/// function calls) — conservative L4/L8.
fn serialize_metricql_predicate(expr: &Expr) -> Option<String> {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let op_str = match op {
                BinaryOperator::Eq => "=",
                BinaryOperator::NotEq => "!=",
                BinaryOperator::Lt => "<",
                BinaryOperator::LtEq => "<=",
                BinaryOperator::Gt => ">",
                BinaryOperator::GtEq => ">=",
                // AND within a predicate: recurse
                BinaryOperator::And => {
                    let l = serialize_metricql_predicate(left)?;
                    let r = serialize_metricql_predicate(right)?;
                    return Some(format!("{l} and {r}"));
                }
                // MetricQL filter grammar is strictly `predicate ('and' predicate)*` —
                // OR is not in the grammar. Return None to bail to Tier 3
                // conservatively (L4/L8). The round-trip guard (validate_metricql)
                // would catch any emitted `or` anyway, but being explicit here
                // makes the intent clear and avoids misleading callers.
                BinaryOperator::Or => return None,
                _ => return None,
            };
            let l = serialize_metricql_simple(left)?;
            let r = serialize_metricql_simple(right)?;
            Some(format!("{l} {op_str} {r}"))
        }
        Expr::InList { expr, list, negated } => {
            let col = serialize_metricql_simple(expr)?;
            let items: Option<Vec<String>> = list.iter().map(serialize_metricql_simple).collect();
            let items_str = items?.join(", ");
            if *negated {
                Some(format!("{col} not in [{items_str}]"))
            } else {
                Some(format!("{col} in [{items_str}]"))
            }
        }
        Expr::Nested(inner) => serialize_metricql_predicate(inner),
        _ => None,
    }
}

fn serialize_metricql_simple(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Identifier(id) => Some(id.value.to_ascii_lowercase()),
        Expr::CompoundIdentifier(parts) => {
            Some(parts.last()?.value.to_ascii_lowercase())
        }
        Expr::Value(v) => match v {
            sqlparser::ast::Value::SingleQuotedString(s) => Some(format!("'{s}'")),
            sqlparser::ast::Value::Number(n, _) => Some(n.clone()),
            sqlparser::ast::Value::Null => Some("null".to_string()),
            _ => None,
        },
        Expr::Nested(inner) => serialize_metricql_simple(inner),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// WindowedAggregation → MetricQL count(...) where ... within N hours of exposure
// ---------------------------------------------------------------------------
//
// Handles cases where Tier 1 rejected the query because:
//   - The filter predicates go beyond Tier 1's allowlist (e.g. content_type = 'movie')
//
// Emits: `count(<event_type>) [where <filter>] within N hours of exposure`

fn translate_windowed_aggregation(stmt: &Statement) -> Option<String> {
    let select = extract_select(stmt)?;

    // Validate FROM: must be exposures LEFT JOIN metric_events.
    if !from_is_exposures_with_metric_events_join(&select.from) {
        return None;
    }

    let join_on = find_join_on_expr(&select.from)?;
    let predicates = collect_and_predicates(join_on);

    // Extract event_type.
    let event_type = extract_event_type_from_predicates_t2(&predicates)?;

    // Extract window_hours from INTERVAL predicate.
    let window_hours = extract_window_hours_t2(&predicates)?;

    // Remaining predicates (not user_id join, not event_type, not timestamp).
    let filter_preds: Vec<&Expr> = predicates
        .iter()
        .copied()
        .filter(|p| {
            !is_user_id_join_pred(p)
                && !is_event_type_pred(p)
                && !is_timestamp_pred(p)
        })
        .collect();

    let filter_str = if filter_preds.is_empty() {
        None
    } else {
        let parts: Option<Vec<String>> =
            filter_preds.iter().map(|p| serialize_metricql_predicate(p)).collect();
        Some(parts?.join(" and "))
    };

    // Determine window unit: use "hours" for multiples of 24 that suggest days,
    // but stick to hours for precision. The corpus uses hours throughout.
    let window_expr = format!("within {window_hours} hours of exposure");

    let expr = match filter_str {
        Some(f) => format!("count({event_type}) where {f} {window_expr}"),
        None => format!("count({event_type}) {window_expr}"),
    };

    Some(expr)
}

fn extract_event_type_from_predicates_t2(predicates: &[&Expr]) -> Option<String> {
    for pred in predicates {
        if let Some(et) = extract_field_eq_literal(pred, "event_type") {
            return Some(et);
        }
    }
    None
}

fn extract_window_hours_t2(predicates: &[&Expr]) -> Option<i32> {
    for pred in predicates {
        if let Expr::BinaryOp {
            left,
            op: BinaryOperator::Lt,
            right,
        } = pred
        {
            if !expr_mentions_col(left, "event_timestamp") {
                continue;
            }
            if let Some(h) = extract_interval_hours_from_add(right) {
                return Some(h);
            }
        }
    }
    None
}

fn extract_interval_hours_from_add(expr: &Expr) -> Option<i32> {
    if let Expr::BinaryOp {
        op: BinaryOperator::Plus,
        right,
        ..
    } = expr
    {
        return extract_interval_hours_val(right);
    }
    None
}

fn extract_interval_hours_val(expr: &Expr) -> Option<i32> {
    use sqlparser::ast::DateTimeField;
    if let Expr::Interval(interval) = expr {
        if matches!(interval.leading_field, Some(DateTimeField::Hour)) {
            if let Expr::Value(sqlparser::ast::Value::SingleQuotedString(s)) =
                interval.value.as_ref()
            {
                return s.parse::<i32>().ok();
            }
            if let Expr::Value(sqlparser::ast::Value::Number(n, _)) = interval.value.as_ref() {
                return n.parse::<i32>().ok();
            }
        }
    }
    None
}

fn expr_mentions_col(expr: &Expr, col: &str) -> bool {
    match expr {
        Expr::Identifier(id) => id.value == col,
        Expr::CompoundIdentifier(parts) => {
            parts.last().map(|p| p.value == col).unwrap_or(false)
        }
        Expr::BinaryOp { left, right, .. } => {
            expr_mentions_col(left, col) || expr_mentions_col(right, col)
        }
        Expr::UnaryOp { expr, .. } => expr_mentions_col(expr, col),
        Expr::Nested(inner) => expr_mentions_col(inner, col),
        _ => false,
    }
}

fn is_user_id_join_pred(pred: &Expr) -> bool {
    if let Expr::BinaryOp { left, op: BinaryOperator::Eq, right } = pred {
        let l = strip_table_alias(left);
        let r = strip_table_alias(right);
        matches!((l.as_deref(), r.as_deref()), (Some("user_id"), Some("user_id")))
    } else {
        false
    }
}

fn is_event_type_pred(pred: &Expr) -> bool {
    extract_field_eq_literal(pred, "event_type").is_some()
}

fn is_timestamp_pred(pred: &Expr) -> bool {
    expr_mentions_col(pred, "event_timestamp") || expr_mentions_col(pred, "exposure_ts")
}

// ---------------------------------------------------------------------------
// Proto builder
// ---------------------------------------------------------------------------

fn build_metricql_metric(original: &MetricDefinition, metricql_expression: String) -> MetricDefinition {
    MetricDefinition {
        metric_id: format!("{}-migrated", original.metric_id),
        name: original.name.clone(),
        description: original.description.clone(),
        lower_is_better: original.lower_is_better,
        surrogate_target_metric_id: original.surrogate_target_metric_id.clone(),
        is_qoe_metric: original.is_qoe_metric,
        cuped_covariate_metric_id: original.cuped_covariate_metric_id.clone(),
        minimum_detectable_effect: original.minimum_detectable_effect,
        stakeholder: original.stakeholder,
        aggregation_level: original.aggregation_level,
        r#type: MetricType::Metricql as i32,
        metricql_expression,
        custom_sql: String::new(),
        type_config: None,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::classifier::{extract_shape, parse_or_tier3};
    use crate::migration::test_support::{EmptyLookup, SeedLookup};

    fn custom_original(metric_id: &str) -> MetricDefinition {
        MetricDefinition {
            metric_id: metric_id.to_string(),
            name: "Test Metric".to_string(),
            r#type: experimentation_proto::experimentation::common::v1::MetricType::Custom as i32,
            ..Default::default()
        }
    }

    fn parse_and_classify(sql: &str) -> (Statement, ShapeHint) {
        let stmt = parse_or_tier3(sql).expect("must parse");
        let shape = extract_shape(&stmt).expect("must classify");
        (stmt, shape)
    }

    // -----------------------------------------------------------------------
    // CompositeArithmetic → @-ref arithmetic
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn composite_arith_subtract_two_refs() {
        // metricql_subtract_two_metrics fixture
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (MAX(CASE WHEN ms.metric_id = 'revenue_metric' THEN ms.metric_value END) \
                    - MAX(CASE WHEN ms.metric_id = 'cost_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('revenue_metric', 'cost_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("subtract_two");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&["revenue_metric", "cost_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(proposal.metric.r#type, MetricType::Metricql as i32);
        assert_eq!(proposal.metric.metricql_expression, "@revenue_metric - @cost_metric");
    }

    #[tokio::test]
    async fn composite_arith_multiply_two_refs() {
        // metricql_multiply_metric_refs fixture
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (MAX(CASE WHEN ms.metric_id = 'ctr_metric' THEN ms.metric_value END) \
                    * MAX(CASE WHEN ms.metric_id = 'avg_order_value_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('ctr_metric', 'avg_order_value_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("multiply_two");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&["ctr_metric", "avg_order_value_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(proposal.metric.metricql_expression, "@ctr_metric * @avg_order_value_metric");
    }

    /// Devin PR #577 round-1 🔴 regression: `(@a + @b) * @c` must NOT emit
    /// `@a + @b * @c`. The original bug only checked the RIGHT operand for
    /// additive sub-expressions needing parens; the left operand was emitted
    /// raw, silently changing arithmetic semantics. Round-trip validate
    /// didn't catch it because both expressions are syntactically valid.
    #[tokio::test]
    async fn composite_arith_left_additive_under_multiply_gets_parens() {
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   ((MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                     + MAX(CASE WHEN ms.metric_id = 'session_count_metric' THEN ms.metric_value END)) \
                    * MAX(CASE WHEN ms.metric_id = 'ctr_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('watch_time_metric', 'session_count_metric', 'ctr_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("left_additive_under_mul");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&["watch_time_metric", "session_count_metric", "ctr_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        // The critical assertion: the left-side (additive) operand MUST be
        // parenthesized. The pre-fix output was `@watch_time_metric +
        // @session_count_metric * @ctr_metric` — same tokens, different math.
        assert_eq!(
            proposal.metric.metricql_expression,
            "(@watch_time_metric + @session_count_metric) * @ctr_metric"
        );
    }

    /// Devin PR #577 round-1 🚩 regression: proportion path now strips
    /// redundant event_type from WHERE clause (mirrors count/mean/sum path).
    #[tokio::test]
    async fn proportion_strips_redundant_event_type_from_where() {
        // The CASE inside proportion picks event_type = 'purchase_completed'.
        // WHERE redundantly mentions the same event_type. Output should NOT
        // include it in the `where` clause.
        let sql = "SELECT me.user_id, \
                   CAST(MAX(CASE WHEN me.event_type = 'purchase_completed' THEN 1 ELSE 0 END) AS DOUBLE) \
                       AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'purchase_completed' AND me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = custom_original("proportion_redundant_event_type");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&[]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(
            proposal.metric.metricql_expression,
            "proportion(purchase_completed) where platform = 'mobile'"
        );
    }

    /// Devin PR #577 round-1 🚩 regression: proportion path rejects
    /// contradictory event_type (CASE = X, WHERE = Y) as Tier 3 rather than
    /// emitting MetricQL whose filter would never match a row.
    #[tokio::test]
    async fn proportion_rejects_contradictory_event_type_in_where() {
        let sql = "SELECT me.user_id, \
                   CAST(MAX(CASE WHEN me.event_type = 'purchase_completed' THEN 1 ELSE 0 END) AS DOUBLE) \
                       AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'page_view' \
                   GROUP BY me.user_id";
        let original = custom_original("proportion_contradictory_event_type");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&[]);
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(
            result.is_none(),
            "expected Tier 3 rejection on contradictory event_type, got Some(proposal with MetricQL: {:?})",
            result.as_ref().map(|p| &p.metric.metricql_expression)
        );
    }

    #[tokio::test]
    async fn composite_arith_three_ref_add() {
        // metricql_three_ref_combination fixture
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                    + MAX(CASE WHEN ms.metric_id = 'session_count_metric' THEN ms.metric_value END) \
                    + MAX(CASE WHEN ms.metric_id = 'ctr_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('watch_time_metric', 'session_count_metric', 'ctr_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("three_ref_add");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&["watch_time_metric", "session_count_metric", "ctr_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(
            proposal.metric.metricql_expression,
            "@watch_time_metric + @session_count_metric + @ctr_metric"
        );
    }

    #[tokio::test]
    async fn composite_arith_weighted_three_ref() {
        // metricql_weighted_engagement_score fixture (3 operands → Tier1 rejects? Actually
        // Tier1 accepts 3-operand WEIGHTED_SUM if lookup is empty — validate_composite fails)
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (0.5 * MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                    + 0.3 * MAX(CASE WHEN ms.metric_id = 'ctr_metric' THEN ms.metric_value END) \
                    + 0.2 * MAX(CASE WHEN ms.metric_id = 'completion_rate_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('watch_time_metric', 'ctr_metric', 'completion_rate_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("weighted_three");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&["watch_time_metric", "ctr_metric", "completion_rate_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(
            proposal.metric.metricql_expression,
            "0.5 * @watch_time_metric + 0.3 * @ctr_metric + 0.2 * @completion_rate_metric"
        );
    }

    // -----------------------------------------------------------------------
    // FilteredAggregation → mean/count with where clause
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn filtered_agg_mean_range_predicate() {
        // metricql_filtered_mean_with_percentile_filter fixture
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'video_play' \
                   AND me.duration_ms > 30000 AND me.duration_ms < 7200000 \
                   GROUP BY me.user_id";
        let original = custom_original("mean_range");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(
            proposal.metric.metricql_expression,
            "mean(video_play.duration_ms) where duration_ms > 30000 and duration_ms < 7200000"
        );
    }

    #[tokio::test]
    async fn filtered_agg_count_in_list() {
        // metricql_count_with_in_list_filter fixture
        let sql = "SELECT me.user_id, COUNT(*) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'search_query' \
                   AND me.country IN ('us', 'gb', 'ca', 'au') \
                   GROUP BY me.user_id";
        let original = custom_original("count_in_list");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(
            proposal.metric.metricql_expression,
            "count(search_query) where country in ['us', 'gb', 'ca', 'au']"
        );
    }

    #[tokio::test]
    async fn filtered_agg_proportion_with_filter() {
        // metricql_proportion_with_filter fixture
        let sql = "SELECT me.user_id, CAST(MAX(CASE WHEN me.event_type = 'purchase_completed' \
                   THEN 1 ELSE 0 END) AS DOUBLE) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = custom_original("proportion_mobile");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(
            proposal.metric.metricql_expression,
            "proportion(purchase_completed) where platform = 'mobile'"
        );
    }

    // -----------------------------------------------------------------------
    // WindowedAggregation → count with filter + within N hours
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn windowed_agg_with_content_type_filter() {
        // metricql_windowed_count_filtered fixture
        let sql = "SELECT eu.user_id, eu.variant_id, CAST(COUNT(me.user_id) AS DOUBLE) AS metric_value \
                   FROM delta.exposures eu \
                   LEFT JOIN delta.metric_events me \
                     ON eu.user_id = me.user_id \
                     AND me.event_type = 'content_start' \
                     AND me.event_timestamp >= eu.exposure_ts \
                     AND me.event_timestamp < eu.exposure_ts + INTERVAL '72' HOUR \
                     AND me.content_type = 'movie' \
                   GROUP BY eu.user_id, eu.variant_id";
        let original = custom_original("windowed_content_type");
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(
            proposal.metric.metricql_expression,
            "count(content_start) where content_type = 'movie' within 72 hours of exposure"
        );
    }

    // -----------------------------------------------------------------------
    // Metadata copy
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn metadata_copied_from_original() {
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (MAX(CASE WHEN ms.metric_id = 'revenue_metric' THEN ms.metric_value END) \
                    - MAX(CASE WHEN ms.metric_id = 'cost_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('revenue_metric', 'cost_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = MetricDefinition {
            metric_id: "profit_metric".to_string(),
            name: "Profit".to_string(),
            description: "Revenue minus cost".to_string(),
            lower_is_better: false,
            minimum_detectable_effect: 0.05,
            r#type: experimentation_proto::experimentation::common::v1::MetricType::Custom as i32,
            ..Default::default()
        };
        let (stmt, shape) = parse_and_classify(sql);
        let lookup = SeedLookup::with_ids(&["revenue_metric", "cost_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(proposal.metric.metric_id, "profit_metric-migrated");
        assert_eq!(proposal.metric.name, "Profit");
        assert_eq!(proposal.metric.description, "Revenue minus cost");
        assert!(!proposal.metric.lower_is_better);
        assert!((proposal.metric.minimum_detectable_effect - 0.05).abs() < 1e-12);
        assert!(proposal.metric.custom_sql.is_empty());
        assert!(proposal.metric.type_config.is_none());
        assert_eq!(proposal.metric.r#type, MetricType::Metricql as i32);
    }

    // -----------------------------------------------------------------------
    // Round-trip validation rejection test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn invalid_metricql_returns_none() {
        // A CompositeArithmetic that would produce invalid MetricQL (empty metric_id).
        // We test via a shape that produces something the MetricQL validator rejects.
        // In practice, we drive through a SQL that emits a bad event_type (uppercase).
        // Instead, test directly by verifying an aggregation-based SQL with
        // no event_type filter falls through gracefully.
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   WHERE me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = custom_original("no_event_type");
        let stmt = parse_or_tier3(sql).expect("must parse");
        let shape = extract_shape(&stmt);
        // If FilteredAggregation shape, translate must return None (no event_type).
        if let Some(s) = shape {
            let result = translate(&stmt, s, &original, &EmptyLookup).await;
            // Should be None (no event_type → split_where_for_metricql returns None).
            assert!(result.is_none(), "no event_type must return None from tier2");
        }
    }

    // -----------------------------------------------------------------------
    // FIX 3: RatioOfSums → ratio(@n, @d)
    //
    // Classifier yields RatioOfSums when the projection is `SUM(x) / SUM(y)`.
    // When those SUM arguments are pivot expressions (MAX(CASE WHEN metric_id =
    // '...' THEN metric_value END)), Tier 2 can extract the @-ref IDs and emit
    // `ratio(@n, @d)`.
    //
    // Tier 1 never handles RatioOfSums (mod.rs only routes FilteredAggregation,
    // WindowedAggregation, and CompositeArithmetic to Tier 1). The classifier
    // does NOT fire CompositeArithmetic for this SQL because:
    //   - CompositeArithmetic requires `is_metric_summary_table &&
    //     projection_is_composite_arithmetic`.
    //   - `projection_is_composite_arithmetic` recurses via
    //     `expr_contains_metric_pivot`, which returns true only for MAX/MIN
    //     directly wrapping CASE — not for SUM wrapping MAX wrapping CASE.
    //   - So the outer SUM is opaque to the composite-arithmetic detector, and
    //     RatioOfSums fires instead (it only checks the divide pattern).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn ratio_of_sums_translates_to_metricql_ratio() {
        // SUM(MAX(CASE WHEN metric_id = 'n' THEN metric_value END)) /
        // SUM(MAX(CASE WHEN metric_id = 'd' THEN metric_value END))
        // → ratio(@numerator_metric, @denominator_metric)
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   SUM(MAX(CASE WHEN ms.metric_id = 'numerator_metric' THEN ms.metric_value END)) \
                   / SUM(MAX(CASE WHEN ms.metric_id = 'denominator_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('numerator_metric', 'denominator_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("ratio_of_sums_test");
        let stmt = parse_or_tier3(sql).expect("must parse");
        let shape = extract_shape(&stmt);

        // Confirm classifier yields RatioOfSums (not CompositeArithmetic).
        assert_eq!(
            shape,
            Some(ShapeHint::RatioOfSums),
            "expected RatioOfSums shape for SUM(pivot)/SUM(pivot)"
        );

        let lookup = SeedLookup::with_ids(&["numerator_metric", "denominator_metric"]);
        let proposal = translate(&stmt, ShapeHint::RatioOfSums, &original, &lookup)
            .await
            .expect("RatioOfSums with pivot SUM args must produce a proposal");

        assert_eq!(proposal.metric.r#type, MetricType::Metricql as i32);
        assert_eq!(
            proposal.metric.metricql_expression,
            "ratio(@numerator_metric, @denominator_metric)"
        );
        assert_eq!(proposal.reason, "RatioOfSums → ratio(@n, @d)");
    }

    // -----------------------------------------------------------------------
    // FIX 4: FilteredAggregation with LIKE predicate → None (negative test)
    //
    // LIKE is not in the MetricQL filter grammar. serialize_metricql_predicate
    // returns None for any unrecognised predicate type (the catch-all `_ => None`
    // arm), which propagates as None from translate_filtered_aggregation, which
    // propagates as None from translate. No bad MetricDefinition is produced.
    //
    // A4 added the equivalent test for Tier 1 (tier1.rs). This mirrors it for
    // Tier 2 to ensure the same LIKE → None behaviour holds after the Tier 1
    // fall-through route reaches Tier 2.
    //
    // Why Tier 2 sees this SQL:
    //   Tier 1 also rejects it — LIKE is not in Tier 1's predicate allowlist
    //   either (predicates_to_filter_sql calls serialize_filter_predicate which
    //   returns None for LIKE). So the SQL reaches Tier 2 as FilteredAggregation.
    //   Tier 2's serialize_metricql_predicate must also return None for LIKE.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn filtered_aggregation_with_like_predicate_returns_none() {
        let sql = "SELECT AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'play' AND me.title LIKE '%action%' \
                   GROUP BY me.user_id";
        let original = custom_original("like_predicate_test");
        let stmt = parse_or_tier3(sql).expect("must parse");
        let shape = extract_shape(&stmt);

        // Classifier must yield FilteredAggregation.
        assert_eq!(
            shape,
            Some(ShapeHint::FilteredAggregation),
            "expected FilteredAggregation for AVG with LIKE; got {shape:?}"
        );

        let result =
            translate(&stmt, ShapeHint::FilteredAggregation, &original, &EmptyLookup).await;

        // LIKE is unserializable in MetricQL — must fall through to Tier 3.
        assert!(
            result.is_none(),
            "FilteredAggregation with LIKE predicate must return None from tier2"
        );
    }
}
