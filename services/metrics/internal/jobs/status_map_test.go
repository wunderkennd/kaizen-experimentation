package jobs

import (
	"testing"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/status"
)

func TestStatusMap_BlockerForOperands(t *testing.T) {
	sm := newStatusMap()
	sm.markCompleted("watch_time")
	sm.markFailed("ctr", "spark timeout")

	// engagement_score depends on watch_time (completed) + ctr (failed).
	operands := []config.OperandConfig{
		{MetricID: "watch_time", Weight: 1.0},
		{MetricID: "ctr", Weight: 1.0},
	}
	blocker, ok := sm.blockerFor(operands)
	if !ok {
		t.Fatalf("expected blocker, got none")
	}
	if blocker != "ctr" {
		t.Fatalf("expected blocker=ctr, got %s", blocker)
	}
}

func TestStatusMap_NoBlockerWhenAllCompleted(t *testing.T) {
	sm := newStatusMap()
	sm.markCompleted("a")
	sm.markCompleted("b")
	_, ok := sm.blockerFor([]config.OperandConfig{{MetricID: "a"}, {MetricID: "b"}})
	if ok {
		t.Fatalf("expected no blocker")
	}
}

func TestStatusMap_OperandNotYetRunIsBlocker(t *testing.T) {
	// An operand not in the status map (e.g., out-of-pass or not yet processed) blocks the
	// dependent. The caller must have iterated in topo order so this case = upstream not in pass.
	sm := newStatusMap()
	sm.markCompleted("a")
	blocker, ok := sm.blockerFor([]config.OperandConfig{{MetricID: "a"}, {MetricID: "missing"}})
	if !ok || blocker != "missing" {
		t.Fatalf("expected blocker=missing, got blocker=%q ok=%v", blocker, ok)
	}
	_ = status.Completed // keep import
}
