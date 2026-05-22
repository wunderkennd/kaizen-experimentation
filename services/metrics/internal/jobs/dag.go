package jobs

import (
	"fmt"
	"strings"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/metricql"
)

// operandIDs returns the metric IDs that the given metric depends on, used by
// both the DAG builder and the upstream-failure gate in standard.go::Run.
//
//   - COMPOSITE: m.Operands (config-time slice).
//   - METRICQL:  parsed @metric_refs from m.MetricqlExpression. Parse error
//     here is propagated to the caller (TopologicalOrder records it in
//     failedParse so the scheduler can write status.Failed upfront -- avoids
//     surfacing parse errors as a confusing "operand-missing" log later).
//   - other types: nil (leaf metric, no dependencies).
//
// Devin design feedback on PR #559: swallowing the parse error here defers
// user-visible failure to compile time, where status_map sees no recorded
// entry and downstream dependents get marked SkippedUpstreamFailure for a
// reason that's actually a parse error. Propagating yields a single, clearer
// Failed row with reason "metricql: parse: <msg>".
func operandIDs(m *config.MetricConfig) ([]string, error) {
	switch strings.ToUpper(m.Type) {
	case "COMPOSITE":
		ids := make([]string, len(m.Operands))
		for i, op := range m.Operands {
			ids[i] = op.MetricID
		}
		return ids, nil
	case "METRICQL":
		if m.MetricqlExpression == "" {
			return nil, fmt.Errorf("metricql parse for %s: empty metricql_expression", m.MetricID)
		}
		root, err := metricql.Parse(m.MetricqlExpression)
		if err != nil {
			return nil, fmt.Errorf("metricql parse for %s: %w", m.MetricID, err)
		}
		return metricql.ExtractMetricRefs(root), nil
	}
	return nil, nil
}

// TopologicalOrder returns (sorted, skipped_cycle, failed_parse, error) for the
// given metrics using Kahn's algorithm over the dependency graph (COMPOSITE
// operands + METRICQL @metric_refs).
//
//   - `sorted` is the topo order; iterate in this order to ensure dependencies
//     run before dependents.
//   - `skipped_cycle` is the set of metric IDs that participate in a cycle and
//     must be skipped (defense-in-depth -- M5 already rejects cycles at
//     creation per ADR-026 / composite_cycle.rs, but the scheduler must not
//     loop forever if a bad cycle ever slips through).
//   - `failed_parse` is the set of METRICQL metric IDs whose source text
//     failed to parse during DAG build. The scheduler must pre-mark these as
//     status.Failed before the main loop so downstream gates treat them
//     identically to executor failures.
//   - `error` is non-nil only for genuine algorithmic bugs; bad input
//     (cycles, parse failures) produces an error-free return so the caller
//     can keep going.
//
// Operands referencing metrics not in the current scheduling pass
// (cross-experiment, out of scope for ADR-026) are ignored when building the
// in-degree map -- the dependent stays in-degree 0 and is emitted. The
// caller's statusMap gate then marks it skipped_upstream_failure at runtime
// because the operand has no recorded status.
func TopologicalOrder(metrics []*config.MetricConfig) (
	[]*config.MetricConfig,
	map[string]bool,
	map[string]error,
	error,
) {
	byID := make(map[string]*config.MetricConfig, len(metrics))
	for _, m := range metrics {
		byID[m.MetricID] = m
	}

	failedParse := make(map[string]error)

	// Build in-degree map + adjacency list. Edges go FROM operand TO dependent
	// (operand must run first).
	inDeg := make(map[string]int, len(metrics))
	children := make(map[string][]string, len(metrics))
	for _, m := range metrics {
		if _, ok := inDeg[m.MetricID]; !ok {
			inDeg[m.MetricID] = 0
		}
		ids, err := operandIDs(m)
		if err != nil {
			// METRICQL parse failure -- record for the scheduler to mark Failed.
			// The metric itself has no edges (we can't extract refs), so it
			// stays in-degree 0 and lands in the topo output. The scheduler's
			// pre-loop pre-mark + the renderer arm both surface the failure.
			failedParse[m.MetricID] = err
			continue
		}
		for _, opID := range ids {
			if _, ok := byID[opID]; !ok {
				// Operand defined outside this pass -- leave in in-degree 0
				// and let the runtime status check skip it.
				continue
			}
			inDeg[m.MetricID]++
			children[opID] = append(children[opID], m.MetricID)
		}
	}

	// Kahn's: seed queue with in-degree-zero nodes, peel layers.
	// Iterate the original `metrics` slice rather than `inDeg` so output order
	// is stable across runs (Go map iteration is randomized).
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
	return sorted, skipped, failedParse, nil
}
