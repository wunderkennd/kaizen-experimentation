//! Tier 1 translator for ADR-026 Phase 3 — Task A4.
//!
//! Converts a parsed SQL `Statement` (after A3 classification) into a
//! structured `MetricDefinition` for one of the three Tier 1 target types:
//!
//! | Shape hint           | Target type    |
//! |----------------------|----------------|
//! | `FilteredAggregation`| FILTERED_MEAN  |
//! | `WindowedAggregation`| WINDOWED_COUNT |
//! | `CompositeArithmetic`| COMPOSITE      |
//! | `RatioOfSums`        | *not handled*  |
//!
//! After building the candidate `MetricDefinition`, the translated metric is
//! passed through `validators::validate_metric_definition` for a round-trip
//! validation (L8 / PR-#567 bug class: the translator must never produce output
//! that the validator rejects).
//!
//! ## Async design
//!
//! `translate` is `async` because `validate_metric_definition` is async (it
//! dispatches to a `MetricLookup` for COMPOSITE cycle-detection). All callers
//! must be in a tokio runtime. Do NOT use `block_on` inside this module.

use sqlparser::ast::{
    BinaryOperator, Expr, FunctionArguments, Interval, JoinConstraint, JoinOperator, SelectItem,
    SetExpr, Statement,
};
use sqlparser::ast::DateTimeField;

use experimentation_proto::experimentation::common::v1::{
    metric_definition::TypeConfig as MetricTypeConfig, CompositeConfig, CompositeOperand,
    CompositeOperator, FilteredMeanConfig, MetricDefinition, MetricType, WindowedCountConfig,
};

use crate::validators::{validate_metric_definition, MetricLookup};
use super::classifier::ShapeHint;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which Tier 1 target type was produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Tier {
    Filtered,
    Composite,
    WindowedCount,
}

/// A successful translation proposal from Tier 1.
pub struct TranslationProposal {
    pub metric: MetricDefinition,
    pub tier: Tier,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Attempt to translate `stmt` (which has shape hint `shape`) into a Tier 1
/// `MetricDefinition`.
///
/// Returns `Some(proposal)` if and only if:
///   1. The AST matches the expected structural pattern for `shape`.
///   2. The synthesized `MetricDefinition` passes `validate_metric_definition`.
///
/// Returns `None` in all other cases — the caller should fall through to
/// Tier 2 (A5) or Tier 3.
///
/// ## Parameters
///
/// * `stmt` — the parsed SQL statement (from `classifier::parse_or_tier3`).
/// * `shape` — the structural shape hint (from `classifier::extract_shape`).
/// * `original` — the source CUSTOM `MetricDefinition`; metadata fields
///   (`name`, `description`, `lower_is_better`, …) are copied verbatim.
/// * `lookup` — a `MetricLookup` impl for the validation round-trip.
///   For FILTERED_MEAN and WINDOWED_COUNT, an empty lookup is sufficient.
///   For COMPOSITE, the lookup must be seeded with the operand metric IDs
///   so that `validate_composite`'s existence check passes.
pub async fn translate<L>(
    stmt: &Statement,
    shape: ShapeHint,
    original: &MetricDefinition,
    lookup: &L,
) -> Option<TranslationProposal>
where
    L: MetricLookup + ?Sized,
{
    let candidate = match shape {
        ShapeHint::FilteredAggregation => translate_filtered_mean(stmt, original)?,
        ShapeHint::WindowedAggregation => translate_windowed_count(stmt, original)?,
        ShapeHint::CompositeArithmetic => translate_composite(stmt, original)?,
        // RatioOfSums is NOT a Tier 1 shape — A5 handles it as Tier 2.
        ShapeHint::RatioOfSums => return None,
    };

    // Round-trip validation: only return Some if the validator accepts the
    // candidate. This prevents the PR-#567 bug class where the translator
    // produces output that M5 would reject at create time.
    if validate_metric_definition(&candidate.metric, lookup).await.is_err() {
        return None;
    }

    Some(candidate)
}

// ---------------------------------------------------------------------------
// FILTERED_MEAN translator
// ---------------------------------------------------------------------------
//
// Pattern:
//   SELECT <table_alias>.user_id, AVG(<alias>.<value_col>) AS metric_value
//   FROM delta.metric_events <alias>
//   [INNER JOIN delta.exposures ... ON ...]
//   WHERE <alias>.event_type = '<event_type>' [AND <remaining_filter>]
//   GROUP BY <alias>.user_id
//
// Extraction:
//   - value_column: the column name inside AVG(…), stripped of table alias
//   - source_event_type: the literal from `alias.event_type = '<lit>'`
//   - filter_sql: the remaining AND'd predicates, qualified identifiers stripped
//     of table alias, emitted as a string that passes validate_filter_sql

fn translate_filtered_mean(
    stmt: &Statement,
    original: &MetricDefinition,
) -> Option<TranslationProposal> {
    let select = extract_select(stmt)?;

    // Validate FROM clause: must be exactly metric_events + optional exposures JOIN.
    // Any additional JOINs (e.g. user_profiles, content_catalog) are Tier 3 — they
    // introduce table-specific predicates that can't be captured in filter_sql.
    if !from_is_events_with_optional_exposures_join(&select.from) {
        return None;
    }

    // Extract value_column from AVG(alias.col) OR SUM(alias.col)/COUNT(*) projection.
    let value_column = find_avg_column(&select.projection)
        .or_else(|| find_sum_div_count_column(&select.projection))?;

    // Extract event_type and remaining filter predicates from WHERE.
    let where_expr = select.selection.as_ref()?;
    let (event_type, filter_predicates) = split_where_by_event_type(where_expr)?;

    // Reconstruct filter_sql from remaining predicates (strip table alias prefix).
    // Returns None if any predicate is unserializable (e.g. LIKE, BETWEEN, IN-subquery).
    let filter_sql = predicates_to_filter_sql(&filter_predicates)?;

    // filter_sql is REQUIRED for FILTERED_MEAN (otherwise use MEAN).
    if filter_sql.trim().is_empty() {
        return None;
    }

    let candidate = build_metric(
        original,
        MetricType::FilteredMean,
        MetricTypeConfig::FilteredMean(FilteredMeanConfig {
            value_column,
            filter_sql,
        }),
    );
    // Copy source_event_type.
    let mut m = candidate;
    m.source_event_type = event_type;

    Some(TranslationProposal {
        metric: m,
        tier: Tier::Filtered,
        reason: "matches FILTERED_MEAN shape".to_string(),
    })
}

// ---------------------------------------------------------------------------
// WINDOWED_COUNT translator
// ---------------------------------------------------------------------------
//
// Pattern:
//   SELECT eu.user_id, eu.variant_id, CAST(COUNT(me.user_id) AS DOUBLE) AS metric_value
//   FROM delta.exposures eu
//   LEFT JOIN delta.metric_events me ON
//     eu.user_id = me.user_id
//     AND me.event_type = '<event_type>'
//     AND me.event_timestamp >= eu.exposure_ts
//     AND me.event_timestamp < eu.exposure_ts + INTERVAL '<N>' HOUR
//     [AND <remaining_filter>]
//   GROUP BY eu.user_id, eu.variant_id
//
// Extraction from JOIN ON condition:
//   - event_type: literal from `me.event_type = '<lit>'`
//   - window_hours: integer from `INTERVAL '<N>' HOUR`
//   - filter_sql: remaining predicates (not user_id join, not event_type, not timestamps)

fn translate_windowed_count(
    stmt: &Statement,
    original: &MetricDefinition,
) -> Option<TranslationProposal> {
    let select = extract_select(stmt)?;

    // Validate FROM clause: must be exposures (main) JOIN metric_events (joined).
    // Any other table pair violates the WINDOWED_COUNT structural contract (L4).
    if !from_is_exposures_with_metric_events_join(&select.from) {
        return None;
    }

    // Find the JOIN ON condition — it contains event_type, window, and optional filter.
    let join_on = find_join_on_expr(&select.from)?;

    // Collect all AND'd predicates from the JOIN ON.
    let predicates = collect_and_predicates(join_on);

    // Extract event_type predicate.
    let event_type = extract_event_type_from_predicates(&predicates)?;

    // Extract window_hours from the INTERVAL predicate.
    let window_hours = extract_window_hours_from_predicates(&predicates)?;

    // The remaining predicates (after removing: user_id join, event_type, timestamp predicates)
    // become the filter_sql.
    let filter_predicates: Vec<&Expr> = predicates
        .iter()
        .copied()
        .filter(|p| {
            !is_user_id_join_predicate(p)
                && !is_event_type_predicate(p)
                && !is_timestamp_predicate(p)
        })
        .collect();

    // Returns None if any predicate is unserializable (conservative L4/L8).
    let filter_sql = predicates_to_filter_sql(&filter_predicates)?;

    let candidate = build_metric(
        original,
        MetricType::WindowedCount,
        MetricTypeConfig::WindowedCount(WindowedCountConfig {
            event_type,
            filter_sql,
            window_hours,
        }),
    );

    Some(TranslationProposal {
        metric: candidate,
        tier: Tier::WindowedCount,
        reason: "COUNT within window".to_string(),
    })
}

// ---------------------------------------------------------------------------
// COMPOSITE translator
// ---------------------------------------------------------------------------
//
// Pattern (from metric_summaries pivot):
//   SELECT ms.user_id, ms.variant_id, <arithmetic_expr> AS metric_value
//   FROM delta.metric_summaries ms
//   WHERE ...
//
// The arithmetic expression is built from:
//   MAX(CASE WHEN ms.metric_id = '<id>' THEN ms.metric_value END)
//
// Supported shapes:
//   A + B                       → COMPOSITE_OPERATOR_ADD
//   A - B                       → COMPOSITE_OPERATOR_SUBTRACT
//   A * B (pivot only)          → COMPOSITE_OPERATOR_MULTIPLY
//   A / NULLIF(B, 0)            → COMPOSITE_OPERATOR_DIVIDE
//   w1 * A + w2 * B [+ ...]     → COMPOSITE_OPERATOR_WEIGHTED_SUM
//
// WEIGHTED_SUM detection: the top-level binary tree is all-Plus and every
// leaf is `<number> * <pivot>` OR `<pivot> * <number>`.

fn translate_composite(
    stmt: &Statement,
    original: &MetricDefinition,
) -> Option<TranslationProposal> {
    let select = extract_select(stmt)?;

    // Find the metric_value projection item (the non-identifier, aliased to
    // metric_value). Fall back to any complex expression.
    let expr = find_metric_value_expr(&select.projection)?;

    // Unwrap Nested parens.
    let expr = unwrap_nested(expr);

    // Try to extract (operator, operands) from the arithmetic expression.
    let (op, operands) = extract_composite_op_and_operands(expr)?;

    // Must have at least 2 operands.
    if operands.len() < 2 {
        return None;
    }

    // SUBTRACT and DIVIDE require exactly 2 operands.
    if matches!(op, CompositeOperator::Subtract | CompositeOperator::Divide)
        && operands.len() != 2
    {
        return None;
    }

    let proto_operands: Vec<CompositeOperand> = operands
        .into_iter()
        .map(|(metric_id, weight)| CompositeOperand { metric_id, weight })
        .collect();

    let candidate = build_metric(
        original,
        MetricType::Composite,
        MetricTypeConfig::Composite(CompositeConfig {
            operator: op as i32,
            operands: proto_operands,
        }),
    );

    Some(TranslationProposal {
        metric: candidate,
        tier: Tier::Composite,
        reason: "matches COMPOSITE shape".to_string(),
    })
}

// ---------------------------------------------------------------------------
// AST walking helpers — FILTERED_MEAN
// ---------------------------------------------------------------------------

/// Find the column argument inside an `AVG(alias.col)` projection item.
/// Returns the bare column name (no table alias).
fn find_avg_column(projection: &[SelectItem]) -> Option<String> {
    for item in projection {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => continue,
        };
        if let Some(col) = extract_avg_col(expr) {
            return Some(col);
        }
    }
    None
}

fn extract_avg_col(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Function(f) if f.name.to_string().eq_ignore_ascii_case("avg") => {
            // AVG(alias.col) or AVG(col)
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(inner),
                )) = list.args.first()
                {
                    return strip_table_alias(inner);
                }
            }
            None
        }
        Expr::Nested(inner) => extract_avg_col(inner),
        _ => None,
    }
}

/// Find the column argument inside a `SUM(alias.col) / COUNT(*)` projection item.
/// Returns the bare column name (no table alias), matching the same contract as
/// `find_avg_column` so callers can use either interchangeably.
///
/// Accepted projection shapes:
///   `SUM(alias.col) / COUNT(*)`
///   `SUM(col) / COUNT(*)`
fn find_sum_div_count_column(projection: &[SelectItem]) -> Option<String> {
    for item in projection {
        let expr = match item {
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
            _ => continue,
        };
        if let Some(col) = extract_sum_div_count_col(expr) {
            return Some(col);
        }
    }
    None
}

fn extract_sum_div_count_col(expr: &Expr) -> Option<String> {
    let expr = unwrap_nested(expr);
    // Pattern: BinaryOp { left: SUM(col), op: Divide, right: COUNT(*) }
    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::Divide,
        right,
    } = expr
    {
        let left = unwrap_nested(left);
        let right = unwrap_nested(right);
        // right must be COUNT(*) or COUNT(1)
        if !is_count_star_or_one(right) {
            return None;
        }
        // left must be SUM(alias.col)
        if let Expr::Function(f) = left {
            if f.name.to_string().eq_ignore_ascii_case("sum") {
                if let FunctionArguments::List(list) = &f.args {
                    if let Some(sqlparser::ast::FunctionArg::Unnamed(
                        sqlparser::ast::FunctionArgExpr::Expr(inner),
                    )) = list.args.first()
                    {
                        return strip_table_alias(inner);
                    }
                }
            }
        }
    }
    None
}

fn is_count_star_or_one(expr: &Expr) -> bool {
    if let Expr::Function(f) = expr {
        if f.name.to_string().eq_ignore_ascii_case("count") {
            if let FunctionArguments::List(list) = &f.args {
                // COUNT(*) with empty args list
                if list.args.is_empty() {
                    return true;
                }
                if let Some(arg) = list.args.first() {
                    match arg {
                        sqlparser::ast::FunctionArg::Unnamed(
                            sqlparser::ast::FunctionArgExpr::Wildcard,
                        ) => return true,
                        sqlparser::ast::FunctionArg::Unnamed(
                            sqlparser::ast::FunctionArgExpr::Expr(Expr::Value(
                                sqlparser::ast::Value::Number(n, _),
                            )),
                        ) => return n == "1",
                        _ => {}
                    }
                }
            }
        }
    }
    false
}

/// Split a WHERE `Expr` into:
///   - `event_type`: the string literal from `alias.event_type = '<lit>'`
///   - remaining predicates: everything else (excluding the event_type predicate)
///
/// Returns `None` if there is no `event_type` predicate.
fn split_where_by_event_type<'a>(
    expr: &'a Expr,
) -> Option<(String, Vec<&'a Expr>)> {
    let preds = collect_and_predicates(expr);
    let mut event_type = None;
    let mut remaining: Vec<&'a Expr> = Vec::new();

    for pred in &preds {
        if let Some(et) = extract_event_type_literal(pred) {
            if event_type.is_none() {
                event_type = Some(et);
                // Do NOT push to remaining.
                continue;
            }
        }
        remaining.push(pred);
    }

    Some((event_type?, remaining))
}

/// If `expr` is `<col_or_alias_col> = '<literal>'` where the column name
/// (after stripping any table alias) is `event_type`, return `Some(literal)`.
fn extract_event_type_literal(expr: &Expr) -> Option<String> {
    if let Expr::BinaryOp {
        left,
        op: BinaryOperator::Eq,
        right,
    } = expr
    {
        let col_name = strip_table_alias(left)?;
        if col_name == "event_type" {
            if let Expr::Value(sqlparser::ast::Value::SingleQuotedString(s)) = right.as_ref() {
                return Some(s.clone());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// AST walking helpers — WINDOWED_COUNT
// ---------------------------------------------------------------------------

/// Validate that the FILTERED_MEAN FROM clause is `metric_events [JOIN exposures]`.
///
/// Accepts:
///   - single table `metric_events` (no joins)
///   - `metric_events JOIN exposures` (exactly one join, joined table is `exposures`)
///
/// Rejects any other shape (multiple joins, joined table is not `exposures`, etc.).
/// This guards against the multi-table-join Tier 3 case where predicates on
/// non-events tables (e.g. `user_profiles.country`) can't be captured in `filter_sql`.
pub(crate) fn from_is_events_with_optional_exposures_join(
    from: &[sqlparser::ast::TableWithJoins],
) -> bool {
    if from.len() != 1 {
        return false;
    }
    let twj = &from[0];
    // Main table must be metric_events.
    let main_table = table_name_from_factor_str(&twj.relation);
    if !main_table.map(|n| n.contains("metric_events")).unwrap_or(false) {
        return false;
    }
    // Zero or one join allowed; if one, it must be to exposures.
    match twj.joins.len() {
        0 => true,
        1 => {
            let joined = table_name_from_factor_str(&twj.joins[0].relation);
            joined.map(|n| n.contains("exposures")).unwrap_or(false)
        }
        _ => false, // multiple joins → Tier 3
    }
}

/// Validate that the WINDOWED_COUNT FROM clause is `exposures JOIN metric_events`.
///
/// The WINDOWED_COUNT pattern (per `windowed_count.sql.tmpl`) requires:
///   - main table: `exposures` or `delta.exposures`
///   - exactly one join: `metric_events` or `delta.metric_events`
///
/// Any other table pair (e.g. `orders JOIN shipments`) violates L4 conservatism
/// and must fall through to Tier 3.
pub(crate) fn from_is_exposures_with_metric_events_join(
    from: &[sqlparser::ast::TableWithJoins],
) -> bool {
    if from.len() != 1 {
        return false;
    }
    let twj = &from[0];
    // Main table must be exposures.
    let main_table = table_name_from_factor_str(&twj.relation);
    if !main_table.map(|n| n.contains("exposures")).unwrap_or(false) {
        return false;
    }
    // Exactly one join required, joined table must be metric_events.
    if twj.joins.len() != 1 {
        return false;
    }
    let joined = table_name_from_factor_str(&twj.joins[0].relation);
    joined.map(|n| n.contains("metric_events")).unwrap_or(false)
}

fn table_name_from_factor_str(factor: &sqlparser::ast::TableFactor) -> Option<String> {
    match factor {
        sqlparser::ast::TableFactor::Table { name, .. } => {
            Some(name.to_string().to_ascii_lowercase())
        }
        _ => None,
    }
}

/// Find the single JOIN ON expression in the FROM clause.
pub(crate) fn find_join_on_expr(from: &[sqlparser::ast::TableWithJoins]) -> Option<&Expr> {
    for twj in from {
        for join in &twj.joins {
            let constraint = match &join.join_operator {
                JoinOperator::Inner(c)
                | JoinOperator::LeftOuter(c)
                | JoinOperator::RightOuter(c)
                | JoinOperator::FullOuter(c)
                | JoinOperator::LeftSemi(c)
                | JoinOperator::RightSemi(c)
                | JoinOperator::LeftAnti(c)
                | JoinOperator::RightAnti(c) => c,
                _ => continue,
            };
            if let JoinConstraint::On(ref on_expr) = constraint {
                return Some(on_expr);
            }
        }
    }
    None
}

/// True if `pred` is `eu.user_id = me.user_id` (or similar user_id equality join).
fn is_user_id_join_predicate(pred: &Expr) -> bool {
    if let Expr::BinaryOp { left, op: BinaryOperator::Eq, right } = pred {
        let l = strip_table_alias(left);
        let r = strip_table_alias(right);
        matches!((l.as_deref(), r.as_deref()), (Some("user_id"), Some("user_id")))
    } else {
        false
    }
}

/// True if `pred` is `alias.event_type = '<lit>'`.
fn is_event_type_predicate(pred: &Expr) -> bool {
    extract_event_type_literal(pred).is_some()
}

/// True if `pred` involves `event_timestamp` or `exposure_ts` — the window
/// predicates (`>= exposure_ts` and `< exposure_ts + INTERVAL N HOUR`).
fn is_timestamp_predicate(pred: &Expr) -> bool {
    expr_mentions_column(pred, "event_timestamp") || expr_mentions_column(pred, "exposure_ts")
}

fn expr_mentions_column(expr: &Expr, col: &str) -> bool {
    match expr {
        Expr::Identifier(id) => id.value == col,
        Expr::CompoundIdentifier(parts) => parts.last().map(|p| p.value == col).unwrap_or(false),
        Expr::BinaryOp { left, right, .. } => {
            expr_mentions_column(left, col) || expr_mentions_column(right, col)
        }
        Expr::UnaryOp { expr, .. } => expr_mentions_column(expr, col),
        Expr::Nested(inner) => expr_mentions_column(inner, col),
        Expr::Interval(Interval { value, .. }) => expr_mentions_column(value, col),
        _ => false,
    }
}

/// Extract `window_hours` from the INTERVAL predicate `alias.event_timestamp < alias.exposure_ts + INTERVAL '<N>' HOUR`.
fn extract_window_hours_from_predicates(predicates: &[&Expr]) -> Option<i32> {
    for pred in predicates {
        if let Expr::BinaryOp {
            left,
            op: BinaryOperator::Lt,
            right,
        } = pred
        {
            // left must mention event_timestamp
            if !expr_mentions_column(left, "event_timestamp") {
                continue;
            }
            // right must be `exposure_ts + INTERVAL '<N>' HOUR`
            if let Some(hours) = extract_interval_hours_from_addition(right) {
                return Some(hours);
            }
        }
    }
    None
}

fn extract_interval_hours_from_addition(expr: &Expr) -> Option<i32> {
    // Pattern: `exposure_ts + INTERVAL '<N>' HOUR`
    if let Expr::BinaryOp {
        left: _,
        op: BinaryOperator::Plus,
        right,
    } = expr
    {
        return extract_interval_hours(right);
    }
    None
}

fn extract_interval_hours(expr: &Expr) -> Option<i32> {
    if let Expr::Interval(interval) = expr {
        // leading_field should be Hour (DateTimeField::Hour)
        if matches!(interval.leading_field, Some(DateTimeField::Hour)) {
            // value is Expr::Value(SingleQuotedString("48"))
            if let Expr::Value(sqlparser::ast::Value::SingleQuotedString(s)) =
                interval.value.as_ref()
            {
                return s.parse::<i32>().ok();
            }
            // Also handle Expr::Value(Number("48", _)) if unquoted
            if let Expr::Value(sqlparser::ast::Value::Number(n, _)) = interval.value.as_ref() {
                return n.parse::<i32>().ok();
            }
        }
    }
    None
}

/// Extract `event_type` literal from predicates in a JOIN ON clause.
fn extract_event_type_from_predicates(predicates: &[&Expr]) -> Option<String> {
    for pred in predicates {
        if let Some(et) = extract_event_type_literal(pred) {
            return Some(et);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// AST walking helpers — COMPOSITE
// ---------------------------------------------------------------------------

/// Find the projection expression aliased as `metric_value`, or the last
/// non-identifier projection item.
fn find_metric_value_expr(projection: &[SelectItem]) -> Option<&Expr> {
    // Prefer item aliased as "metric_value".
    for item in projection {
        if let SelectItem::ExprWithAlias { expr, alias } = item {
            if alias.value.eq_ignore_ascii_case("metric_value") {
                return Some(expr);
            }
        }
    }
    // Fallback: the last non-identifier, non-wildcard item.
    for item in projection.iter().rev() {
        match item {
            SelectItem::UnnamedExpr(e) => {
                if !matches!(e, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
                    return Some(e);
                }
            }
            SelectItem::ExprWithAlias { expr: e, .. } => {
                if !matches!(e, Expr::Identifier(_) | Expr::CompoundIdentifier(_)) {
                    return Some(e);
                }
            }
            _ => continue,
        }
    }
    None
}

/// Extract (operator, vec[(metric_id, weight)]) from a composite arithmetic expression.
///
/// Supported top-level shapes:
///   A + B [+ C ...]           → ADD, weight=0.0
///   A - B                     → SUBTRACT, weight=0.0
///   A * B                     → MULTIPLY, weight=0.0
///   A / B   or  A / NULLIF(B, 0)  → DIVIDE, weight=0.0
///   w*A + w*B [+ ...]         → WEIGHTED_SUM, weight=coefficient
fn extract_composite_op_and_operands(
    expr: &Expr,
) -> Option<(CompositeOperator, Vec<(String, f64)>)> {
    let expr = unwrap_nested(expr);

    // Check for WEIGHTED_SUM first: all-Plus tree where leaves are `num * pivot` or `pivot * num`.
    if let Some(operands) = try_weighted_sum(expr) {
        return Some((CompositeOperator::WeightedSum, operands));
    }

    // DIVIDE: A / B  or A / NULLIF(B, 0)
    if let Expr::BinaryOp { left, op: BinaryOperator::Divide, right } = expr {
        let a = extract_pivot_metric_id(unwrap_nested(left))?;
        let b = extract_pivot_metric_id(unwrap_nullif_pivot(unwrap_nested(right)))?;
        return Some((CompositeOperator::Divide, vec![(a, 0.0), (b, 0.0)]));
    }

    // SUBTRACT: A - B
    if let Expr::BinaryOp { left, op: BinaryOperator::Minus, right } = expr {
        let a = extract_pivot_metric_id(unwrap_nested(left))?;
        let b = extract_pivot_metric_id(unwrap_nested(right))?;
        return Some((CompositeOperator::Subtract, vec![(a, 0.0), (b, 0.0)]));
    }

    // MULTIPLY: A * B (both pivots, not weighted)
    if let Expr::BinaryOp { left, op: BinaryOperator::Multiply, right } = expr {
        if let (Some(a), Some(b)) = (
            extract_pivot_metric_id(unwrap_nested(left)),
            extract_pivot_metric_id(unwrap_nested(right)),
        ) {
            return Some((CompositeOperator::Multiply, vec![(a, 0.0), (b, 0.0)]));
        }
    }

    // ADD: collect all Plus-connected leaves.
    if let Some(operands) = try_add(expr) {
        if operands.len() >= 2 {
            return Some((CompositeOperator::Add, operands));
        }
    }

    None
}

/// Try to parse the expression as a WEIGHTED_SUM: an all-Plus tree where
/// every leaf is `<number> * <pivot>` or `<pivot> * <number>`.
fn try_weighted_sum(expr: &Expr) -> Option<Vec<(String, f64)>> {
    let leaves = collect_plus_leaves(expr);
    if leaves.len() < 2 {
        return None;
    }
    let mut operands = Vec::new();
    for leaf in leaves {
        let leaf = unwrap_nested(leaf);
        if let Expr::BinaryOp { left, op: BinaryOperator::Multiply, right } = leaf {
            // Try `num * pivot` or `pivot * num`
            let maybe = if let Some(w) = extract_number_literal(unwrap_nested(left)) {
                extract_pivot_metric_id(unwrap_nested(right)).map(|id| (id, w))
            } else if let Some(w) = extract_number_literal(unwrap_nested(right)) {
                extract_pivot_metric_id(unwrap_nested(left)).map(|id| (id, w))
            } else {
                None
            };
            operands.push(maybe?);
        } else {
            // Leaf is not a weighted term — not WEIGHTED_SUM.
            return None;
        }
    }
    Some(operands)
}

/// Try to parse the expression as ADD: an all-Plus tree where every leaf
/// is a bare pivot `MAX(CASE WHEN ...)`.
fn try_add(expr: &Expr) -> Option<Vec<(String, f64)>> {
    let leaves = collect_plus_leaves(expr);
    let mut operands = Vec::new();
    for leaf in leaves {
        let leaf = unwrap_nested(leaf);
        let id = extract_pivot_metric_id(leaf)?;
        operands.push((id, 0.0));
    }
    Some(operands)
}

/// Collect all leaves of a left-associative Plus tree.
fn collect_plus_leaves(expr: &Expr) -> Vec<&Expr> {
    let expr = unwrap_nested(expr);
    if let Expr::BinaryOp { left, op: BinaryOperator::Plus, right } = expr {
        let mut leaves = collect_plus_leaves(left);
        leaves.push(right.as_ref());
        leaves
    } else {
        vec![expr]
    }
}

/// Extract the metric_id string from `MAX(CASE WHEN alias.metric_id = '<id>' THEN alias.metric_value END)`.
fn extract_pivot_metric_id(expr: &Expr) -> Option<String> {
    let expr = unwrap_nested(expr);
    // MAX(CASE ...) or MIN(CASE ...)
    if let Expr::Function(f) = expr {
        let fname = f.name.to_string().to_ascii_lowercase();
        if fname == "max" || fname == "min" {
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(inner),
                )) = list.args.first()
                {
                    return extract_case_metric_id(inner);
                }
            }
        }
        // NULLIF wrapping: handled by unwrap_nullif_pivot before calling this.
    }
    None
}

/// From a NULLIF(MAX(...), 0) expression, unwrap to MAX(...).
fn unwrap_nullif_pivot(expr: &Expr) -> &Expr {
    if let Expr::Function(f) = expr {
        if f.name.to_string().eq_ignore_ascii_case("nullif") {
            if let FunctionArguments::List(list) = &f.args {
                if let Some(sqlparser::ast::FunctionArg::Unnamed(
                    sqlparser::ast::FunctionArgExpr::Expr(inner),
                )) = list.args.first()
                {
                    return inner;
                }
            }
        }
    }
    expr
}

/// Extract metric_id from `CASE WHEN alias.metric_id = '<id>' THEN alias.metric_value END`.
fn extract_case_metric_id(expr: &Expr) -> Option<String> {
    if let Expr::Case { operand: None, conditions, results: _, else_result: _ } = expr {
        if let Some(cond) = conditions.first() {
            return extract_event_type_literal_for_field(cond, "metric_id");
        }
    }
    None
}

/// Like `extract_event_type_literal` but for any column name (e.g., `metric_id`).
fn extract_event_type_literal_for_field(expr: &Expr, col: &str) -> Option<String> {
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

/// Extract a numeric literal from `Expr::Value(Number(...))`.
fn extract_number_literal(expr: &Expr) -> Option<f64> {
    if let Expr::Value(sqlparser::ast::Value::Number(n, _)) = expr {
        return n.parse::<f64>().ok();
    }
    None
}

// ---------------------------------------------------------------------------
// filter_sql serializer
// ---------------------------------------------------------------------------
//
// Takes a slice of predicate AST expressions and reconstructs them as a
// flat SQL string that passes `validate_filter_sql`.
//
// Identifiers are emitted as their bare (alias-stripped) lowercase form.
// String literals use single-quote wrapping. Numeric literals are emitted
// as-is. IN lists use `IN (...)` form.
//
// Complex predicates that can't be reliably serialized without risk of
// producing disallowed tokens (function calls, subqueries, LIKE, BETWEEN)
// emit `None` from `serialize_predicate`, and the caller returns `None`
// from the entire translator (conservative L8).

/// Serialize `predicates` into a flat SQL AND-string suitable for `filter_sql`.
///
/// Returns `None` if **any** predicate cannot be serialized (e.g. LIKE, BETWEEN,
/// IN-with-subquery, function calls). This is the conservative L4/L8 policy —
/// dropping even one predicate would silently widen the semantic filter, which is
/// exactly the bug class rejected in PR #567.
///
/// Returns `Some("")` for an empty slice (legitimate "no filter" state).
fn predicates_to_filter_sql(predicates: &[&Expr]) -> Option<String> {
    if predicates.is_empty() {
        return Some(String::new());
    }
    let mut parts = Vec::with_capacity(predicates.len());
    for p in predicates {
        let s = serialize_predicate(p)?; // bail on first unserializable predicate
        parts.push(s);
    }
    Some(parts.join(" AND "))
}

fn serialize_predicate(expr: &Expr) -> Option<String> {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let op_str = binary_op_str(op)?;
            let l = serialize_simple_expr(left)?;
            let r = serialize_simple_expr(right)?;
            Some(format!("{l} {op_str} {r}"))
        }
        Expr::Nested(inner) => serialize_predicate(inner),
        Expr::UnaryOp { op: sqlparser::ast::UnaryOperator::Not, expr } => {
            let inner = serialize_predicate(expr)?;
            Some(format!("NOT {inner}"))
        }
        Expr::InList { expr, list, negated } => {
            let col = serialize_simple_expr(expr)?;
            let items: Option<Vec<String>> = list.iter().map(serialize_simple_expr).collect();
            let items_str = items?.join(", ");
            let neg = if *negated { "NOT IN" } else { "IN" };
            Some(format!("{col} {neg} ({items_str})"))
        }
        Expr::IsNull(inner) => {
            let col = serialize_simple_expr(inner)?;
            Some(format!("{col} IS NULL"))
        }
        Expr::IsNotNull(inner) => {
            let col = serialize_simple_expr(inner)?;
            Some(format!("{col} IS NOT NULL"))
        }
        _ => None, // conservative — unknown shape → None → caller returns None
    }
}

fn serialize_simple_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Identifier(id) => Some(id.value.to_ascii_lowercase()),
        Expr::CompoundIdentifier(parts) => {
            // Strip table alias: take the last part only.
            Some(parts.last()?.value.to_ascii_lowercase())
        }
        Expr::Value(v) => match v {
            sqlparser::ast::Value::SingleQuotedString(s) => Some(format!("'{s}'")),
            sqlparser::ast::Value::Number(n, _) => Some(n.clone()),
            sqlparser::ast::Value::Null => Some("NULL".to_string()),
            _ => None,
        },
        Expr::Nested(inner) => serialize_simple_expr(inner),
        _ => None,
    }
}

fn binary_op_str(op: &BinaryOperator) -> Option<&'static str> {
    match op {
        BinaryOperator::Eq => Some("="),
        BinaryOperator::NotEq => Some("!="),
        BinaryOperator::Lt => Some("<"),
        BinaryOperator::LtEq => Some("<="),
        BinaryOperator::Gt => Some(">"),
        BinaryOperator::GtEq => Some(">="),
        BinaryOperator::And => Some("AND"),
        BinaryOperator::Or => Some("OR"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shared AST utilities
// ---------------------------------------------------------------------------

/// Extract the `Select` body from a `Statement::Query`.
pub(crate) fn extract_select(stmt: &Statement) -> Option<&sqlparser::ast::Select> {
    if let Statement::Query(q) = stmt {
        if let SetExpr::Select(s) = q.body.as_ref() {
            return Some(s.as_ref());
        }
    }
    None
}

/// Recursively collect all leaf predicates of an AND tree.
pub(crate) fn collect_and_predicates(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::BinaryOp { left, op: BinaryOperator::And, right } => {
            let mut v = collect_and_predicates(left);
            v.extend(collect_and_predicates(right));
            v
        }
        Expr::Nested(inner) => collect_and_predicates(inner),
        other => vec![other],
    }
}

/// Strip table alias from `alias.col` → `col`. Returns `None` for complex expressions.
pub(crate) fn strip_table_alias(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Identifier(id) => Some(id.value.clone()),
        Expr::CompoundIdentifier(parts) => {
            // `alias.col` → take the last ident.
            parts.last().map(|id| id.value.clone())
        }
        _ => None,
    }
}

/// Unwrap `Expr::Nested(...)` recursively.
pub(crate) fn unwrap_nested(expr: &Expr) -> &Expr {
    match expr {
        Expr::Nested(inner) => unwrap_nested(inner),
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Proto builder
// ---------------------------------------------------------------------------

/// Build a `MetricDefinition` for the specified type, copying metadata from
/// `original` and setting `metric_id` to `<original_id>-migrated`.
fn build_metric(
    original: &MetricDefinition,
    metric_type: MetricType,
    type_config: MetricTypeConfig,
) -> MetricDefinition {
    MetricDefinition {
        // Generate a new migrated ID (per L7: two-step migration creates a new metric_id).
        metric_id: format!("{}-migrated", original.metric_id),
        // Copy metadata verbatim from original.
        name: original.name.clone(),
        description: original.description.clone(),
        lower_is_better: original.lower_is_better,
        surrogate_target_metric_id: original.surrogate_target_metric_id.clone(),
        is_qoe_metric: original.is_qoe_metric,
        cuped_covariate_metric_id: original.cuped_covariate_metric_id.clone(),
        minimum_detectable_effect: original.minimum_detectable_effect,
        stakeholder: original.stakeholder,
        aggregation_level: original.aggregation_level,
        // Set type.
        r#type: metric_type as i32,
        // Set type_config.
        type_config: Some(type_config),
        // Clear CUSTOM-specific fields.
        custom_sql: String::new(),
        metricql_expression: String::new(),
        // source_event_type will be set by caller if needed (FILTERED_MEAN).
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::migration::test_support::{EmptyLookup, SeedLookup};

    // -----------------------------------------------------------------------
    // Shared helpers
    // -----------------------------------------------------------------------

    fn custom_original(metric_id: &str) -> MetricDefinition {
        MetricDefinition {
            metric_id: metric_id.to_string(),
            name: "Test Metric".to_string(),
            r#type: MetricType::Custom as i32,
            ..Default::default()
        }
    }

    fn parse_and_shape(sql: &str) -> (Statement, ShapeHint) {
        use crate::migration::classifier::{extract_shape, parse_or_tier3};
        let stmt = parse_or_tier3(sql).expect("must parse");
        let shape = extract_shape(&stmt).expect("must classify");
        (stmt, shape)
    }

    // -----------------------------------------------------------------------
    // FILTERED_MEAN unit tests (RED → GREEN)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn filtered_mean_mobile_watch_time() {
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'video_play' AND me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = custom_original("mobile_watch_time");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(proposal.tier, Tier::Filtered);
        assert_eq!(proposal.metric.source_event_type, "video_play");
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::FilteredMean(c) => c,
            other => panic!("expected FilteredMean, got: {other:?}"),
        };
        assert_eq!(cfg.value_column, "duration_ms");
        assert_eq!(cfg.filter_sql, "platform = 'mobile'");
    }

    #[tokio::test]
    async fn filtered_mean_high_quality_stream() {
        let sql = "SELECT me.user_id, AVG(me.bitrate_kbps) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'heartbeat' AND me.bitrate_kbps > 2000 \
                   GROUP BY me.user_id";
        let original = custom_original("high_quality_stream");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::FilteredMean(c) => c,
            _ => panic!(),
        };
        assert_eq!(proposal.metric.source_event_type, "heartbeat");
        assert_eq!(cfg.value_column, "bitrate_kbps");
        assert_eq!(cfg.filter_sql, "bitrate_kbps > 2000");
    }

    #[tokio::test]
    async fn filtered_mean_ttff_desktop_non_premium() {
        let sql = "SELECT me.user_id, AVG(me.ttff_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'play_start' AND me.platform = 'desktop' \
                   AND me.subscription_tier != 'premium' \
                   GROUP BY me.user_id";
        let original = custom_original("ttff_desktop_non_premium");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::FilteredMean(c) => c,
            _ => panic!(),
        };
        assert_eq!(proposal.metric.source_event_type, "play_start");
        assert_eq!(cfg.value_column, "ttff_ms");
        assert_eq!(cfg.filter_sql, "platform = 'desktop' AND subscription_tier != 'premium'");
    }

    #[tokio::test]
    async fn filtered_mean_rebuffer_ratio_mobile() {
        let sql = "SELECT me.user_id, AVG(me.rebuffer_ratio) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'heartbeat' AND me.platform = 'mobile' \
                   AND me.rebuffer_ratio >= 0 \
                   GROUP BY me.user_id";
        let original = custom_original("rebuffer_ratio_mobile");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::FilteredMean(c) => c,
            _ => panic!(),
        };
        assert_eq!(proposal.metric.source_event_type, "heartbeat");
        assert_eq!(cfg.value_column, "rebuffer_ratio");
        assert_eq!(cfg.filter_sql, "platform = 'mobile' AND rebuffer_ratio >= 0");
    }

    #[tokio::test]
    async fn filtered_mean_content_completion_long_form() {
        let sql = "SELECT me.user_id, AVG(me.completion_ratio) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'video_play' AND me.content_type = 'series' \
                   AND me.duration_ms > 1200000 \
                   GROUP BY me.user_id";
        let original = custom_original("content_completion_long_form");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::FilteredMean(c) => c,
            _ => panic!(),
        };
        assert_eq!(proposal.metric.source_event_type, "video_play");
        assert_eq!(cfg.value_column, "completion_ratio");
        assert_eq!(cfg.filter_sql, "content_type = 'series' AND duration_ms > 1200000");
    }

    // -----------------------------------------------------------------------
    // WINDOWED_COUNT unit tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn windowed_count_purchase_48h() {
        let sql = "SELECT eu.user_id, eu.variant_id, CAST(COUNT(me.user_id) AS DOUBLE) AS metric_value \
                   FROM delta.exposures eu \
                   LEFT JOIN delta.metric_events me \
                     ON eu.user_id = me.user_id \
                     AND me.event_type = 'purchase_completed' \
                     AND me.event_timestamp >= eu.exposure_ts \
                     AND me.event_timestamp < eu.exposure_ts + INTERVAL '48' HOUR \
                   GROUP BY eu.user_id, eu.variant_id";
        let original = custom_original("purchase_48h");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(proposal.tier, Tier::WindowedCount);
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::WindowedCount(c) => c,
            _ => panic!(),
        };
        assert_eq!(cfg.event_type, "purchase_completed");
        assert_eq!(cfg.window_hours, 48);
        assert_eq!(cfg.filter_sql, "");
    }

    #[tokio::test]
    async fn windowed_count_add_to_list_24h_mobile() {
        let sql = "SELECT eu.user_id, eu.variant_id, CAST(COUNT(me.user_id) AS DOUBLE) AS metric_value \
                   FROM delta.exposures eu \
                   LEFT JOIN delta.metric_events me \
                     ON eu.user_id = me.user_id \
                     AND me.event_type = 'add_to_list' \
                     AND me.event_timestamp >= eu.exposure_ts \
                     AND me.event_timestamp < eu.exposure_ts + INTERVAL '24' HOUR \
                     AND me.platform = 'mobile' \
                   GROUP BY eu.user_id, eu.variant_id";
        let original = custom_original("add_to_list_24h_mobile");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::WindowedCount(c) => c,
            _ => panic!(),
        };
        assert_eq!(cfg.event_type, "add_to_list");
        assert_eq!(cfg.window_hours, 24);
        assert_eq!(cfg.filter_sql, "platform = 'mobile'");
    }

    // -----------------------------------------------------------------------
    // COMPOSITE unit tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn composite_add_two_metrics() {
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                    + MAX(CASE WHEN ms.metric_id = 'session_count_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.experiment_id = '{{ExperimentID}}' \
                     AND ms.metric_id IN ('watch_time_metric', 'session_count_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("add_two_metrics");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = SeedLookup::with_ids(&["watch_time_metric", "session_count_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        assert_eq!(proposal.tier, Tier::Composite);
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::Composite(c) => c,
            _ => panic!(),
        };
        assert_eq!(
            cfg.operator,
            CompositeOperator::Add as i32
        );
        assert_eq!(cfg.operands.len(), 2);
        assert_eq!(cfg.operands[0].metric_id, "watch_time_metric");
        assert_eq!(cfg.operands[1].metric_id, "session_count_metric");
    }

    #[tokio::test]
    async fn composite_divide_two_metrics() {
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (MAX(CASE WHEN ms.metric_id = 'revenue_metric' THEN ms.metric_value END) \
                    / NULLIF(MAX(CASE WHEN ms.metric_id = 'session_count_metric' THEN ms.metric_value END), 0)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.experiment_id = '{{ExperimentID}}' \
                     AND ms.metric_id IN ('revenue_metric', 'session_count_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("divide_two_metrics");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = SeedLookup::with_ids(&["revenue_metric", "session_count_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::Composite(c) => c,
            _ => panic!(),
        };
        assert_eq!(cfg.operator, CompositeOperator::Divide as i32);
        assert_eq!(cfg.operands[0].metric_id, "revenue_metric");
        assert_eq!(cfg.operands[1].metric_id, "session_count_metric");
    }

    #[tokio::test]
    async fn composite_weighted_sum_engagement() {
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (0.7 * MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                    + 0.3 * MAX(CASE WHEN ms.metric_id = 'ctr_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.experiment_id = '{{ExperimentID}}' \
                     AND ms.metric_id IN ('watch_time_metric', 'ctr_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("weighted_sum_engagement");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = SeedLookup::with_ids(&["watch_time_metric", "ctr_metric"]);
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::Composite(c) => c,
            _ => panic!(),
        };
        assert_eq!(cfg.operator, CompositeOperator::WeightedSum as i32);
        assert_eq!(cfg.operands[0].metric_id, "watch_time_metric");
        assert!((cfg.operands[0].weight - 0.7).abs() < 1e-9);
        assert_eq!(cfg.operands[1].metric_id, "ctr_metric");
        assert!((cfg.operands[1].weight - 0.3).abs() < 1e-9);
    }

    // -----------------------------------------------------------------------
    // RatioOfSums — must return None from translate
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn ratio_of_sums_returns_none() {
        use crate::migration::classifier::{extract_shape, parse_or_tier3};
        let sql = "SELECT SUM(revenue) / SUM(sessions) AS arpu FROM events WHERE experiment_id = 'exp1'";
        let stmt = parse_or_tier3(sql).expect("must parse");
        let shape = extract_shape(&stmt);
        assert_eq!(shape, Some(ShapeHint::RatioOfSums));
        // translate with RatioOfSums shape → None
        let lookup = EmptyLookup;
        let result = translate(&stmt, ShapeHint::RatioOfSums, &custom_original("arpu"), &lookup).await;
        assert!(result.is_none(), "RatioOfSums must return None from translate");
    }

    // -----------------------------------------------------------------------
    // Metadata copy test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn metadata_copied_from_original() {
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'video_play' AND me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let mut original = MetricDefinition {
            metric_id: "my_metric".to_string(),
            name: "My Watch Time".to_string(),
            description: "Description here".to_string(),
            lower_is_better: true,
            minimum_detectable_effect: 0.05,
            r#type: MetricType::Custom as i32,
            ..Default::default()
        };
        original.custom_sql = sql.to_string();

        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup).await.unwrap();

        assert_eq!(proposal.metric.metric_id, "my_metric-migrated");
        assert_eq!(proposal.metric.name, "My Watch Time");
        assert_eq!(proposal.metric.description, "Description here");
        assert!(proposal.metric.lower_is_better);
        assert!((proposal.metric.minimum_detectable_effect - 0.05).abs() < 1e-12);
        assert!(proposal.metric.custom_sql.is_empty());
    }

    // -----------------------------------------------------------------------
    // FIX 3: SUM(col)/COUNT(*) alternative for FILTERED_MEAN
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn filtered_mean_sum_div_count_star_projection() {
        // Per plan spec: FILTERED_MEAN accepts SUM(col)/COUNT(*) as well as AVG(col).
        let sql = "SELECT SUM(duration_ms) / COUNT(*) AS metric_value \
                   FROM delta.metric_events \
                   WHERE event_type = 'play'";
        let original = custom_original("sum_div_count_play");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        // SQL has no filter beyond event_type → filter_sql is empty → None.
        // The spec requires filter_sql to be non-empty for FILTERED_MEAN, so this
        // falls through to Tier 3. Verify it does NOT panic and produces None.
        // (A FILTERED_MEAN without a filter is a plain MEAN — conservatism holds.)
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(
            result.is_none(),
            "SUM/COUNT without extra filter → no filter_sql → must return None (use MEAN instead)"
        );
    }

    #[tokio::test]
    async fn filtered_mean_sum_div_count_with_filter_translates() {
        // SUM(col)/COUNT(*) with a real filter predicate → valid FILTERED_MEAN.
        let sql = "SELECT me.user_id, SUM(me.duration_ms) / COUNT(*) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'video_play' AND me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = custom_original("sum_div_count_mobile");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let proposal = translate(&stmt, shape, &original, &lookup)
            .await
            .expect("SUM(col)/COUNT(*) with filter must translate to FILTERED_MEAN");
        assert_eq!(proposal.tier, Tier::Filtered);
        let cfg = match proposal.metric.type_config.as_ref().unwrap() {
            MetricTypeConfig::FilteredMean(c) => c,
            other => panic!("expected FilteredMean, got: {other:?}"),
        };
        assert_eq!(cfg.value_column, "duration_ms");
        assert_eq!(cfg.filter_sql, "platform = 'mobile'");
    }

    // -----------------------------------------------------------------------
    // FIX 1 (regression) — predicates_to_filter_sql must not silently drop LIKE
    //
    // Pre-fix: translator returned Some(FilteredMeanConfig { filter_sql: "" })
    // Post-fix: translator returns None (LIKE is unserializable → bail).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn like_predicate_causes_translator_to_return_none() {
        // The LIKE predicate `me.description LIKE '%HD%'` is unserializable.
        // Pre-fix behavior: filter_map silently dropped it → filter_sql = "event_type = 'play'"
        // Post-fix behavior: returns None (semantic drift avoided).
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   WHERE me.event_type = 'play' AND me.description LIKE '%HD%' \
                   GROUP BY me.user_id";
        let original = custom_original("like_regression");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(
            result.is_none(),
            "LIKE predicate is unserializable — translator must return None, not Some with \
             silent filter drop. Pre-fix would have returned Some(FilteredMeanConfig {{ \
             filter_sql: \"\" }}) which is semantically wrong."
        );
    }

    // -----------------------------------------------------------------------
    // FIX 4: Negative tests
    // -----------------------------------------------------------------------

    /// A FILTERED_MEAN-shaped query with a 3rd JOIN (not exposures/metric_events)
    /// must return None — extra join introduces predicates on unknown tables.
    #[tokio::test]
    async fn filtered_mean_with_third_join_returns_none() {
        // user_profiles is not exposures → from_is_events_with_optional_exposures_join rejects.
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   INNER JOIN delta.exposures eu ON me.user_id = eu.user_id \
                   INNER JOIN delta.user_profiles up ON me.user_id = up.user_id \
                   WHERE me.event_type = 'video_play' AND me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = custom_original("third_join_negative");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(result.is_none(), "3-table join must return None (Tier 3)");
    }

    /// A FILTERED_MEAN-shaped query with no event_type predicate in WHERE must
    /// return None — event_type is required to populate source_event_type.
    #[tokio::test]
    async fn filtered_mean_without_event_type_in_where_returns_none() {
        let sql = "SELECT me.user_id, AVG(me.duration_ms) AS metric_value \
                   FROM delta.metric_events me \
                   WHERE me.platform = 'mobile' \
                   GROUP BY me.user_id";
        let original = custom_original("no_event_type_negative");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(result.is_none(), "missing event_type predicate must return None");
    }

    /// A WINDOWED_COUNT-shaped query without an INTERVAL predicate must return
    /// None — window_hours is mandatory for WindowedCountConfig.
    #[tokio::test]
    async fn windowed_count_without_interval_returns_none() {
        // No `event_timestamp < exposure_ts + INTERVAL` predicate.
        let sql = "SELECT eu.user_id, eu.variant_id, CAST(COUNT(me.user_id) AS DOUBLE) AS metric_value \
                   FROM delta.exposures eu \
                   LEFT JOIN delta.metric_events me \
                     ON eu.user_id = me.user_id \
                     AND me.event_type = 'purchase' \
                   GROUP BY eu.user_id, eu.variant_id";
        let original = custom_original("windowed_no_interval_negative");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = EmptyLookup;
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(result.is_none(), "missing INTERVAL predicate must return None");
    }

    /// A WINDOWED_COUNT with non-exposures/metric_events tables must return None (L4).
    #[tokio::test]
    async fn windowed_count_wrong_tables_returns_none() {
        // FROM orders o LEFT JOIN shipments s — wrong table pair.
        // NOTE: sqlparser may not parse this as WindowedAggregation shape (the
        // classifier looks for INTERVAL in the JOIN ON), so we assert at whichever
        // boundary fires first. The intent is to confirm conservative behavior.
        let sql = "SELECT o.user_id, CAST(COUNT(s.id) AS DOUBLE) AS metric_value \
                   FROM delta.orders o \
                   LEFT JOIN delta.shipments s \
                     ON o.user_id = s.user_id \
                     AND s.event_timestamp >= o.created_at \
                     AND s.event_timestamp < o.created_at + INTERVAL '24' HOUR \
                   GROUP BY o.user_id";
        let original = custom_original("windowed_wrong_tables_negative");
        use crate::migration::classifier::{extract_shape, parse_or_tier3};
        let stmt = parse_or_tier3(sql).expect("must parse");
        let shape = extract_shape(&stmt);
        // If classifier recognizes the shape, the FROM guard must reject it.
        // If classifier returns None, it already fell to Tier 3 — also correct.
        match shape {
            Some(s @ ShapeHint::WindowedAggregation) => {
                let result = translate(&stmt, s, &original, &EmptyLookup).await;
                assert!(
                    result.is_none(),
                    "wrong table pair (orders/shipments) must return None from translate"
                );
            }
            _ => {
                // Classifier already returned None or a different shape — Tier 3 path.
                // Conservative: correct behavior.
            }
        }
    }

    /// A COMPOSITE expression with a single operand (no operator) must return None.
    #[tokio::test]
    async fn composite_single_operand_returns_none() {
        // Only one pivot term — can't form a COMPOSITE with <2 operands.
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.metric_id IN ('watch_time_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("single_operand_negative");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = SeedLookup::with_ids(&["watch_time_metric"]);
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(result.is_none(), "single-operand COMPOSITE must return None");
    }

    /// A COMPOSITE WEIGHTED_SUM where one weight is 0.0 must return None.
    /// The validator rejects zero weights; this confirms the round-trip catches it.
    #[tokio::test]
    async fn composite_weighted_sum_with_zero_weight_returns_none() {
        // 0.0 * A + 0.3 * B — zero weight on the first term.
        let sql = "SELECT ms.user_id, ms.variant_id, \
                   (0.0 * MAX(CASE WHEN ms.metric_id = 'watch_time_metric' THEN ms.metric_value END) \
                    + 0.3 * MAX(CASE WHEN ms.metric_id = 'ctr_metric' THEN ms.metric_value END)) \
                   AS metric_value \
                   FROM delta.metric_summaries ms \
                   WHERE ms.experiment_id = '{{ExperimentID}}' \
                     AND ms.metric_id IN ('watch_time_metric', 'ctr_metric') \
                   GROUP BY ms.user_id, ms.variant_id";
        let original = custom_original("zero_weight_negative");
        let (stmt, shape) = parse_and_shape(sql);
        let lookup = SeedLookup::with_ids(&["watch_time_metric", "ctr_metric"]);
        let result = translate(&stmt, shape, &original, &lookup).await;
        assert!(
            result.is_none(),
            "WEIGHTED_SUM with zero weight must return None (validator rejects zero weights)"
        );
    }
}
