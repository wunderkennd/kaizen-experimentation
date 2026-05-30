//! Shared test helpers for the migration module.
//!
//! Available when the `test-support` feature is enabled or inside `#[cfg(test)]`
//! blocks. This module provides `MetricLookup` implementations used by both unit
//! tests (in `tier1.rs`) and integration tests (`tests/custom_corpus_parity.rs`).
//!
//! ## Motivation
//!
//! `SeedLookup` was previously duplicated byte-for-byte between `tier1::tests`
//! and `custom_corpus_parity`. Moving it here ensures the two copies cannot
//! drift independently.

use std::collections::HashMap;

use experimentation_proto::experimentation::common::v1::MetricType;

use crate::store::StoreError;
use crate::validators::MetricLookup;

// ---------------------------------------------------------------------------
// EmptyLookup
// ---------------------------------------------------------------------------

/// Empty lookup — `exists_all_metrics` returns `true` only for the empty
/// slice (vacuous truth), and `false` for any non-empty slice.
///
/// Suitable for FILTERED_MEAN and WINDOWED_COUNT where the validator never
/// calls `exists_all_metrics` with a non-empty slice.
pub struct EmptyLookup;

#[tonic::async_trait]
impl MetricLookup for EmptyLookup {
    async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
        // For FILTERED_MEAN and WINDOWED_COUNT, the validator never calls
        // this. For COMPOSITE it is called — use SeedLookup instead.
        Ok(metric_ids.is_empty())
    }

    async fn get_composite_operands(
        &self,
        metric_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        Err(StoreError::NotFound(metric_id.to_string()))
    }

    async fn get_metricql_refs(&self, _metric_id: &str) -> Result<Vec<String>, StoreError> {
        Ok(vec![])
    }

    async fn get_metric_type(
        &self,
        metric_id: &str,
    ) -> Result<MetricType, StoreError> {
        Err(StoreError::NotFound(metric_id.to_string()))
    }
}

// ---------------------------------------------------------------------------
// SeedLookup
// ---------------------------------------------------------------------------

/// Seeded lookup — knows a fixed set of metric IDs as leaves (no sub-operands).
///
/// Used for COMPOSITE fixture tests where `validate_composite` calls
/// `exists_all_metrics` to confirm each operand exists. All seeded IDs are
/// treated as leaf metrics (no further operands), so cycle detection terminates
/// immediately.
pub struct SeedLookup {
    ids: HashMap<String, ()>,
}

impl SeedLookup {
    pub fn with_ids(ids: &[&str]) -> Self {
        Self {
            ids: ids.iter().map(|s| ((*s).to_string(), ())).collect(),
        }
    }
}

#[tonic::async_trait]
impl MetricLookup for SeedLookup {
    async fn exists_all_metrics(&self, metric_ids: &[&str]) -> Result<bool, StoreError> {
        Ok(metric_ids.iter().all(|id| self.ids.contains_key(*id)))
    }

    async fn get_composite_operands(
        &self,
        _metric_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        // All seeded metrics are leaves (no sub-operands).
        Ok(vec![])
    }

    async fn get_metricql_refs(&self, _metric_id: &str) -> Result<Vec<String>, StoreError> {
        Ok(vec![])
    }

    async fn get_metric_type(
        &self,
        metric_id: &str,
    ) -> Result<MetricType, StoreError> {
        if self.ids.contains_key(metric_id) {
            // Leaf type — cycle detection won't try to follow operands.
            Ok(MetricType::FilteredMean)
        } else {
            Err(StoreError::NotFound(metric_id.to_string()))
        }
    }
}
