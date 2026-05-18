package jobs

import (
	"strings"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
)

// TopologicalOrder returns (sorted, skipped_cycle, error) for the given metrics
// using Kahn's algorithm over the COMPOSITE → operand dependency graph.
//
//   - `sorted` is the topo order; iterate in this order to ensure operands run
//     before COMPOSITEs that reference them.
//   - `skipped_cycle` is the set of metric IDs that participate in a cycle and
//     must be skipped (defense-in-depth — M5 already rejects cycles at
//     creation per ADR-026 / crates/experimentation-management/src/validators/
//     composite_cycle.rs, but the scheduler must not loop forever if a bad
//     cycle ever slips through).
//   - `error` is non-nil only for genuine algorithmic bugs; bad input produces
//     (sorted, skipped_cycle, nil) so the caller can keep going.
//
// Operands referencing metrics not in the current scheduling pass
// (cross-experiment, out of scope for ADR-026 #475) are ignored when building
// the in-degree map — the dependent COMPOSITE stays in-degree 0 and is
// emitted. The caller's statusMap.blockerFor gate then marks it
// skipped_upstream_failure at runtime because the operand has no recorded
// status.
func TopologicalOrder(metrics []*config.MetricConfig) (
	[]*config.MetricConfig,
	map[string]bool,
	error,
) {
	byID := make(map[string]*config.MetricConfig, len(metrics))
	for _, m := range metrics {
		byID[m.MetricID] = m
	}

	// Build in-degree map + adjacency list. Edges go FROM operand TO composite
	// (operand must run first).
	inDeg := make(map[string]int, len(metrics))
	children := make(map[string][]string, len(metrics))
	for _, m := range metrics {
		if _, ok := inDeg[m.MetricID]; !ok {
			inDeg[m.MetricID] = 0
		}
		// Case-insensitive match: standard.go, renderer.go, and isLegacyStyle
		// all normalize via strings.ToUpper. A lowercase / mixed-case "composite"
		// in a metric config would silently skip edge-building here while the
		// scheduler loop still gates on operand status — the COMPOSITE would land
		// before its operands in topo order and get marked SkippedUpstreamFailure
		// even though every operand would have succeeded. Devin BUG-0001 on #556.
		if strings.ToUpper(m.Type) != "COMPOSITE" {
			continue
		}
		for _, op := range m.Operands {
			if _, ok := byID[op.MetricID]; !ok {
				// Operand defined outside this pass — leave the COMPOSITE in
				// in-degree 0 and let the runtime status check skip it.
				continue
			}
			inDeg[m.MetricID]++
			children[op.MetricID] = append(children[op.MetricID], m.MetricID)
		}
	}

	// Kahn's: seed queue with in-degree-zero nodes, peel layers.
	// Iterate the original `metrics` slice rather than `inDeg` so the output
	// order is stable across runs (Go map iteration is randomized). Downstream
	// tests assert that non-COMPOSITE metrics keep their seed-file ordering;
	// only COMPOSITE metrics get reshuffled to land after their operands.
	queue := make([]string, 0, len(metrics))
	for _, m := range metrics {
		if inDeg[m.MetricID] == 0 {
			queue = append(queue, m.MetricID)
		}
	}

	sorted := make([]*config.MetricConfig, 0, len(metrics))
	for len(queue) > 0 {
		id := queue[0]
		queue = queue[1:]
		sorted = append(sorted, byID[id])
		for _, child := range children[id] {
			inDeg[child]--
			if inDeg[child] == 0 {
				queue = append(queue, child)
			}
		}
	}

	// Any node still with in-degree > 0 is in (or downstream of) a cycle.
	skipped := make(map[string]bool)
	for id, d := range inDeg {
		if d > 0 {
			skipped[id] = true
		}
	}
	return sorted, skipped, nil
}
