//! Shared `sqlparser::ast` helpers for the CUSTOM-metric migration tiers.
//!
//! Extracted from `tier1.rs` in #587: these helpers manipulate AST primitives
//! (`Expr`, `Select`, `Statement`, `TableWithJoins`) and are called from both
//! `tier1.rs` and `tier2.rs` at roughly even rates (37 / 33 references). Hosting
//! them in a sibling module makes `tier1` and `tier2` true siblings instead of
//! one implicitly depending on the other.
//!
//! Pure code-motion — no behavior change.

use sqlparser::ast::{
    BinaryOperator, Expr, JoinConstraint, JoinOperator, SetExpr, Statement,
};

// ---------------------------------------------------------------------------
// FROM-clause shape gates
// ---------------------------------------------------------------------------

/// Validate that the FILTERED_MEAN FROM clause matches one of:
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

// ---------------------------------------------------------------------------
// Statement / Expr utilities
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
