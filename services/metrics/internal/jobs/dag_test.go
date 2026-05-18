package jobs

import (
	"testing"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
)

func TestTopologicalOrder_LinearChain(t *testing.T) {
	// operand=watch_time, composite=engagement_score depending on watch_time
	metrics := []*config.MetricConfig{
		{MetricID: "engagement_score", Type: "COMPOSITE", Operands: []config.OperandConfig{
			{MetricID: "watch_time", Weight: 1.0},
		}},
		{MetricID: "watch_time", Type: "MEAN"},
	}

	sorted, skipped, err := TopologicalOrder(metrics)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	if len(sorted) != 2 {
		t.Fatalf("expected 2 sorted, got %d", len(sorted))
	}
	if sorted[0].MetricID != "watch_time" {
		t.Fatalf("expected watch_time first, got %s", sorted[0].MetricID)
	}
	if sorted[1].MetricID != "engagement_score" {
		t.Fatalf("expected engagement_score second, got %s", sorted[1].MetricID)
	}
}

func TestTopologicalOrder_NestedComposite(t *testing.T) {
	// a (MEAN) -> b (COMPOSITE of a) -> c (COMPOSITE of b)
	metrics := []*config.MetricConfig{
		{MetricID: "c", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "b", Weight: 1}}},
		{MetricID: "b", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "a", Weight: 1}}},
		{MetricID: "a", Type: "MEAN"},
	}
	sorted, skipped, _ := TopologicalOrder(metrics)
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	got := []string{sorted[0].MetricID, sorted[1].MetricID, sorted[2].MetricID}
	want := []string{"a", "b", "c"}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("position %d: want %s, got %s (full: %v)", i, want[i], got[i], got)
		}
	}
}

func TestTopologicalOrder_CycleIsSkipped(t *testing.T) {
	// a -> b -> a (cycle); c is independent and should still be sorted.
	metrics := []*config.MetricConfig{
		{MetricID: "a", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "b", Weight: 1}}},
		{MetricID: "b", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "a", Weight: 1}}},
		{MetricID: "c", Type: "MEAN"},
	}
	sorted, skipped, err := TopologicalOrder(metrics)
	if err != nil {
		t.Fatalf("expected no error (cycles are reported via skipped map), got %v", err)
	}
	if !skipped["a"] || !skipped["b"] {
		t.Fatalf("expected a + b skipped (cycle), got %v", skipped)
	}
	if len(sorted) != 1 || sorted[0].MetricID != "c" {
		t.Fatalf("expected only c sorted, got %v", sorted)
	}
}

func TestTopologicalOrder_LowercaseCompositeType(t *testing.T) {
	// Devin BUG-0001 regression on #556: the loader / renderer / scheduler all
	// normalize Type via strings.ToUpper, so a config with "composite" must
	// build the same DAG as "COMPOSITE". Before the fix, edges weren't built
	// for lowercase entries — the COMPOSITE landed before its operands in
	// topo order and was wrongly marked SkippedUpstreamFailure at runtime.
	metrics := []*config.MetricConfig{
		{MetricID: "engagement", Type: "composite", Operands: []config.OperandConfig{
			{MetricID: "watch_time", Weight: 1.0},
		}},
		{MetricID: "watch_time", Type: "mean"},
	}
	sorted, skipped, err := TopologicalOrder(metrics)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	if len(sorted) != 2 || sorted[0].MetricID != "watch_time" || sorted[1].MetricID != "engagement" {
		ids := []string{sorted[0].MetricID, sorted[1].MetricID}
		t.Fatalf("expected [watch_time, engagement], got %v", ids)
	}
}

func TestTopologicalOrder_OperandOutsidePass(t *testing.T) {
	// c references operand x that's not in this scheduling pass — c remains
	// in-degree 0 (Kahn's emits it). The caller's status_map gates skipping on
	// operand status at runtime.
	metrics := []*config.MetricConfig{
		{MetricID: "c", Type: "COMPOSITE", Operands: []config.OperandConfig{{MetricID: "x", Weight: 1}}},
	}
	sorted, skipped, _ := TopologicalOrder(metrics)
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	if len(sorted) != 1 || sorted[0].MetricID != "c" {
		t.Fatalf("expected c sorted, got %v", sorted)
	}
}
