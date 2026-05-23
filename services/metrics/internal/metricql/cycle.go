package metricql

import (
	"fmt"
	"strings"
)

// MaxCompositeDepth matches M5's DEFAULT_DEPTH_CAP in
// crates/experimentation-management/src/validators/composite_cycle.rs.
// Keep the two values in sync -- M5 and M3 must agree on what is a cycle.
const MaxCompositeDepth = 5

// OperandLookup returns the @metric_ref operand IDs declared by a given
// metric ID, plus a boolean indicating whether the metric exists in the
// store. The two-value signature is the Go idiom for "fallible lookup":
//   - non-COMPOSITE / non-METRICQL metric: return (nil, true)
//   - metric not in store at all:          return (nil, false)
//
// The cycle walker treats `not exists` as a data-integrity error because
// the existence check ran first during M5 validation.
type OperandLookup func(metricID string) (operands []string, exists bool)

// CycleError reports a detected cycle or depth-cap violation with the path
// that produced it. Matches the Rust analogue's error message format so M5
// and M3 surface identical diagnostics for the same input.
type CycleError struct {
	Message string
}

func (e *CycleError) Error() string { return e.Message }

// CheckNoCycles walks the @metric_ref graph rooted at rootID. Rejects
// self-references, back-edges to ancestors on the current path, traversals
// deeper than MaxCompositeDepth, and dangling references encountered during
// the walk.
//
// Algorithm (3-color iterative DFS, ported from Rust):
//   - WHITE = absent from the color map
//   - GRAY  = on the current recursion path
//   - BLACK = fully explored; descendants known clean
//
// A back-edge (DFS reaches a GRAY node) means the graph contains a cycle.
// BLACK nodes short-circuit the walk -- a diamond shape is acyclic.
func CheckNoCycles(rootID string, directOperands []string, lookup OperandLookup) error {
	return checkNoCyclesWithCap(rootID, directOperands, lookup, MaxCompositeDepth)
}

// checkNoCyclesWithCap is the depth-cap-parameterized variant used by tests
// to validate cap-boundary behavior independently of the production constant.
func checkNoCyclesWithCap(rootID string, directOperands []string, lookup OperandLookup, depthCap int) error {
	// Trivial cycle: any direct operand equals the root id.
	for _, op := range directOperands {
		if op == rootID {
			return &CycleError{Message: fmt.Sprintf("composite cycle detected: %s -> %s (self-reference)", rootID, rootID)}
		}
	}

	// Degenerate: depth_cap = 0 admits only roots with empty operands.
	if depthCap == 0 && len(directOperands) > 0 {
		return &CycleError{Message: fmt.Sprintf("composite metric depth 1 exceeds maximum of %d", depthCap)}
	}

	const (
		colorGray  = 1
		colorBlack = 2
	)
	color := map[string]int{rootID: colorGray}

	// Each stack frame stores the node, remaining-children iterator (as a
	// slice of strings), depth, and the path that led here. Path is duplicated
	// per frame so back-edges can produce a clear "A -> B -> C -> ..." trail.
	type frame struct {
		node      string
		remaining []string
		idx       int
		depth     int
		path      []string
	}

	rootPath := []string{rootID}
	stack := []frame{{
		node:      rootID,
		remaining: append([]string(nil), directOperands...),
		idx:       0,
		depth:     0,
		path:      rootPath,
	}}

	for len(stack) > 0 {
		top := &stack[len(stack)-1]
		if top.idx >= len(top.remaining) {
			// Exhausted this node's children -- mark BLACK and pop.
			color[top.node] = colorBlack
			stack = stack[:len(stack)-1]
			continue
		}

		child := top.remaining[top.idx]
		top.idx++
		childDepth := top.depth + 1

		if childDepth > depthCap {
			return &CycleError{Message: fmt.Sprintf("composite metric depth %d exceeds maximum of %d", childDepth, depthCap)}
		}

		switch color[child] {
		case colorGray:
			return &CycleError{Message: fmt.Sprintf("composite cycle detected: %s -> %s", strings.Join(top.path, " -> "), child)}
		case colorBlack:
			// Already proven clean -- skip.
			continue
		}

		// White (unvisited). Descend.
		color[child] = colorGray
		grandchildren, exists := lookup(child)
		if !exists {
			// A referenced metric has no row. The existence check ran first
			// in M5 validation, so this means the row exists but is not a
			// composite/metricql -- non-composite metrics return ([], true)
			// from the lookup, so a true !exists here is a data integrity error.
			return &CycleError{Message: fmt.Sprintf("composite operand '%s' not found during cycle walk", child)}
		}

		newPath := make([]string, len(top.path)+1)
		copy(newPath, top.path)
		newPath[len(top.path)] = child
		stack = append(stack, frame{
			node:      child,
			remaining: grandchildren,
			idx:       0,
			depth:     childDepth,
			path:      newPath,
		})
	}

	return nil
}
