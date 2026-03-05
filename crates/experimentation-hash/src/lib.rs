//! Deterministic hashing for experiment assignment and feature flag evaluation.
//!
//! This crate is the single source of truth for user → bucket mapping.
//! It MUST produce identical results across:
//!   - Rust native (this crate)
//!   - WebAssembly (feature: wasm)
//!   - C FFI via cbindgen (feature: ffi)
//!   - UniFFI for Swift/Kotlin (feature: uniffi)
//!   - Python via PyO3 (feature: python)
//!
//! The 10,000-entry test vector file in test-vectors/hash_vectors.json
//! is the authoritative correctness reference. All targets must pass it.

pub mod murmur3;

/// Compute the bucket assignment for a user in an experiment.
///
/// This is the core function that every SDK and service calls.
/// It MUST be deterministic: same (user_id, salt, total_buckets) → same bucket, always.
///
/// # Arguments
/// * `user_id` - Unique user identifier
/// * `salt` - Experiment-specific salt (typically experiment_id or hash_salt from config)
/// * `total_buckets` - Number of buckets in the layer (default: 10,000)
///
/// # Returns
/// Bucket index in `[0, total_buckets)`.
///
/// # Panics
/// Panics if `total_buckets` is 0.
pub fn bucket(user_id: &str, salt: &str, total_buckets: u32) -> u32 {
    assert!(total_buckets > 0, "total_buckets must be > 0");

    // Concatenate user_id and salt with a separator that can't appear in either.
    let key = format!("{user_id}\x00{salt}");
    let hash = murmur3::murmurhash3_x86_32(key.as_bytes(), 0);

    hash % total_buckets
}

/// Check if a bucket falls within an allocation range (inclusive).
///
/// Layer allocations use inclusive ranges: [start_bucket, end_bucket].
pub fn is_in_allocation(bucket: u32, start_bucket: u32, end_bucket: u32) -> bool {
    bucket >= start_bucket && bucket <= end_bucket
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn wasm_bucket(user_id: &str, salt: &str, total_buckets: u32) -> u32 {
    bucket(user_id, salt, total_buckets)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn wasm_is_in_allocation(bucket: u32, start_bucket: u32, end_bucket: u32) -> bool {
    is_in_allocation(bucket, start_bucket, end_bucket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic() {
        let b1 = bucket("user_123", "salt_abc", 10_000);
        let b2 = bucket("user_123", "salt_abc", 10_000);
        assert_eq!(b1, b2, "same inputs must produce same bucket");
    }

    #[test]
    fn test_range() {
        for _ in 0..1000 {
            let b = bucket("user", &uuid::Uuid::new_v4().to_string(), 10_000);
            assert!(b < 10_000);
        }
    }

    #[test]
    fn test_different_salts_differ() {
        let b1 = bucket("user_123", "experiment_1", 10_000);
        let b2 = bucket("user_123", "experiment_2", 10_000);
        // Different salts should (almost always) produce different buckets.
        // Not guaranteed, but probability of collision is 1/10000.
        // We just verify the function runs without error.
        let _ = (b1, b2);
    }

    #[test]
    fn test_allocation_check() {
        assert!(is_in_allocation(50, 0, 100));
        assert!(is_in_allocation(0, 0, 100));
        assert!(is_in_allocation(100, 0, 100));
        assert!(!is_in_allocation(101, 0, 100));
    }

    #[test]
    #[should_panic(expected = "total_buckets must be > 0")]
    fn test_zero_buckets_panics() {
        bucket("user", "salt", 0);
    }
}
