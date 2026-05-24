//! DFS-based cycle and depth-cap detection for COMPOSITE and METRICQL metric
//! graphs (ADR-026 Phase 1 B1 + Phase 2 A8).
//!
//! The validator walks the operand/reference graph rooted at the metric being
//! created (`start_metric_id`) and rejects any back-edge or any traversal that
//! exceeds the depth cap (default 5; see `validators::DEFAULT_DEPTH_CAP`).
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
//! store, so looking up `start_metric_id` will typically return
//! `StoreError::NotFound`. The caller passes `direct_operands` (the
//! operands/refs declared by the incoming proto) explicitly to seed the walk;
//! for every subsequent COMPOSITE or METRICQL node we read neighbors from the
//! store, treating NotFound there as a hard error — that means an
//! already-validated pointer is dangling, which is a real data-integrity bug.
//!
//! Neighbor dispatch (A8): the DFS calls `lookup.get_metric_type` on each
//! neighbor before deciding which getter to call:
//!   - COMPOSITE → `get_composite_operands`
//!   - METRICQL  → `get_metricql_refs`
//!   - all others → empty vec (leaf; no graph edges)

use std::collections::HashMap;

use tonic::Status;

use experimentation_proto::experimentation::common::v1::MetricType;

use crate::store::StoreError;
use crate::validators::MetricLookup;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    Gray,
    Black,
}

/// Walk the operand/reference graph rooted at `start_metric_id`. Reject
/// self-references, back-edges, missing intermediate nodes, and traversals
/// deeper than `depth_cap` levels.
///
/// The DFS dispatches on each neighbor's metric type so that COMPOSITE,
/// METRICQL, and mixed chains are all covered by a single walk.
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
        remaining: Vec::from(direct_operands).into_iter(),
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
                        // Descend. Dispatch on the neighbor's metric type to
                        // pick the right neighbor-source (A8 generalization).
                        color.insert(child.clone(), Color::Gray);

                        let neighbors = match lookup.get_metric_type(&child).await {
                            Ok(MetricType::Composite) => {
                                lookup.get_composite_operands(&child).await
                            }
                            Ok(MetricType::Metricql) => {
                                lookup.get_metricql_refs(&child).await
                            }
                            Ok(_) => {
                                // Leaf metric type — no graph edges, no further descent.
                                Ok(vec![])
                            }
                            Err(StoreError::NotFound(_)) => {
                                // A referenced metric has no row. The existence
                                // check (`exists_all_metrics`) ran first, so a
                                // true NotFound here is a real data-integrity error.
                                return Err(Box::new(Status::invalid_argument(format!(
                                    "metric operand '{}' not found during cycle walk",
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

                        let grandchildren = match neighbors {
                            Ok(v) => v,
                            Err(StoreError::NotFound(_)) => {
                                return Err(Box::new(Status::invalid_argument(format!(
                                    "metric operand '{}' lookup failed during cycle walk",
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

    /// Minimal in-memory lookup backed by a typed graph.
    ///
    /// `types[id]` carries the metric type; `graph[id]` lists the neighbors
    /// (COMPOSITE operands or METRICQL refs, depending on the type). Any id
    /// missing from `types` triggers `StoreError::NotFound` from
    /// `get_metric_type`.
    ///
    /// The legacy `new(pairs)` constructor defaults all nodes to COMPOSITE so
    /// all original tests keep passing without modification.
    struct GraphLookup {
        graph: HashMap<String, Vec<String>>,
        types: HashMap<String, MetricType>,
    }

    impl GraphLookup {
        /// Legacy constructor — every node is treated as COMPOSITE.
        fn new(pairs: &[(&str, &[&str])]) -> Self {
            let mut graph = HashMap::new();
            let mut types = HashMap::new();
            for (k, vs) in pairs {
                graph.insert(
                    (*k).to_string(),
                    vs.iter().map(|s| (*s).to_string()).collect(),
                );
                types.insert((*k).to_string(), MetricType::Composite);
            }
            Self { graph, types }
        }

        /// Typed constructor — caller specifies (id, MetricType, neighbors[]).
        fn typed(pairs: &[(&str, MetricType, &[&str])]) -> Self {
            let mut graph = HashMap::new();
            let mut types = HashMap::new();
            for (k, t, vs) in pairs {
                graph.insert(
                    (*k).to_string(),
                    vs.iter().map(|s| (*s).to_string()).collect(),
                );
                types.insert((*k).to_string(), *t);
            }
            Self { graph, types }
        }
    }

    #[tonic::async_trait]
    impl MetricLookup for GraphLookup {
        async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
            Ok(metric_ids.iter().all(|id| self.types.contains_key(*id)))
        }

        async fn get_composite_operands(
            &self,
            metric_id: &str,
        ) -> Result<Vec<String>, StoreError> {
            // Return stored neighbors; the DFS only calls this for COMPOSITE nodes.
            match self.types.get(metric_id) {
                None => Err(StoreError::NotFound(metric_id.to_string())),
                Some(_) => Ok(self.graph.get(metric_id).cloned().unwrap_or_default()),
            }
        }

        async fn get_metricql_refs(&self, metric_id: &str) -> Result<Vec<String>, StoreError> {
            match self.types.get(metric_id) {
                None => Err(StoreError::NotFound(metric_id.to_string())),
                Some(_) => Ok(self.graph.get(metric_id).cloned().unwrap_or_default()),
            }
        }

        async fn get_metric_type(&self, metric_id: &str) -> Result<MetricType, StoreError> {
            self.types
                .get(metric_id)
                .copied()
                .ok_or_else(|| StoreError::NotFound(metric_id.to_string()))
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_string()).collect()
    }

    // -----------------------------------------------------------------------
    // Original 11 COMPOSITE-only tests (must still pass)
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // A8 new tests: mixed COMPOSITE/METRICQL graph cycle detection
    // -----------------------------------------------------------------------

    /// COMPOSITE A → METRICQL B → @A → cycle.
    #[tokio::test]
    async fn composite_to_metricql_to_self_cycle() {
        // A (COMPOSITE) operands = [B]; B (METRICQL) refs = [A].
        let lookup = GraphLookup::typed(&[
            ("A", MetricType::Composite, &["B"]),
            ("B", MetricType::Metricql, &["A"]),
        ]);
        let err = check_no_cycles("A", &s(&["B"]), &lookup, 5).await.unwrap_err();
        assert!(
            err.message().contains("cycle detected"),
            "expected cycle error, got: {}",
            err.message()
        );
    }

    /// A (METRICQL) refs = [B], B (METRICQL) refs = [A] → cycle.
    #[tokio::test]
    async fn metricql_to_metricql_to_self_cycle() {
        let lookup = GraphLookup::typed(&[
            ("A", MetricType::Metricql, &["B"]),
            ("B", MetricType::Metricql, &["A"]),
        ]);
        let err = check_no_cycles("A", &s(&["B"]), &lookup, 5).await.unwrap_err();
        assert!(
            err.message().contains("cycle"),
            "expected cycle error, got: {}",
            err.message()
        );
    }

    /// A (COMPOSITE) → B (METRICQL) → C (COMPOSITE) → A → cycle (depth 3).
    #[tokio::test]
    async fn mixed_chain_depth_3_cycle() {
        let lookup = GraphLookup::typed(&[
            ("A", MetricType::Composite, &["B"]),
            ("B", MetricType::Metricql, &["C"]),
            ("C", MetricType::Composite, &["A"]),
        ]);
        let err = check_no_cycles("A", &s(&["B"]), &lookup, 5).await.unwrap_err();
        assert!(
            err.message().contains("cycle"),
            "expected cycle error, got: {}",
            err.message()
        );
    }

    /// Regression for Step 0 audit: a COMPOSITE metric referencing a MEAN-typed
    /// leaf must succeed. The DFS must treat MEAN as a leaf and return Ok.
    /// This test pins that the dispatcher never calls `get_composite_operands`
    /// on a non-COMPOSITE type.
    #[tokio::test]
    async fn regression_composite_with_mean_operand() {
        // A (notional COMPOSITE) operands = [meanmetric]; meanmetric type = MEAN.
        let lookup = GraphLookup::typed(&[
            ("meanmetric", MetricType::Mean, &[]),
        ]);
        let r = check_no_cycles("A", &s(&["meanmetric"]), &lookup, 5).await;
        assert!(
            r.is_ok(),
            "COMPOSITE referencing a MEAN leaf must be acyclic, got {:?}",
            r
        );
    }

    /// Acyclic METRICQL chain of depth 3: A (METRICQL) → B (METRICQL) → C (leaf Mean).
    #[tokio::test]
    async fn metricql_acyclic_chain_3() {
        let lookup = GraphLookup::typed(&[
            ("B", MetricType::Metricql, &["C"]),
            ("C", MetricType::Mean, &[]),
        ]);
        let r = check_no_cycles("A", &s(&["B"]), &lookup, 5).await;
        assert!(r.is_ok(), "acyclic METRICQL chain must pass, got {:?}", r);
    }

    /// Diamond METRICQL graph — A → B and A → C; both B and C reference D (leaf).
    /// Diamond is acyclic; must not be flagged as cycle.
    #[tokio::test]
    async fn metricql_diamond_not_cycle() {
        let lookup = GraphLookup::typed(&[
            ("B", MetricType::Metricql, &["D"]),
            ("C", MetricType::Metricql, &["D"]),
            ("D", MetricType::Mean, &[]),
        ]);
        let r = check_no_cycles("A", &s(&["B", "C"]), &lookup, 5).await;
        assert!(r.is_ok(), "METRICQL diamond must be acyclic, got {:?}", r);
    }
}
