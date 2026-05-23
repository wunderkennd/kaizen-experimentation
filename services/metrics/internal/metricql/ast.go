// Package metricql implements the MetricQL expression language (ADR-026 Phase 2).
//
// MetricQL is a hand-rolled, recursive-descent expression language compiled to
// Spark SQL. It covers the ~35% of CUSTOM use cases that Phase 1 structured
// types (FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT) cannot express:
// composed/windowed/filtered metrics with arithmetic and @metric_ref operands.
//
// Pipeline: lexer -> parser -> AST -> analyzer -> codegen -> Spark SQL.
// See docs/adrs/026-custom-metrics-layer.md Phase 2 for the full design.
package metricql

// Node is the marker interface implemented by every AST node type.
// Go lacks sum types; the unexported isNode() method enforces closed enumeration.
type Node interface {
	isNode()
	// Span returns the source position range for error messages.
	Span() Span
}

// Span is a byte-offset range into the original MetricQL source string.
// Half-open: [Start, End).
type Span struct {
	Start, End int
}

// --- Top-level expression nodes ----------------------------------------------

// Aggregation: agg_func '(' source ')' filter? window?
type Aggregation struct {
	Func AggFunc
	// Percentile is the human-friendly 0-100 scale (matches how the source
	// text reads -- percentile(95)(latency.value)). Validity: 0 < Percentile < 100.
	// NOTE convention mismatch with the existing proto field
	// MetricDefinition.percentile which uses 0-1 (0.95). MetricQL uses 0-100
	// in the AST because that's what users write; the codegen template in T6
	// divides by 100 before emitting percentile_approx(col, 0.95) for Spark.
	Percentile float64
	Source     Source
	Filter     *Filter // nil if no where-clause
	Window     *Window // nil if no within-clause
	span       Span
}

func (*Aggregation) isNode()          {}
func (a *Aggregation) Span() Span     { return a.span }
func (a *Aggregation) SetSpan(s Span) { a.span = s }

// AggFunc enumerates the aggregation functions supported by MetricQL.
type AggFunc int

const (
	AggUnknown AggFunc = iota
	AggMean
	AggSum
	AggCount
	AggCountDistinct
	AggProportion
	AggPercentile
)

// --- Composite (arithmetic over leaves, with precedence) --------------------

// Composite is a binary arithmetic node. Children may themselves be
// Composite | Negate | MetricRef | Literal | Ratio.
type Composite struct {
	Op    ArithOp
	Left  Node
	Right Node
	span  Span
}

func (*Composite) isNode()          {}
func (c *Composite) Span() Span     { return c.span }
func (c *Composite) SetSpan(s Span) { c.span = s }

// ArithOp enumerates binary arithmetic operators.
type ArithOp int

const (
	OpUnknown ArithOp = iota
	OpAdd
	OpSub
	OpMul
	OpDiv
)

// Negate is the AST node for unary minus produced by parseUnary (grammar
// production unary := '-'? factor). Wrapping a factor rather than rewriting
// to Literal{0} - x keeps source spans accurate (the span covers the leading
// '-' plus the operand) and lets the renderer emit (-expr) without spurious
// zeros. Lock 2 must carry a node for unary negation or parseUnary has
// nothing to return.
type Negate struct {
	Operand Node
	span    Span
}

func (*Negate) isNode()          {}
func (n *Negate) Span() Span     { return n.span }
func (n *Negate) SetSpan(s Span) { n.span = s }

// MetricRef is an @-prefixed reference to another metric definition.
// The leading '@' is NOT part of ID.
type MetricRef struct {
	ID   string
	span Span
}

func (*MetricRef) isNode()          {}
func (m *MetricRef) Span() Span     { return m.span }
func (m *MetricRef) SetSpan(s Span) { m.span = s }

// Literal is an unsigned numeric literal. Signed values are produced by
// wrapping a Literal in a Negate (grammar production unary := '-'? factor).
type Literal struct {
	Value float64
	span  Span
}

func (*Literal) isNode()          {}
func (l *Literal) Span() Span     { return l.span }
func (l *Literal) SetSpan(s Span) { l.span = s }

// Ratio is the built-in binary aggregator over two referenced metrics.
// First-class to enable variance computation via delta method later.
type Ratio struct {
	Numerator   MetricRef
	Denominator MetricRef
	span        Span
}

func (*Ratio) isNode()          {}
func (r *Ratio) Span() Span     { return r.span }
func (r *Ratio) SetSpan(s Span) { r.span = s }

// --- Sub-nodes (used inside Aggregation) ------------------------------------

// Source identifies the event_type (and optional field) feeding an aggregation.
// Validated against the event catalog by the semantic analyzer.
type Source struct {
	EventType string
	Field     string // "" if not present (count() over events vs mean(heartbeat.value))
	span      Span
}

func (s Source) Span() Span { return s.span }

// SourceWithSpan returns a copy of s with span set to the given Span.
// Kept as a helper because Source is a value type (not a pointer receiver).
func SourceWithSpan(s Source, sp Span) Source { s.span = sp; return s }

// Filter is a chain of predicates joined by implicit AND.
type Filter struct {
	Predicates []Predicate
	span       Span
}

func (f Filter) Span() Span { return f.span }

// FilterWithSpan returns a copy of f with span set to the given Span.
func FilterWithSpan(f Filter, sp Span) Filter { f.span = sp; return f }

// Predicate is a single comparison: field operator value.
type Predicate struct {
	Field    FieldRef
	Operator Op
	Value    Value
	span     Span
}

func (p Predicate) Span() Span { return p.span }

// PredicateWithSpan returns a copy of p with span set to the given Span.
func PredicateWithSpan(p Predicate, sp Span) Predicate { p.span = sp; return p }

// FieldRef is a namespaced field reference. Namespace is "" for top-level
// fields, or one of "properties", "event", "context" for nested fields.
type FieldRef struct {
	Namespace string
	Name      string
}

// Op enumerates predicate comparison operators.
type Op int

const (
	OpEq Op = iota + 1
	OpNeq
	OpLt
	OpLte
	OpGt
	OpGte
	OpIn
)

// Value is a discriminated union -- exactly one of String/Number/List is set.
// IN-list literal: List populated, String/Number nil.
type Value struct {
	String *string
	Number *float64
	List   []Value
	span   Span
}

func (v Value) Span() Span { return v.span }

// ValueWithSpan returns a copy of v with span set to the given Span.
func ValueWithSpan(v Value, sp Span) Value { v.span = sp; return v }

// Window is a "within N hours/days of exposure" clause.
type Window struct {
	N    int
	Unit WindowUnit
	span Span
}

func (w Window) Span() Span { return w.span }

// WindowWithSpan returns a copy of w with span set to the given Span.
func WindowWithSpan(w Window, sp Span) Window { w.span = sp; return w }

// WindowUnit enumerates valid window time units.
type WindowUnit int

const (
	WindowHours WindowUnit = iota + 1
	WindowDays
)
