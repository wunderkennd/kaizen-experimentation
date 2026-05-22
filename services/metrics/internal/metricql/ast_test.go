package metricql

import "testing"

// TestAST_AllNodeTypesImplementNodeInterface is a compile-time assertion that
// every concrete AST node type implements the Node interface. If a new node
// type is added without an isNode() / Span() method, this test fails to
// compile -- which is exactly what we want for a closed sum-type-by-convention.
func TestAST_AllNodeTypesImplementNodeInterface(t *testing.T) {
	var _ Node = &Aggregation{}
	var _ Node = &Composite{}
	var _ Node = &Negate{}
	var _ Node = &MetricRef{}
	var _ Node = &Literal{}
	var _ Node = &Ratio{}
}

func TestAST_SpanRoundTrip(t *testing.T) {
	s := Span{Start: 5, End: 12}
	if s.End-s.Start != 7 {
		t.Fatalf("expected length 7, got %d", s.End-s.Start)
	}
}
