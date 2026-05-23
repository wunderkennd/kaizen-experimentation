package metricql

import (
	"strings"
	"testing"
)

func TestParser_HappyPath(t *testing.T) {
	cases := []struct {
		name      string
		src       string
		assertion func(t *testing.T, root Node)
	}{
		{
			"mean over field",
			"mean(heartbeat.value)",
			func(t *testing.T, root Node) {
				agg, ok := root.(*Aggregation)
				if !ok {
					t.Fatalf("got %T, want *Aggregation", root)
				}
				if agg.Func != AggMean {
					t.Errorf("func: got %v, want AggMean", agg.Func)
				}
				if agg.Source.EventType != "heartbeat" {
					t.Errorf("event_type: got %q", agg.Source.EventType)
				}
				if agg.Source.Field != "value" {
					t.Errorf("field: got %q", agg.Source.Field)
				}
				if agg.Filter != nil || agg.Window != nil {
					t.Errorf("unexpected filter/window present")
				}
			},
		},
		{
			"composite addition with precedence",
			"0.7 * @a + 0.3 * @b",
			func(t *testing.T, root Node) {
				top, ok := root.(*Composite)
				if !ok {
					t.Fatalf("got %T, want *Composite", root)
				}
				if top.Op != OpAdd {
					t.Errorf("top op: got %v, want OpAdd", top.Op)
				}
				leftMul, ok := top.Left.(*Composite)
				if !ok || leftMul.Op != OpMul {
					t.Fatalf("left subtree: got %T %v, want *Composite OpMul", top.Left, leftMul)
				}
				rightMul, ok := top.Right.(*Composite)
				if !ok || rightMul.Op != OpMul {
					t.Fatalf("right subtree: got %T %v, want *Composite OpMul", top.Right, rightMul)
				}
				lit, ok := leftMul.Left.(*Literal)
				if !ok || lit.Value != 0.7 {
					t.Errorf("left literal: got %T %v, want 0.7", leftMul.Left, lit)
				}
				ref, ok := leftMul.Right.(*MetricRef)
				if !ok || ref.ID != "a" {
					t.Errorf("left metric_ref: got %T %v, want @a", leftMul.Right, ref)
				}
			},
		},
		{
			"ratio",
			"ratio(@total_revenue, @total_sessions)",
			func(t *testing.T, root Node) {
				r, ok := root.(*Ratio)
				if !ok {
					t.Fatalf("got %T, want *Ratio", root)
				}
				if r.Numerator.ID != "total_revenue" {
					t.Errorf("num: got %q", r.Numerator.ID)
				}
				if r.Denominator.ID != "total_sessions" {
					t.Errorf("den: got %q", r.Denominator.ID)
				}
			},
		},
		{
			"windowed count",
			"count(session_start) within 7 days of exposure",
			func(t *testing.T, root Node) {
				agg, ok := root.(*Aggregation)
				if !ok {
					t.Fatalf("got %T, want *Aggregation", root)
				}
				if agg.Func != AggCount {
					t.Errorf("func: got %v, want AggCount", agg.Func)
				}
				if agg.Window == nil {
					t.Fatal("expected non-nil Window")
				}
				if agg.Window.N != 7 || agg.Window.Unit != WindowDays {
					t.Errorf("window: got %+v, want N=7 Unit=Days", *agg.Window)
				}
			},
		},
		{
			"filtered mean",
			"mean(heartbeat.value) where properties.platform = 'mobile'",
			func(t *testing.T, root Node) {
				agg, ok := root.(*Aggregation)
				if !ok {
					t.Fatalf("got %T, want *Aggregation", root)
				}
				if agg.Filter == nil || len(agg.Filter.Predicates) != 1 {
					t.Fatalf("expected 1 predicate, got %v", agg.Filter)
				}
				pred := agg.Filter.Predicates[0]
				if pred.Field.Namespace != "properties" || pred.Field.Name != "platform" {
					t.Errorf("field: got %+v", pred.Field)
				}
				if pred.Operator != OpEq {
					t.Errorf("op: got %v, want OpEq", pred.Operator)
				}
				if pred.Value.String == nil || *pred.Value.String != "mobile" {
					t.Errorf("value: got %+v", pred.Value)
				}
			},
		},
		{
			"in-list",
			"mean(x) where p in ['a', 'b']",
			func(t *testing.T, root Node) {
				agg := root.(*Aggregation)
				if agg.Filter == nil || len(agg.Filter.Predicates) != 1 {
					t.Fatalf("expected 1 predicate, got %+v", agg.Filter)
				}
				pred := agg.Filter.Predicates[0]
				if pred.Operator != OpIn {
					t.Errorf("op: got %v, want OpIn", pred.Operator)
				}
				if len(pred.Value.List) != 2 {
					t.Fatalf("expected 2-element list, got %+v", pred.Value.List)
				}
				if pred.Value.List[0].String == nil || *pred.Value.List[0].String != "a" {
					t.Errorf("first list item: got %+v", pred.Value.List[0])
				}
			},
		},
		{
			"parens override precedence",
			"(0.7 + 0.3) * @a",
			func(t *testing.T, root Node) {
				top, ok := root.(*Composite)
				if !ok || top.Op != OpMul {
					t.Fatalf("top: got %T %v, want *Composite OpMul", root, top)
				}
				leftSum, ok := top.Left.(*Composite)
				if !ok || leftSum.Op != OpAdd {
					t.Fatalf("left subtree: got %T %v, want *Composite OpAdd (parens forced)", top.Left, leftSum)
				}
				ref, ok := top.Right.(*MetricRef)
				if !ok || ref.ID != "a" {
					t.Errorf("right: got %T, want @a", top.Right)
				}
			},
		},
		{
			"percentile aggregation",
			"percentile(95)(latency.value)",
			func(t *testing.T, root Node) {
				agg, ok := root.(*Aggregation)
				if !ok {
					t.Fatalf("got %T, want *Aggregation", root)
				}
				if agg.Func != AggPercentile {
					t.Errorf("func: got %v, want AggPercentile", agg.Func)
				}
				if agg.Percentile != 95 {
					t.Errorf("percentile: got %v, want 95", agg.Percentile)
				}
			},
		},
		{
			"unary minus on metric_ref produces Negate",
			"-@a + @b",
			func(t *testing.T, root Node) {
				top, ok := root.(*Composite)
				if !ok || top.Op != OpAdd {
					t.Fatalf("top: got %T %v, want *Composite OpAdd", root, top)
				}
				neg, ok := top.Left.(*Negate)
				if !ok {
					t.Fatalf("left: got %T, want *Negate (unary minus)", top.Left)
				}
				ref, ok := neg.Operand.(*MetricRef)
				if !ok || ref.ID != "a" {
					t.Errorf("negate operand: got %T %+v, want @a", neg.Operand, ref)
				}
			},
		},
		{
			"binary minus does NOT produce Negate",
			"@a - @b",
			func(t *testing.T, root Node) {
				top, ok := root.(*Composite)
				if !ok || top.Op != OpSub {
					t.Fatalf("top: got %T %v, want *Composite OpSub", root, top)
				}
				if _, isNeg := top.Left.(*Negate); isNeg {
					t.Error("left should be plain MetricRef, not Negate (binary minus)")
				}
				if _, isNeg := top.Right.(*Negate); isNeg {
					t.Error("right should be plain MetricRef, not Negate (binary minus)")
				}
			},
		},
		{
			"multi-predicate filter with AND",
			"mean(x.v) where platform = 'mobile' and country != 'us'",
			func(t *testing.T, root Node) {
				agg := root.(*Aggregation)
				if len(agg.Filter.Predicates) != 2 {
					t.Fatalf("expected 2 predicates, got %d", len(agg.Filter.Predicates))
				}
				if agg.Filter.Predicates[1].Operator != OpNeq {
					t.Errorf("second pred op: got %v, want OpNeq", agg.Filter.Predicates[1].Operator)
				}
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			root, err := Parse(tc.src)
			if err != nil {
				t.Fatalf("unexpected parse error: %v", err)
			}
			tc.assertion(t, root)
		})
	}
}

func TestParser_ErrorMessages(t *testing.T) {
	cases := []struct {
		name, src, wantMsgSubstring string
	}{
		{"missing close paren", "mean(x", "expected ')'"},
		{"empty agg arg", "mean()", "expected event identifier"},
		{"bad operator in filter", "mean(x) where p ~ 1", "unexpected character"}, // '~' fails at lex
		{"trailing tokens", "@a + @b extra", "unexpected trailing tokens"},
		{"raw ident at top", "watch_time", "expected aggregation or composite"},
		{"missing within unit", "count(x) within 7 of exposure", "expected 'hours' or 'days'"},
		{"missing exposure", "count(x) within 7 days of yesterday", "expected keyword \"exposure\""},
		{"percentile out of range", "percentile(0)(x.v)", "percentile must be in"},
		{"empty in-list", "mean(x) where p in []", "at least one value"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := Parse(tc.src)
			if err == nil {
				t.Fatalf("expected error, got nil")
			}
			if !strings.Contains(err.Error(), tc.wantMsgSubstring) {
				t.Errorf("error %q does not contain %q", err.Error(), tc.wantMsgSubstring)
			}
		})
	}
}

func TestParser_Negate_IsTheOnlyMinusSource(t *testing.T) {
	// Invariant: a Negate node only exists when parseUnary consumed a leading '-'.
	// All other minus tokens are Composite{OpSub}. Walk the tree and assert.
	check := func(t *testing.T, src string, wantHasNegate bool) {
		t.Helper()
		root, err := Parse(src)
		if err != nil {
			t.Fatalf("parse %q: %v", src, err)
		}
		has := containsNegate(root)
		if has != wantHasNegate {
			t.Errorf("containsNegate(%q) = %v, want %v", src, has, wantHasNegate)
		}
	}
	check(t, "@a + @b", false)
	check(t, "@a - @b", false)     // binary minus
	check(t, "-@a", true)          // unary minus
	check(t, "-@a + @b", true)     // unary + binary
	check(t, "@a - -@b", true)     // binary then unary
	check(t, "(-@a) * @b", true)   // unary inside parens
	check(t, "@a * @b - 3", false) // binary minus on number
	check(t, "@a * -3", true)      // unary on number literal
}

// containsNegate walks the AST looking for any *Negate node.
func containsNegate(n Node) bool {
	switch v := n.(type) {
	case *Negate:
		return true
	case *Composite:
		return containsNegate(v.Left) || containsNegate(v.Right)
	case *Aggregation, *MetricRef, *Literal, *Ratio:
		return false
	}
	return false
}

func TestParser_Spans_CoverExtent(t *testing.T) {
	// Verify that a top-level Composite's span covers its full source range.
	src := "@a + @b"
	root, err := Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	sp := root.Span()
	if sp.Start != 0 || sp.End != len(src) {
		t.Errorf("root span: got [%d,%d), want [0,%d)", sp.Start, sp.End, len(src))
	}
}
