//! WASM target validation of the 10,000 hash test vectors.
//!
//! Run: wasm-pack test --node --features wasm crates/experimentation-hash

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

#[derive(serde::Deserialize)]
struct HashVector {
    user_id: String,
    salt: String,
    total_buckets: u32,
    expected_bucket: u32,
}

// Embed the 10K vectors at compile time — no filesystem access in WASM.
const VECTORS_JSON: &str = include_str!("../../../test-vectors/hash_vectors.json");

#[wasm_bindgen_test]
fn wasm_hash_vectors_10k() {
    let vectors: Vec<HashVector> =
        serde_json::from_str(VECTORS_JSON).expect("Failed to parse hash vectors");

    assert!(!vectors.is_empty(), "test vector file is empty");

    let mut failures = Vec::new();
    for (i, v) in vectors.iter().enumerate() {
        let actual =
            experimentation_hash::wasm_bucket(&v.user_id, &v.salt, v.total_buckets);
        if actual != v.expected_bucket {
            if failures.len() < 10 {
                failures.push(format!(
                    "vector[{i}]: user={:?} salt={:?} expected={} got={}",
                    v.user_id, v.salt, v.expected_bucket, actual
                ));
            } else {
                failures.push(String::new()); // count only
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} WASM test vectors failed. First failures:\n{}",
        failures.len(),
        vectors.len(),
        failures.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[wasm_bindgen_test]
fn wasm_allocation_boundary() {
    // Inclusive range: [0, 100]
    assert!(experimentation_hash::wasm_is_in_allocation(0, 0, 100));
    assert!(experimentation_hash::wasm_is_in_allocation(50, 0, 100));
    assert!(experimentation_hash::wasm_is_in_allocation(100, 0, 100));
    assert!(!experimentation_hash::wasm_is_in_allocation(101, 0, 100));

    // Single-bucket allocation
    assert!(experimentation_hash::wasm_is_in_allocation(42, 42, 42));
    assert!(!experimentation_hash::wasm_is_in_allocation(41, 42, 42));
    assert!(!experimentation_hash::wasm_is_in_allocation(43, 42, 42));

    // Full range
    assert!(experimentation_hash::wasm_is_in_allocation(0, 0, u32::MAX));
    assert!(experimentation_hash::wasm_is_in_allocation(u32::MAX, 0, u32::MAX));
}
