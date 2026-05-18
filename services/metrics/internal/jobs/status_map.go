package jobs

import (
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/status"
)

// statusMap tracks the per-metric outcome for one scheduling pass (one experiment
// × computation_date). It is the in-memory companion to the status.Writer that
// flushes results to PG at the end of the pass.
//
// COMPOSITE metrics gate execution on operand status via blockerFor: if any
// operand is missing or not Completed, the COMPOSITE is marked
// SkippedUpstreamFailure without attempting render/execute.
//
// Not safe for concurrent use — the scheduler iterates metrics serially in
// topological order (see TopologicalOrder in dag.go).
type statusMap struct {
	entries map[string]status.Status
	reasons map[string]string
}

func newStatusMap() *statusMap {
	return &statusMap{
		entries: make(map[string]status.Status),
		reasons: make(map[string]string),
	}
}

func (s *statusMap) markCompleted(id string) {
	s.entries[id] = status.Completed
}

func (s *statusMap) markFailed(id, reason string) {
	s.entries[id] = status.Failed
	s.reasons[id] = reason
}

func (s *statusMap) markSkippedUpstream(id, blocker string) {
	s.entries[id] = status.SkippedUpstreamFailure
	s.reasons[id] = "operand " + blocker + " did not complete"
}

func (s *statusMap) markSkippedCycle(id string) {
	s.entries[id] = status.SkippedCycle
	s.reasons[id] = "metric participates in a COMPOSITE cycle"
}

// blockerFor returns the first operand whose status is not Completed (or "" if
// all operands completed). The boolean is true iff a blocker exists.
//
// Operands not recorded in the map (e.g., out-of-pass metrics referenced by a
// COMPOSITE) are treated as blockers — the caller has iterated in topo order so
// every operand within the pass will already have a recorded status by the time
// the dependent COMPOSITE is evaluated.
func (s *statusMap) blockerFor(operands []config.OperandConfig) (string, bool) {
	for _, op := range operands {
		st, ok := s.entries[op.MetricID]
		if !ok || st != status.Completed {
			return op.MetricID, true
		}
	}
	return "", false
}

// reasonOf returns the recorded reason (or empty string if unrecorded).
func (s *statusMap) reasonOf(id string) string {
	return s.reasons[id]
}
