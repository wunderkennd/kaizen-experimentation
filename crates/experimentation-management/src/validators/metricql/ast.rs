//! MetricQL AST node types — direct mechanical port of
//! `services/metrics/internal/metricql/ast.go` (ADR-026 Phase 2 / #436).
//!
//! Only type definitions live here. No parsing, lexing, or analysis — those
//! land in A2/A3/A4 in their own submodules.

// ---------------------------------------------------------------------------
// Span
// ---------------------------------------------------------------------------

/// Half-open `[start, end)` byte-offset range into the original MetricQL
/// source string. Mirrors Go `Span{Start, End int}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

// ---------------------------------------------------------------------------
// Top-level Node enum (Rust sum type — replaces Go marker interface)
// ---------------------------------------------------------------------------

/// Every MetricQL expression tree node. Mirrors the closed set of types that
/// implement Go's `Node` marker interface.
///
/// `Negate` and `Composite` are boxed because they contain `Node` children,
/// making the type recursive. The other leaf-like variants carry plain structs.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Aggregation(Aggregation),
    Composite(Box<Composite>),
    Negate(Box<Negate>),
    Ratio(Ratio),
    MetricRef(MetricRef),
    Literal(Literal),
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// `agg_func '(' source ')' filter? window?`
///
/// Mirrors Go `Aggregation` struct. `filter` and `window` are `Option`
/// (nil-able pointer fields on the Go side).
#[derive(Debug, Clone, PartialEq)]
pub struct Aggregation {
    pub func: AggFunc,
    /// Percentile on the 0–100 scale as written by the user (e.g. `95` for
    /// the 95th percentile). The codegen layer divides by 100 before emitting
    /// `percentile_approx(col, 0.95)` for Spark.
    pub percentile: f64,
    pub source: Source,
    pub filter: Option<Filter>,
    pub window: Option<Window>,
    pub span: Span,
}

/// Aggregation functions supported by MetricQL.
/// Mirrors Go `AggFunc int` / `Agg*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggFunc {
    Unknown,
    Mean,
    Sum,
    Count,
    CountDistinct,
    Proportion,
    Percentile,
}

// ---------------------------------------------------------------------------
// Composite (binary arithmetic)
// ---------------------------------------------------------------------------

/// Binary arithmetic node. Children may themselves be any `Node` variant.
/// Both `left` and `right` are boxed because `Node` is the recursive type.
///
/// Mirrors Go `Composite` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Composite {
    pub op: ArithOp,
    pub left: Box<Node>,
    pub right: Box<Node>,
    pub span: Span,
}

/// Binary arithmetic operators. Mirrors Go `ArithOp int` / `Op*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Unknown,
    Add,
    Sub,
    Mul,
    Div,
}

// ---------------------------------------------------------------------------
// Negate (unary minus)
// ---------------------------------------------------------------------------

/// Unary negation node. Wraps any factor-level `Node`. The `Box<Negate>` at
/// the `Node::Negate` variant breaks the recursive size cycle.
///
/// Mirrors Go `Negate` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Negate {
    pub operand: Node,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// MetricRef
// ---------------------------------------------------------------------------

/// An `@`-prefixed reference to another metric definition. The leading `@` is
/// NOT included in `id`. Mirrors Go `MetricRef` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricRef {
    pub id: String,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Literal
// ---------------------------------------------------------------------------

/// An unsigned numeric literal. Signed values are produced by wrapping a
/// `Literal` inside a `Negate`. Mirrors Go `Literal` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub value: f64,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Ratio
// ---------------------------------------------------------------------------

/// Built-in binary aggregator over two referenced metrics.
/// First-class to enable variance computation via the delta method later.
///
/// Mirrors Go `Ratio` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Ratio {
    pub numerator: MetricRef,
    pub denominator: MetricRef,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Sub-nodes (used inside Aggregation)
// ---------------------------------------------------------------------------

/// Identifies the event type (and optional field) feeding an aggregation.
/// `field` is `""` when not present (e.g. `count()` over events vs.
/// `mean(heartbeat.value)`). Mirrors Go `Source` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub event_type: String,
    /// Empty string means no `.field` suffix.
    pub field: String,
    pub span: Span,
}

/// A chain of predicates joined by implicit AND.
/// Mirrors Go `Filter` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub predicates: Vec<Predicate>,
    pub span: Span,
}

/// A single comparison: `field operator value`.
/// Mirrors Go `Predicate` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    pub field: FieldRef,
    pub operator: Op,
    pub value: Value,
    pub span: Span,
}

/// A namespaced field reference. `namespace` is `""` for top-level fields, or
/// one of `"properties"`, `"event"`, `"context"` for nested fields.
/// Mirrors Go `FieldRef` struct. No `span` on the Go side — not added here.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldRef {
    pub namespace: String,
    pub name: String,
}

/// Predicate comparison operators.
/// Mirrors Go `Op int` / `Op*` constants. Note: Go's `OpEq` starts at `1`,
/// but in Rust we use a plain enum — the discriminant values are not exposed
/// externally, so we start at the logical first member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
}

// ---------------------------------------------------------------------------
// Value (discriminated union — idiomatic Rust enum form)
// ---------------------------------------------------------------------------

/// The payload of a predicate value. Exactly one variant is active.
///
/// Go uses `struct Value { String *string; Number *float64; List []Value }`
/// with only one field set at a time. The Rust equivalent encodes this
/// invariant structurally via an inner `enum ValueKind`, preventing any
/// "two fields set simultaneously" bug at the type level.
#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub kind: ValueKind,
    pub span: Span,
}

/// Inner discriminant for `Value`.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueKind {
    String(String),
    Number(f64),
    List(Vec<Value>),
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

/// A "within N hours/days of exposure" clause.
/// Mirrors Go `Window` struct. `n` is `u32` (Go uses `int` but the value is
/// always non-negative; `u32` makes the non-negativity invariant structural).
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub n: u32,
    pub unit: WindowUnit,
    pub span: Span,
}

/// Valid window time units. Mirrors Go `WindowUnit int` / `Window*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowUnit {
    Hours,
    Days,
}

// ---------------------------------------------------------------------------
// Smoke tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::new(0, 5)
    }

    /// Return a canonical name for each `Node` variant. The match is
    /// exhaustive — no wildcard — so the compiler will error if a variant is
    /// missing, proving the enum is closed and complete.
    fn node_name(n: &Node) -> &'static str {
        match n {
            Node::Aggregation(_) => "Aggregation",
            Node::Composite(_) => "Composite",
            Node::Negate(_) => "Negate",
            Node::Ratio(_) => "Ratio",
            Node::MetricRef(_) => "MetricRef",
            Node::Literal(_) => "Literal",
        }
    }

    fn make_metric_ref(id: &str) -> MetricRef {
        MetricRef { id: id.to_string(), span: span() }
    }

    fn make_literal(value: f64) -> Node {
        Node::Literal(Literal { value, span: span() })
    }

    #[test]
    fn span_construction() {
        let s = Span::new(3, 10);
        assert_eq!(s.start, 3);
        assert_eq!(s.end, 10);
    }

    #[test]
    fn all_node_variants_constructible_and_named() {
        // Aggregation
        let agg = Node::Aggregation(Aggregation {
            func: AggFunc::Mean,
            percentile: 0.0,
            source: Source {
                event_type: "heartbeat".to_string(),
                field: "value".to_string(),
                span: span(),
            },
            filter: Some(Filter {
                predicates: vec![Predicate {
                    field: FieldRef {
                        namespace: "properties".to_string(),
                        name: "platform".to_string(),
                    },
                    operator: Op::Eq,
                    value: Value {
                        kind: ValueKind::String("mobile".to_string()),
                        span: span(),
                    },
                    span: span(),
                }],
                span: span(),
            }),
            window: Some(Window { n: 7, unit: WindowUnit::Days, span: span() }),
            span: span(),
        });
        assert_eq!(node_name(&agg), "Aggregation");

        // Composite (boxed)
        let composite = Node::Composite(Box::new(Composite {
            op: ArithOp::Add,
            left: Box::new(make_literal(1.0)),
            right: Box::new(make_literal(2.0)),
            span: span(),
        }));
        assert_eq!(node_name(&composite), "Composite");

        // Negate (boxed)
        let negate = Node::Negate(Box::new(Negate {
            operand: make_literal(3.0),
            span: span(),
        }));
        assert_eq!(node_name(&negate), "Negate");

        // Ratio
        let ratio = Node::Ratio(Ratio {
            numerator: make_metric_ref("revenue"),
            denominator: make_metric_ref("sessions"),
            span: span(),
        });
        assert_eq!(node_name(&ratio), "Ratio");

        // MetricRef
        let metric_ref = Node::MetricRef(make_metric_ref("watch_time"));
        assert_eq!(node_name(&metric_ref), "MetricRef");

        // Literal
        let literal = make_literal(42.0);
        assert_eq!(node_name(&literal), "Literal");
    }

    #[test]
    fn all_node_variants_carry_span() {
        // Verify that every Node variant can expose its span via inner field access.
        let s = Span::new(1, 9);

        let nodes: Vec<Node> = vec![
            Node::Aggregation(Aggregation {
                func: AggFunc::Count,
                percentile: 0.0,
                source: Source { event_type: "e".into(), field: String::new(), span: s.clone() },
                filter: None,
                window: None,
                span: s.clone(),
            }),
            Node::Composite(Box::new(Composite {
                op: ArithOp::Mul,
                left: Box::new(make_literal(1.0)),
                right: Box::new(make_literal(2.0)),
                span: s.clone(),
            })),
            Node::Negate(Box::new(Negate { operand: make_literal(5.0), span: s.clone() })),
            Node::Ratio(Ratio {
                numerator: MetricRef { id: "a".into(), span: s.clone() },
                denominator: MetricRef { id: "b".into(), span: s.clone() },
                span: s.clone(),
            }),
            Node::MetricRef(MetricRef { id: "x".into(), span: s.clone() }),
            Node::Literal(Literal { value: 0.0, span: s.clone() }),
        ];

        for node in &nodes {
            let node_span = match node {
                Node::Aggregation(n) => &n.span,
                Node::Composite(n) => &n.span,
                Node::Negate(n) => &n.span,
                Node::Ratio(n) => &n.span,
                Node::MetricRef(n) => &n.span,
                Node::Literal(n) => &n.span,
            };
            assert_eq!(node_span, &s, "span mismatch for {:?}", node_name(node));
        }
    }

    #[test]
    fn aggfunc_variants_all_constructible() {
        let funcs = [
            AggFunc::Unknown,
            AggFunc::Mean,
            AggFunc::Sum,
            AggFunc::Count,
            AggFunc::CountDistinct,
            AggFunc::Proportion,
            AggFunc::Percentile,
        ];
        // Just assert they're all distinct (no duplicate repr).
        for (i, a) in funcs.iter().enumerate() {
            for (j, b) in funcs.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn arithop_variants_all_constructible() {
        let ops = [ArithOp::Unknown, ArithOp::Add, ArithOp::Sub, ArithOp::Mul, ArithOp::Div];
        for (i, a) in ops.iter().enumerate() {
            for (j, b) in ops.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn op_variants_all_constructible() {
        let ops = [Op::Eq, Op::Neq, Op::Lt, Op::Lte, Op::Gt, Op::Gte, Op::In];
        for (i, a) in ops.iter().enumerate() {
            for (j, b) in ops.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn window_unit_variants_all_constructible() {
        assert_ne!(WindowUnit::Hours, WindowUnit::Days);
    }

    #[test]
    fn value_kind_string_variant() {
        let v = Value {
            kind: ValueKind::String("hello".to_string()),
            span: span(),
        };
        assert!(matches!(v.kind, ValueKind::String(_)));
    }

    #[test]
    fn value_kind_number_variant() {
        let v = Value { kind: ValueKind::Number(3.14), span: span() };
        assert!(matches!(v.kind, ValueKind::Number(_)));
    }

    #[test]
    fn value_kind_list_variant() {
        let inner = Value { kind: ValueKind::String("a".into()), span: span() };
        let v = Value { kind: ValueKind::List(vec![inner]), span: span() };
        assert!(matches!(v.kind, ValueKind::List(_)));
    }

    #[test]
    fn source_empty_field_means_no_field_suffix() {
        let s = Source { event_type: "play".into(), field: String::new(), span: span() };
        assert!(s.field.is_empty());
    }

    #[test]
    fn fieldref_empty_namespace_means_top_level() {
        let f = FieldRef { namespace: String::new(), name: "country".into() };
        assert!(f.namespace.is_empty());
    }

    #[test]
    fn ratio_numerator_denominator_distinct() {
        let r = Ratio {
            numerator: make_metric_ref("revenue"),
            denominator: make_metric_ref("sessions"),
            span: span(),
        };
        assert_ne!(r.numerator.id, r.denominator.id);
    }

    #[test]
    fn negate_wraps_any_node() {
        // Wrapping a Composite inside Negate exercises the recursive structure.
        let inner = Node::Composite(Box::new(Composite {
            op: ArithOp::Sub,
            left: Box::new(make_literal(10.0)),
            right: Box::new(make_literal(3.0)),
            span: span(),
        }));
        let neg = Node::Negate(Box::new(Negate { operand: inner, span: span() }));
        assert_eq!(node_name(&neg), "Negate");
    }

    #[test]
    fn node_clone_and_partial_eq() {
        let n = make_literal(7.0);
        let m = n.clone();
        assert_eq!(n, m);
    }
}
