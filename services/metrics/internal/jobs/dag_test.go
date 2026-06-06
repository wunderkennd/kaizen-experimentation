package jobs

import (
	"testing"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// mc builds a MetricConfig with a typed MetricDefinition core for tests.
func mc(md *commonv1.MetricDefinition) *config.MetricConfig {
	return &config.MetricConfig{MetricDefinition: md}
}

func TestTopologicalOrder_LinearChain(t *testing.T) {
	// operand=watch_time, composite=engagement_score depending on watch_time
	metrics := []*config.MetricConfig{
		mc(&commonv1.MetricDefinition{
			MetricId: "engagement_score",
			Type:     commonv1.MetricType_METRIC_TYPE_COMPOSITE,
			TypeConfig: &commonv1.MetricDefinition_Composite{
				Composite: &commonv1.CompositeConfig{
					Operands: []*commonv1.CompositeOperand{{MetricId: "watch_time", Weight: 1.0}},
				},
			},
		}),
		mc(&commonv1.MetricDefinition{MetricId: "watch_time", Type: commonv1.MetricType_METRIC_TYPE_MEAN}),
	}

	sorted, skipped, failedParse, err := TopologicalOrder(metrics)
	if len(failedParse) != 0 {
		t.Fatalf("expected no parse failures, got %v", failedParse)
	}
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	if len(sorted) != 2 {
		t.Fatalf("expected 2 sorted, got %d", len(sorted))
	}
	if sorted[0].MetricId != "watch_time" {
		t.Fatalf("expected watch_time first, got %s", sorted[0].MetricId)
	}
	if sorted[1].MetricId != "engagement_score" {
		t.Fatalf("expected engagement_score second, got %s", sorted[1].MetricId)
	}
}

func TestTopologicalOrder_NestedComposite(t *testing.T) {
	// a (MEAN) -> b (COMPOSITE of a) -> c (COMPOSITE of b)
	metrics := []*config.MetricConfig{
		mc(&commonv1.MetricDefinition{
			MetricId: "c",
			Type:     commonv1.MetricType_METRIC_TYPE_COMPOSITE,
			TypeConfig: &commonv1.MetricDefinition_Composite{
				Composite: &commonv1.CompositeConfig{Operands: []*commonv1.CompositeOperand{{MetricId: "b", Weight: 1}}},
			},
		}),
		mc(&commonv1.MetricDefinition{
			MetricId: "b",
			Type:     commonv1.MetricType_METRIC_TYPE_COMPOSITE,
			TypeConfig: &commonv1.MetricDefinition_Composite{
				Composite: &commonv1.CompositeConfig{Operands: []*commonv1.CompositeOperand{{MetricId: "a", Weight: 1}}},
			},
		}),
		mc(&commonv1.MetricDefinition{MetricId: "a", Type: commonv1.MetricType_METRIC_TYPE_MEAN}),
	}
	sorted, skipped, failedParse, _ := TopologicalOrder(metrics)
	if len(failedParse) != 0 {
		t.Fatalf("expected no parse failures, got %v", failedParse)
	}
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	got := []string{sorted[0].MetricId, sorted[1].MetricId, sorted[2].MetricId}
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
		mc(&commonv1.MetricDefinition{
			MetricId: "a",
			Type:     commonv1.MetricType_METRIC_TYPE_COMPOSITE,
			TypeConfig: &commonv1.MetricDefinition_Composite{
				Composite: &commonv1.CompositeConfig{Operands: []*commonv1.CompositeOperand{{MetricId: "b", Weight: 1}}},
			},
		}),
		mc(&commonv1.MetricDefinition{
			MetricId: "b",
			Type:     commonv1.MetricType_METRIC_TYPE_COMPOSITE,
			TypeConfig: &commonv1.MetricDefinition_Composite{
				Composite: &commonv1.CompositeConfig{Operands: []*commonv1.CompositeOperand{{MetricId: "a", Weight: 1}}},
			},
		}),
		mc(&commonv1.MetricDefinition{MetricId: "c", Type: commonv1.MetricType_METRIC_TYPE_MEAN}),
	}
	sorted, skipped, failedParse, err := TopologicalOrder(metrics)
	if len(failedParse) != 0 {
		t.Fatalf("expected no parse failures, got %v", failedParse)
	}
	if err != nil {
		t.Fatalf("expected no error (cycles are reported via skipped map), got %v", err)
	}
	if !skipped["a"] || !skipped["b"] {
		t.Fatalf("expected a + b skipped (cycle), got %v", skipped)
	}
	if len(sorted) != 1 || sorted[0].MetricId != "c" {
		t.Fatalf("expected only c sorted, got %v", sorted)
	}
}

// TestTopologicalOrder_MetricqlChain verifies that a METRICQL metric's
// @metric_refs feed the DAG just like a COMPOSITE's Operands -- a METRICQL
// expression "0.7 * @watch_time + 0.3 * @ctr" must land after both operands
// in topo order. ADR-026 Phase 2 (#435).
func TestTopologicalOrder_MetricqlChain(t *testing.T) {
	metrics := []*config.MetricConfig{
		mc(&commonv1.MetricDefinition{
			MetricId:           "weighted",
			Type:               commonv1.MetricType_METRIC_TYPE_METRICQL,
			MetricqlExpression: "0.7 * @watch_time + 0.3 * @ctr",
		}),
		mc(&commonv1.MetricDefinition{
			MetricId:        "watch_time",
			Type:            commonv1.MetricType_METRIC_TYPE_MEAN,
			SourceEventType: "heartbeat",
		}),
		mc(&commonv1.MetricDefinition{
			MetricId:        "ctr",
			Type:            commonv1.MetricType_METRIC_TYPE_PROPORTION,
			SourceEventType: "click",
		}),
	}
	sorted, skipped, failedParse, err := TopologicalOrder(metrics)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	if len(failedParse) != 0 {
		t.Fatalf("expected no parse failures, got %v", failedParse)
	}
	if len(sorted) != 3 {
		t.Fatalf("expected 3 sorted, got %d: %v", len(sorted), sorted)
	}
	if sorted[2].MetricId != "weighted" {
		ids := []string{sorted[0].MetricId, sorted[1].MetricId, sorted[2].MetricId}
		t.Fatalf("weighted must be last in topo order; got %v", ids)
	}
}

// TestTopologicalOrder_MetricqlParseFailure verifies that a METRICQL metric
// with malformed source text is reported in `failedParse` and excluded from
// edge-building, while the rest of the pass proceeds normally.
func TestTopologicalOrder_MetricqlParseFailure(t *testing.T) {
	metrics := []*config.MetricConfig{
		mc(&commonv1.MetricDefinition{
			MetricId:           "bad",
			Type:               commonv1.MetricType_METRIC_TYPE_METRICQL,
			MetricqlExpression: "mean( oops", // intentional syntax error
		}),
		mc(&commonv1.MetricDefinition{
			MetricId:        "good",
			Type:            commonv1.MetricType_METRIC_TYPE_MEAN,
			SourceEventType: "heartbeat",
		}),
	}
	sorted, skipped, failedParse, err := TopologicalOrder(metrics)
	if err != nil {
		t.Fatalf("expected no error (parse failures are reported via failedParse), got %v", err)
	}
	if len(skipped) != 0 {
		t.Fatalf("expected no cycle-skipped, got %v", skipped)
	}
	if _, ok := failedParse["bad"]; !ok {
		t.Fatalf("expected 'bad' in failedParse, got %v", failedParse)
	}
	if len(sorted) != 2 {
		t.Fatalf("expected both metrics in sorted output (failed-parse landing in in-degree 0), got %d", len(sorted))
	}
}

func TestTopologicalOrder_OperandOutsidePass(t *testing.T) {
	// c references operand x that's not in this scheduling pass — c remains
	// in-degree 0 (Kahn's emits it). The caller's status_map gates skipping on
	// operand status at runtime.
	metrics := []*config.MetricConfig{
		mc(&commonv1.MetricDefinition{
			MetricId: "c",
			Type:     commonv1.MetricType_METRIC_TYPE_COMPOSITE,
			TypeConfig: &commonv1.MetricDefinition_Composite{
				Composite: &commonv1.CompositeConfig{Operands: []*commonv1.CompositeOperand{{MetricId: "x", Weight: 1}}},
			},
		}),
	}
	sorted, skipped, failedParse, _ := TopologicalOrder(metrics)
	if len(failedParse) != 0 {
		t.Fatalf("expected no parse failures, got %v", failedParse)
	}
	if len(skipped) != 0 {
		t.Fatalf("expected no skipped, got %v", skipped)
	}
	if len(sorted) != 1 || sorted[0].MetricId != "c" {
		t.Fatalf("expected c sorted, got %v", sorted)
	}
}
