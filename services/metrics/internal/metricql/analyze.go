package metricql

import (
	"fmt"
	"regexp"
)

// AnalyzeContext supplies external knowledge to the semantic analyzer.
//
// Phase 1 punt: no event catalog. AnalyzeContext only knows about metric IDs.
// When the event catalog service ships, EventTypeCatalog will be added here
// and event_type / field existence will be checked against it.
type AnalyzeContext struct {
	// KnownMetricIDs is the set of metric IDs that exist in M5's store.
	// If nil, the analyzer skips @metric_ref existence checks (M5 may pass
	// nil at very-first-creation time and gate this check at update time).
	KnownMetricIDs map[string]bool
}

// identifierRegex matches the Lock 1 lowercase identifier rule.
// Pre-compiled at package init -- runtime regex compilation is wasteful.
var identifierRegex = regexp.MustCompile(`^[a-z_][a-z0-9_]*$`)

// AnalyzeError is a span-tagged semantic-analysis error.
type AnalyzeError struct {
	Span    Span
	Message string
}

func (e *AnalyzeError) Error() string {
	return fmt.Sprintf("metricql analyze error at offset %d: %s", e.Span.Start, e.Message)
}

// Analyze runs all semantic checks against a parsed AST. Does NOT generate SQL.
// Returns nil on success or *AnalyzeError on the first failure encountered.
//
// The analyzer is the primary rejection site for semantic errors -- codegen's
// lower() also defensively rejects bare refs/literals, but the analyzer fails
// faster and with a Span suitable for inline editor display.
func Analyze(root Node, ctx AnalyzeContext) error {
	// Top-level rejection: a metric definition must be an aggregation or
	// arithmetic expression. Bare @ref or NUMBER is grammar-legal but
	// semantically nonsensical as a standalone definition.
	switch root.(type) {
	case *MetricRef:
		return &AnalyzeError{
			Span:    root.Span(),
			Message: "a metric definition must be an aggregation or arithmetic expression, not a bare metric reference",
		}
	case *Literal:
		return &AnalyzeError{
			Span:    root.Span(),
			Message: "a metric definition must be an aggregation or arithmetic expression, not a bare literal",
		}
	}
	return analyzeNode(root, ctx)
}

// analyzeNode is the recursive walker. Visits every node and applies
// per-node-type checks.
func analyzeNode(n Node, ctx AnalyzeContext) error {
	switch v := n.(type) {
	case *Aggregation:
		return analyzeAggregation(v, ctx)
	case *Composite:
		if err := analyzeNode(v.Left, ctx); err != nil {
			return err
		}
		return analyzeNode(v.Right, ctx)
	case *Negate:
		return analyzeNode(v.Operand, ctx)
	case *MetricRef:
		return analyzeMetricRef(v, ctx)
	case *Literal:
		// Literals carry no analyzer-relevant state; the parser already
		// guarantees they are valid float64s.
		return nil
	case *Ratio:
		if err := analyzeMetricRef(&v.Numerator, ctx); err != nil {
			return err
		}
		return analyzeMetricRef(&v.Denominator, ctx)
	}
	return &AnalyzeError{
		Span:    n.Span(),
		Message: fmt.Sprintf("unknown AST node type %T", n),
	}
}

func analyzeAggregation(a *Aggregation, ctx AnalyzeContext) error {
	// Source.EventType: required, must match identifier regex.
	if !identifierRegex.MatchString(a.Source.EventType) {
		return &AnalyzeError{
			Span:    a.Source.Span(),
			Message: fmt.Sprintf("invalid event_type %q: must match [a-z_][a-z0-9_]*", a.Source.EventType),
		}
	}
	// Source.Field: optional, must match identifier regex if present.
	if a.Source.Field != "" && !identifierRegex.MatchString(a.Source.Field) {
		return &AnalyzeError{
			Span:    a.Source.Span(),
			Message: fmt.Sprintf("invalid field %q: must match [a-z_][a-z0-9_]*", a.Source.Field),
		}
	}
	// Percentile aggregations: defense-in-depth on the (0, 100) range.
	if a.Func == AggPercentile && (a.Percentile <= 0 || a.Percentile >= 100) {
		return &AnalyzeError{
			Span:    a.Span(),
			Message: fmt.Sprintf("percentile must be in (0, 100), got %v", a.Percentile),
		}
	}
	// Count / Proportion aggregate over event presence, not a value. A field
	// argument is meaningless and is rejected at semantic time.
	// (Devin BUG-0001/0002 on PR #559 -- locked in here.)
	if (a.Func == AggCount || a.Func == AggProportion) && a.Source.Field != "" {
		return &AnalyzeError{
			Span: a.Source.Span(),
			Message: fmt.Sprintf("%s aggregation operates on event presence; remove the .%s field reference (e.g., %s(%s) not %s(%s.%s))",
				aggFuncName(a.Func), a.Source.Field,
				aggFuncName(a.Func), a.Source.EventType,
				aggFuncName(a.Func), a.Source.EventType, a.Source.Field),
		}
	}
	// Mean / Sum / CountDistinct / Percentile aggregate over a value, so a
	// field is required. count_distinct(purchase.product_id) valid;
	// count_distinct(stream_start) rejected.
	if (a.Func == AggMean || a.Func == AggSum || a.Func == AggCountDistinct || a.Func == AggPercentile) && a.Source.Field == "" {
		return &AnalyzeError{
			Span: a.Source.Span(),
			Message: fmt.Sprintf("%s aggregation requires a value field (e.g., %s(%s.<field>))",
				aggFuncName(a.Func), aggFuncName(a.Func), a.Source.EventType),
		}
	}

	// Filter: walk predicates and validate field identifiers.
	if a.Filter != nil {
		for _, pred := range a.Filter.Predicates {
			if pred.Field.Namespace != "" && !identifierRegex.MatchString(pred.Field.Namespace) {
				return &AnalyzeError{
					Span:    pred.Span(),
					Message: fmt.Sprintf("invalid field namespace %q: must match [a-z_][a-z0-9_]*", pred.Field.Namespace),
				}
			}
			if !identifierRegex.MatchString(pred.Field.Name) {
				return &AnalyzeError{
					Span:    pred.Span(),
					Message: fmt.Sprintf("invalid field name %q: must match [a-z_][a-z0-9_]*", pred.Field.Name),
				}
			}
		}
	}

	// Window: defense-in-depth -- parser already enforces N > 0 and a known unit.
	if a.Window != nil {
		if a.Window.N <= 0 {
			return &AnalyzeError{
				Span:    a.Window.Span(),
				Message: fmt.Sprintf("window size must be positive, got %d", a.Window.N),
			}
		}
		if a.Window.Unit != WindowHours && a.Window.Unit != WindowDays {
			return &AnalyzeError{
				Span:    a.Window.Span(),
				Message: fmt.Sprintf("unknown window unit %v", a.Window.Unit),
			}
		}
	}

	return nil
}

func analyzeMetricRef(m *MetricRef, ctx AnalyzeContext) error {
	if !identifierRegex.MatchString(m.ID) {
		return &AnalyzeError{
			Span:    m.Span(),
			Message: fmt.Sprintf("invalid metric reference @%s: must match [a-z_][a-z0-9_]*", m.ID),
		}
	}
	if ctx.KnownMetricIDs != nil {
		if !ctx.KnownMetricIDs[m.ID] {
			return &AnalyzeError{
				Span:    m.Span(),
				Message: fmt.Sprintf("unknown metric reference @%s (not found in metric definitions store)", m.ID),
			}
		}
	}
	return nil
}

// aggFuncName returns the source-text name of an AggFunc for error messages.
func aggFuncName(f AggFunc) string {
	switch f {
	case AggMean:
		return "mean"
	case AggSum:
		return "sum"
	case AggCount:
		return "count"
	case AggCountDistinct:
		return "count_distinct"
	case AggProportion:
		return "proportion"
	case AggPercentile:
		return "percentile"
	}
	return fmt.Sprintf("AggFunc(%d)", int(f))
}

// CollectMetricRefs walks the AST and returns the deduplicated list of
// @metric_ref IDs referenced anywhere in the tree. Used by T7 (DAG operand
// extraction) and T5 (cycle detection seed).
func CollectMetricRefs(root Node) []string {
	seen := map[string]struct{}{}
	var walk func(Node)
	walk = func(n Node) {
		switch v := n.(type) {
		case *Aggregation:
			// Aggregations have no metric refs by construction.
			_ = v
		case *Composite:
			walk(v.Left)
			walk(v.Right)
		case *Negate:
			walk(v.Operand)
		case *MetricRef:
			seen[v.ID] = struct{}{}
		case *Literal:
			// no-op
		case *Ratio:
			seen[v.Numerator.ID] = struct{}{}
			seen[v.Denominator.ID] = struct{}{}
		}
	}
	walk(root)
	out := make([]string, 0, len(seen))
	for id := range seen {
		out = append(out, id)
	}
	return out
}
