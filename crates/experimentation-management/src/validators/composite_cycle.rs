//! DFS-based cycle and depth-cap detection for COMPOSITE metric graphs
//! (ADR-026 Phase 1, B1).
//!
//! The validator walks the operand graph rooted at the metric being created
//! (`start_metric_id`) and rejects any back-edge or any traversal that exceeds
//! the depth cap (default 5; see `validators::DEFAULT_DEPTH_CAP`).
//!
//! Algorithm — classic 3-color DFS:
//!
//!   WHITE — node not yet visited.
//!   GRAY  — node currently on the recursion stack (visit in progress).
//!   BLACK — node fully explored; all descendants are clean.
//!
//! A back-edge (DFS reaches a GRAY node) means the graph contains a cycle.
//!
//! Note on the root: the metric being validated is *not yet inserted* in the
//! store, so `lookup.get_composite_operands(start_metric_id)` will typically
//! return `StoreError::NotFound`. The caller passes `direct_operands` (the
//! operands declared by the incoming proto) explicitly to seed the walk; for
//! every subsequent COMPOSITE node we read its operands from the store, but
//! treat NotFound there as a hard error — that means an already-validated
//! pointer is dangling, which is a real data-integrity bug.

use std::collections::HashMap;

use tonic::Status;

use crate::store::StoreError;
use crate::validators::MetricLookup;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    Gray,
    Black,
}

/// Walk the operand graph rooted at `start_metric_id`. Reject self-references,
/// back-edges, missing intermediate operands, and traversals deeper than
/// `depth_cap` levels.
#[allow(clippy::result_large_err)]
pub async fn check_no_cycles<L: MetricLookup + ?Sized>(
    start_metric_id: &str,
    direct_operands: &[String],
    lookup: &L,
    depth_cap: usize,
) -> Result<(), Box<Status>> {
    // Trivial cycle: any direct operand equals the root id.
    for op in direct_operands {
        if op == start_metric_id {
            return Err(Box::new(Status::invalid_argument(format!(
                "composite cycle detected: {0} -> {0} (self-reference)",
                start_metric_id
            ))));
        }
    }

    // depth_cap = 0 is degenerate but treat it as "the root itself is fine,
    // no descendants allowed" — only roots with empty operands pass.
    if depth_cap == 0 && !direct_operands.is_empty() {
        return Err(Box::new(Status::invalid_argument(format!(
            "composite metric depth 1 exceeds maximum of {}",
            depth_cap
        ))));
    }

    let mut color: HashMap<String, Color> = HashMap::new();
    color.insert(start_metric_id.to_string(), Color::Gray);

    // Iterative DFS. Each frame stores (node, iterator over remaining children,
    // depth at which the node sits). Using an iterator over an owned Vec keeps
    // lifetimes simple while still avoiding rebuilding the children list on
    // backtrack.
    struct Frame {
        node: String,
        remaining: std::vec::IntoIter<String>,
        depth: usize,
        path: Vec<String>,
    }

    // Seed the stack with the root + its direct operands (depth 0).
    let mut path = vec![start_metric_id.to_string()];
    let mut stack: Vec<Frame> = vec![Frame {
        node: start_metric_id.to_string(),
        remaining: direct_operands.iter().cloned().collect::<Vec<_>>().into_iter(),
        depth: 0,
        path: path.clone(),
    }];

    while let Some(frame) = stack.last_mut() {
        match frame.remaining.next() {
            Some(child) => {
                let child_depth = frame.depth + 1;
                if child_depth > depth_cap {
                    return Err(Box::new(Status::invalid_argument(format!(
                        "composite metric depth {} exceeds maximum of {}",
                        child_depth, depth_cap
                    ))));
                }

                match color.get(&child) {
                    Some(Color::Gray) => {
                        return Err(Box::new(Status::invalid_argument(format!(
                            "composite cycle detected: {} -> {}",
                            frame.path.join(" -> "),
                            child
                        ))));
                    }
                    Some(Color::Black) => {
                        // Already proven clean — skip.
                    }
                    None => {
                        // Descend.
                        color.insert(child.clone(), Color::Gray);
                        let grandchildren = match lookup.get_composite_operands(&child).await {
                            Ok(v) => v,
                            Err(StoreError::NotFound(_)) => {
                                // A referenced metric has no row. The existence
                                // check (`exists_all_metrics`) ran first, so
                                // this means the row exists but is not a
                                // COMPOSITE — those return Vec::new() from the
                                // PG impl, so a true NotFound here is a real
                                // data integrity error.
                                return Err(Box::new(Status::invalid_argument(format!(
                                    "composite operand '{}' not found during cycle walk",
                                    child
                                ))));
                            }
                            Err(e) => {
                                return Err(Box::new(Status::internal(format!(
                                    "composite cycle walk failed: {}",
                                    e
                                ))));
                            }
                        };

                        let mut new_path = frame.path.clone();
                        new_path.push(child.clone());
                        path = new_path.clone();
                        stack.push(Frame {
                            node: child,
                            remaining: grandchildren.into_iter(),
                            depth: child_depth,
                            path: new_path,
                        });
                    }
                }
            }
            None => {
                // Exhausted this node's children — mark BLACK and pop.
                color.insert(frame.node.clone(), Color::Black);
                stack.pop();
                if let Some(parent) = stack.last() {
                    path = parent.path.clone();
                }
            }
        }
    }

    let _ = path; // silence unused-assignment lint on the final pop.
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreError;
    use std::collections::HashMap;

    /// Minimal in-memory lookup backed by a static graph. Any id missing from
    /// the graph triggers `StoreError::NotFound` from `get_composite_operands`.
    struct GraphLookup {
        graph: HashMap<String, Vec<String>>,
    }

    impl GraphLookup {
        fn new(pairs: &[(&str, &[&str])]) -> Self {
            let mut graph = HashMap::new();
            for (k, vs) in pairs {
                graph.insert(
                    (*k).to_string(),
                    vs.iter().map(|s| (*s).to_string()).collect(),
                );
            }
            Self { graph }
        }
    }

    #[tonic::async_trait]
    impl MetricLookup for GraphLookup {
        async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
            Ok(metric_ids.iter().all(|id| self.graph.contains_key(*id)))
        }
        async fn get_composite_operands(
            &self,
            metric_id: &str,
        ) -> Result<Vec<String>, StoreError> {
            self.graph
                .get(metric_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(metric_id.to_string()))
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_string()).collect()
    }

    #[tokio::test]
    async fn linear_two_nodes_accepts() {
        // A -> B (leaf)
        let lookup = GraphLookup::new(&[("B", &[])]);
        let r = check_no_cycles("A", &s(&["B"]), &lookup, 5).await;
        assert!(r.is_ok(), "expected Ok, got {:?}", r);
    }

    #[tokio::test]
    async fn two_level_chain_accepts() {
        // A -> B -> C (leaf)
        let lookup = GraphLookup::new(&[("B", &["C"]), ("C", &[])]);
        let r = check_no_cycles("A", &s(&["B"]), &lookup, 5).await;
        assert!(r.is_ok(), "got {:?}", r);
    }

    #[tokio::test]
    async fn direct_self_reference_rejects() {
        // A -> A
        let lookup = GraphLookup::new(&[]);
        let err = check_no_cycles("A", &s(&["A"]), &lookup, 5).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("self-reference"));
    }

    #[tokio::test]
    async fn two_node_cycle_rejects() {
        // A -> B -> A
        let lookup = GraphLookup::new(&[("B", &["A"])]);
        let err = check_no_cycles("A", &s(&["B"]), &lookup, 5).await.unwrap_err();
        assert!(err.message().contains("cycle detected"), "got: {}", err.message());
    }

    #[tokio::test]
    async fn three_node_cycle_rejects() {
        // A -> B -> C -> A
        let lookup = GraphLookup::new(&[("B", &["C"]), ("C", &["A"])]);
        let err = check_no_cycles("A", &s(&["B"]), &lookup, 5).await.unwrap_err();
        assert!(err.message().contains("cycle"));
    }

    #[tokio::test]
    async fn depth_exceeded_rejects() {
        // A -> B -> C -> D -> E -> F -> G (depth 6, cap 5)
        let lookup = GraphLookup::new(&[
            ("B", &["C"]),
            ("C", &["D"]),
            ("D", &["E"]),
            ("E", &["F"]),
            ("F", &["G"]),
            ("G", &[]),
        ]);
        let err = check_no_cycles("A", &s(&["B"]), &lookup, 5).await.unwrap_err();
        assert!(
            err.message().contains("depth"),
            "expected depth error, got: {}",
            err.message()
        );
        assert!(err.message().contains("maximum of 5"));
    }

    #[tokio::test]
    async fn at_cap_depth_accepts() {
        // A -> B -> C -> D -> E -> F (depth 5, cap 5)
        let lookup = GraphLookup::new(&[
            ("B", &["C"]),
            ("C", &["D"]),
            ("D", &["E"]),
            ("E", &["F"]),
            ("F", &[]),
        ]);
        let r = check_no_cycles("A", &s(&["B"]), &lookup, 5).await;
        assert!(r.is_ok(), "expected Ok at exactly cap, got {:?}", r);
    }

    #[tokio::test]
    async fn missing_intermediate_operand_rejects() {
        // A -> B, but B is not in the graph (NotFound during DFS).
        let lookup = GraphLookup::new(&[]);
        let err = check_no_cycles("A", &s(&["B"]), &lookup, 5).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("not found"));
    }

    #[tokio::test]
    async fn diamond_shape_is_not_a_cycle() {
        // A -> B, A -> C; B -> D; C -> D. D is shared but reached via separate
        // paths — once D goes BLACK, the second visit must short-circuit
        // without flagging a cycle.
        let lookup = GraphLookup::new(&[
            ("B", &["D"]),
            ("C", &["D"]),
            ("D", &[]),
        ]);
        let r = check_no_cycles("A", &s(&["B", "C"]), &lookup, 5).await;
        assert!(r.is_ok(), "diamond is acyclic, got {:?}", r);
    }

    #[tokio::test]
    async fn empty_operands_accepts() {
        let lookup = GraphLookup::new(&[]);
        let r = check_no_cycles("A", &[], &lookup, 5).await;
        assert!(r.is_ok());
    }
}
