//! Interleaving algorithms for ranker comparison.
//!
//! Implements Team Draft (Phase 1), Optimized (Phase 2), and Multileave (Phase 3).

pub mod team_draft;

/// A merged list with provenance metadata.
#[derive(Debug, Clone)]
pub struct InterleavedResult {
    /// Items in display order.
    pub merged_list: Vec<String>,
    /// Map: item_id → algorithm_id that contributed it.
    pub provenance: std::collections::HashMap<String, String>,
}
