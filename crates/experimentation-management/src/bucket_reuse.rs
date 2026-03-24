//! Bucket reuse allocator with overlap detection (ADR-009).
//!
//! Layers divide traffic into `total_buckets` integer buckets [0, total_buckets).
//! Each experiment claims a contiguous range [start_bucket, end_bucket).
//!
//! ## Overlap detection
//!
//! Two ranges [a, b) and [c, d) overlap iff NOT (b <= c OR d <= a).
//! The SQL query returns active allocations that overlap the requested range,
//! so an empty result means the range is free.
//!
//! ## Cooldown (bucket reuse)
//!
//! When an experiment concludes, its allocation is released but stays off-limits
//! until `reusable_after` (layer's `bucket_reuse_cooldown_seconds` after release).
//! This prevents users who remember seeing the control from being re-exposed in
//! a different treatment while the carryover period is still active.

use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::store::StoreError;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Allocation {
    pub allocation_id: Uuid,
    pub layer_id: Uuid,
    pub experiment_id: Uuid,
    pub start_bucket: i32,
    pub end_bucket: i32,
    pub activated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub released_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reusable_after: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
struct AllocationRow {
    allocation_id: Uuid,
    layer_id: Uuid,
    experiment_id: Uuid,
    start_bucket: i32,
    end_bucket: i32,
    activated_at: Option<chrono::DateTime<chrono::Utc>>,
    released_at: Option<chrono::DateTime<chrono::Utc>>,
    reusable_after: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<AllocationRow> for Allocation {
    fn from(r: AllocationRow) -> Self {
        Allocation {
            allocation_id: r.allocation_id,
            layer_id: r.layer_id,
            experiment_id: r.experiment_id,
            start_bucket: r.start_bucket,
            end_bucket: r.end_bucket,
            activated_at: r.activated_at,
            released_at: r.released_at,
            reusable_after: r.reusable_after,
        }
    }
}

// ---------------------------------------------------------------------------
// Allocator errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum AllocatorError {
    #[error("overlap with existing allocation {0} ([{1}, {2}))")]
    Overlap(Uuid, i32, i32),
    #[error("layer {0} not found")]
    LayerNotFound(Uuid),
    #[error("requested range [{0}, {1}) is invalid (must have start < end and both in [0, total_buckets))")]
    InvalidRange(i32, i32),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

// ---------------------------------------------------------------------------
// Layer config
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow, Debug, Clone)]
struct LayerRow {
    #[allow(dead_code)]
    layer_id: Uuid,
    total_buckets: i32,
    bucket_reuse_cooldown_seconds: i32,
}

// ---------------------------------------------------------------------------
// Allocator
// ---------------------------------------------------------------------------

/// Attempt to allocate [start_bucket, end_bucket) for `experiment_id` in `layer_id`.
///
/// Steps:
/// 1. Load the layer to validate `total_buckets`.
/// 2. Check for overlap with active allocations (released_at IS NULL).
/// 3. Check for overlap with allocations still in cooldown (released_at IS NOT NULL AND reusable_after > NOW()).
/// 4. Insert the allocation if clear.
///
/// The check-then-insert is NOT atomic here — callers should hold a layer-level advisory
/// lock or accept the small TOCTOU window (acceptable since allocation races resolve
/// via unique constraint failures).
pub async fn allocate(
    pool: &PgPool,
    layer_id: Uuid,
    experiment_id: Uuid,
    start_bucket: i32,
    end_bucket: i32,
) -> Result<Allocation, AllocatorError> {
    // 1. Load layer.
    let layer: Option<LayerRow> = sqlx::query_as(
        "SELECT layer_id, total_buckets, bucket_reuse_cooldown_seconds FROM layers WHERE layer_id = $1",
    )
    .bind(layer_id)
    .fetch_optional(pool)
    .await?;

    let layer = layer.ok_or(AllocatorError::LayerNotFound(layer_id))?;

    // 2. Validate range.
    if start_bucket >= end_bucket || start_bucket < 0 || end_bucket > layer.total_buckets {
        return Err(AllocatorError::InvalidRange(start_bucket, end_bucket));
    }

    // 3. Detect overlap with active allocations.
    check_overlap(pool, layer_id, experiment_id, start_bucket, end_bucket, false).await?;

    // 4. Detect overlap with cooldown-protected allocations.
    check_overlap(pool, layer_id, experiment_id, start_bucket, end_bucket, true).await?;

    // 5. Insert allocation.
    let row: AllocationRow = sqlx::query_as(
        r#"INSERT INTO layer_allocations
               (layer_id, experiment_id, start_bucket, end_bucket, activated_at,
                reusable_after)
           VALUES ($1, $2, $3, $4, NOW(), NULL)
           RETURNING allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
                     activated_at, released_at, reusable_after"#,
    )
    .bind(layer_id)
    .bind(experiment_id)
    .bind(start_bucket)
    .bind(end_bucket)
    .fetch_one(pool)
    .await?;

    Ok(Allocation::from(row))
}

/// Release the allocation for `experiment_id` in `layer_id`.
/// Sets `released_at = NOW()` and `reusable_after = NOW() + cooldown`.
pub async fn release(
    pool: &PgPool,
    layer_id: Uuid,
    experiment_id: Uuid,
) -> Result<(), AllocatorError> {
    let layer: Option<LayerRow> = sqlx::query_as(
        "SELECT layer_id, total_buckets, bucket_reuse_cooldown_seconds FROM layers WHERE layer_id = $1",
    )
    .bind(layer_id)
    .fetch_optional(pool)
    .await?;

    let cooldown_secs = layer
        .map(|l| l.bucket_reuse_cooldown_seconds as i64)
        .unwrap_or(86400);

    sqlx::query(
        r#"UPDATE layer_allocations
           SET released_at = NOW(),
               reusable_after = NOW() + ($1 || ' seconds')::INTERVAL
           WHERE layer_id = $2 AND experiment_id = $3 AND released_at IS NULL"#,
    )
    .bind(cooldown_secs)
    .bind(layer_id)
    .bind(experiment_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// List all active allocations for a layer (including released ones if requested).
pub async fn list_allocations(
    pool: &PgPool,
    layer_id: Uuid,
    include_released: bool,
) -> Result<Vec<Allocation>, AllocatorError> {
    let rows: Vec<AllocationRow> = if include_released {
        sqlx::query_as(
            r#"SELECT allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
                      activated_at, released_at, reusable_after
               FROM layer_allocations WHERE layer_id = $1
               ORDER BY start_bucket"#,
        )
        .bind(layer_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"SELECT allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
                      activated_at, released_at, reusable_after
               FROM layer_allocations WHERE layer_id = $1 AND released_at IS NULL
               ORDER BY start_bucket"#,
        )
        .bind(layer_id)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.into_iter().map(Allocation::from).collect())
}

// ---------------------------------------------------------------------------
// Internal: overlap check
// ---------------------------------------------------------------------------

/// Check for overlapping allocations.
///
/// If `in_cooldown = false`: checks active (released_at IS NULL) allocations.
/// If `in_cooldown = true`: checks released allocations still in cooldown.
///
/// Two ranges [a, b) and [c, d) overlap iff NOT (b <= c OR d <= a),
/// i.e., a < d AND c < b.
async fn check_overlap(
    pool: &PgPool,
    layer_id: Uuid,
    exclude_experiment_id: Uuid,
    start: i32,
    end: i32,
    in_cooldown: bool,
) -> Result<(), AllocatorError> {
    let conflicting: Option<AllocationRow> = if in_cooldown {
        sqlx::query_as(
            r#"SELECT allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
                      activated_at, released_at, reusable_after
               FROM layer_allocations
               WHERE layer_id = $1
                 AND experiment_id != $2
                 AND released_at IS NOT NULL
                 AND reusable_after > NOW()
                 AND start_bucket < $4   -- their start < our end
                 AND $3 < end_bucket     -- our start < their end
               LIMIT 1"#,
        )
        .bind(layer_id)
        .bind(exclude_experiment_id)
        .bind(start)
        .bind(end)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"SELECT allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
                      activated_at, released_at, reusable_after
               FROM layer_allocations
               WHERE layer_id = $1
                 AND experiment_id != $2
                 AND released_at IS NULL
                 AND start_bucket < $4
                 AND $3 < end_bucket
               LIMIT 1"#,
        )
        .bind(layer_id)
        .bind(exclude_experiment_id)
        .bind(start)
        .bind(end)
        .fetch_optional(pool)
        .await?
    };

    if let Some(conflict) = conflicting {
        return Err(AllocatorError::Overlap(
            conflict.experiment_id,
            conflict.start_bucket,
            conflict.end_bucket,
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests (pure logic, no DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    /// Verify overlap logic with direct range comparisons (mirrors SQL).
    fn ranges_overlap(a_start: i32, a_end: i32, b_start: i32, b_end: i32) -> bool {
        // [a_start, a_end) overlaps [b_start, b_end) iff NOT (a_end <= b_start OR b_end <= a_start)
        !(a_end <= b_start || b_end <= a_start)
    }

    #[test]
    fn adjacent_ranges_do_not_overlap() {
        assert!(!ranges_overlap(0, 500, 500, 1000));
        assert!(!ranges_overlap(500, 1000, 0, 500));
    }

    #[test]
    fn overlapping_ranges() {
        assert!(ranges_overlap(0, 600, 500, 1000));
        assert!(ranges_overlap(200, 800, 100, 300));
        assert!(ranges_overlap(0, 1000, 500, 600)); // contained
    }

    #[test]
    fn identical_ranges_overlap() {
        assert!(ranges_overlap(100, 200, 100, 200));
    }

    #[test]
    fn single_bucket_boundary() {
        assert!(!ranges_overlap(0, 1, 1, 2));
        assert!(ranges_overlap(0, 2, 1, 3));
    }
}
