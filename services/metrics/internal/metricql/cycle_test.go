package metricql

import (
	"strings"
	"testing"
)

// graphLookup builds an OperandLookup over a static in-memory graph.
// Mirrors the GraphLookup struct in composite_cycle.rs tests.
func graphLookup(pairs map[string][]string) OperandLookup {
	return func(id string) ([]string, bool) {
		ops, ok := pairs[id]
		if !ok {
			return nil, false
		}
		return ops, true
	}
}

func TestCycle_LinearTwoNodes_Accepts(t *testing.T) {
	// A -> B (leaf)
	lookup := graphLookup(map[string][]string{"B": {}})
	if err := CheckNoCycles("A", []string{"B"}, lookup); err != nil {
		t.Errorf("expected ok, got: %v", err)
	}
}

func TestCycle_TwoLevelChain_Accepts(t *testing.T) {
	// A -> B -> C (leaf)
	lookup := graphLookup(map[string][]string{
		"B": {"C"},
		"C": {},
	})
	if err := CheckNoCycles("A", []string{"B"}, lookup); err != nil {
		t.Errorf("expected ok, got: %v", err)
	}
}

func TestCycle_DirectSelfReference_Rejects(t *testing.T) {
	// A -> A
	lookup := graphLookup(map[string][]string{})
	err := CheckNoCycles("A", []string{"A"}, lookup)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "self-reference") {
		t.Errorf("error should mention self-reference: %v", err)
	}
}

func TestCycle_TwoNodeCycle_Rejects(t *testing.T) {
	// A -> B -> A
	lookup := graphLookup(map[string][]string{"B": {"A"}})
	err := CheckNoCycles("A", []string{"B"}, lookup)
	if err == nil {
		t.Fatal("expected cycle error")
	}
	if !strings.Contains(err.Error(), "cycle detected") {
		t.Errorf("got: %v", err)
	}
	// The path should trace A -> B -> A
	if !strings.Contains(err.Error(), "A -> B -> A") {
		t.Errorf("error should show path A -> B -> A, got: %v", err)
	}
}

func TestCycle_ThreeNodeCycle_Rejects(t *testing.T) {
	// A -> B -> C -> A
	lookup := graphLookup(map[string][]string{
		"B": {"C"},
		"C": {"A"},
	})
	err := CheckNoCycles("A", []string{"B"}, lookup)
	if err == nil {
		t.Fatal("expected cycle error")
	}
	if !strings.Contains(err.Error(), "cycle") {
		t.Errorf("got: %v", err)
	}
}

func TestCycle_DepthExceeded_Rejects(t *testing.T) {
	// A -> B -> C -> D -> E -> F -> G (depth 6, cap 5)
	lookup := graphLookup(map[string][]string{
		"B": {"C"},
		"C": {"D"},
		"D": {"E"},
		"E": {"F"},
		"F": {"G"},
		"G": {},
	})
	err := CheckNoCycles("A", []string{"B"}, lookup)
	if err == nil {
		t.Fatal("expected depth-cap error")
	}
	if !strings.Contains(err.Error(), "depth") {
		t.Errorf("error should mention depth: %v", err)
	}
	if !strings.Contains(err.Error(), "maximum of 5") {
		t.Errorf("error should mention 'maximum of 5': %v", err)
	}
}

func TestCycle_AtCapDepth_Accepts(t *testing.T) {
	// A -> B -> C -> D -> E -> F (depth 5, cap 5)
	lookup := graphLookup(map[string][]string{
		"B": {"C"},
		"C": {"D"},
		"D": {"E"},
		"E": {"F"},
		"F": {},
	})
	if err := CheckNoCycles("A", []string{"B"}, lookup); err != nil {
		t.Errorf("expected Ok at exactly cap, got: %v", err)
	}
}

func TestCycle_MissingIntermediateOperand_Rejects(t *testing.T) {
	// A -> B, but B is not in the graph (NotFound during DFS).
	lookup := graphLookup(map[string][]string{})
	err := CheckNoCycles("A", []string{"B"}, lookup)
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("error should mention 'not found': %v", err)
	}
}

func TestCycle_DiamondShape_IsNotACycle(t *testing.T) {
	// A -> B, A -> C; B -> D; C -> D. D is shared via separate paths.
	// Once D is BLACK, the second visit short-circuits without flagging a cycle.
	lookup := graphLookup(map[string][]string{
		"B": {"D"},
		"C": {"D"},
		"D": {},
	})
	if err := CheckNoCycles("A", []string{"B", "C"}, lookup); err != nil {
		t.Errorf("diamond is acyclic, got: %v", err)
	}
}

func TestCycle_EmptyOperands_Accepts(t *testing.T) {
	lookup := graphLookup(map[string][]string{})
	if err := CheckNoCycles("A", nil, lookup); err != nil {
		t.Errorf("expected ok, got: %v", err)
	}
}

func TestCycle_DepthCapZero_OnlyEmptyOperandsAccepted(t *testing.T) {
	lookup := graphLookup(map[string][]string{"B": {}})
	if err := checkNoCyclesWithCap("A", nil, lookup, 0); err != nil {
		t.Errorf("cap=0 with empty operands should accept, got: %v", err)
	}
	err := checkNoCyclesWithCap("A", []string{"B"}, lookup, 0)
	if err == nil {
		t.Fatal("cap=0 with non-empty operands should reject")
	}
	if !strings.Contains(err.Error(), "depth 1 exceeds maximum of 0") {
		t.Errorf("got: %v", err)
	}
}

// TestCycle_MaxCompositeDepth_MatchesM5 locks in the parity constraint --
// if M5's DEFAULT_DEPTH_CAP changes from 5, this constant must change too.
// The comment in cycle.go points at the Rust file; this test makes it
// loud at CI time.
func TestCycle_MaxCompositeDepth_MatchesM5(t *testing.T) {
	if MaxCompositeDepth != 5 {
		t.Errorf("MaxCompositeDepth = %d, want 5 (must match M5 DEFAULT_DEPTH_CAP)", MaxCompositeDepth)
	}
}
