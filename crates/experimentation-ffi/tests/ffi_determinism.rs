//! Validate hash parity through the C FFI boundary.
//!
//! This test links against the Rust `lib` target (not the cdylib) so we can
//! call the `extern "C"` functions directly from Rust integration tests.
//! The same symbols are exported identically by the cdylib/staticlib.
//!
//! Run: cargo test --package experimentation-ffi

use std::ffi::CString;

use serde::Deserialize;

// Re-use the FFI functions directly — they are `#[no_mangle] pub extern "C"`
// so they are visible when we depend on the `lib` crate-type.
use experimentation_ffi::{
    experimentation_bucket, experimentation_is_in_allocation, EXPERIMENTATION_BUCKET_ERROR,
};

#[derive(Deserialize)]
struct HashVector {
    user_id: String,
    salt: String,
    total_buckets: u32,
    expected_bucket: u32,
}

#[test]
fn ffi_hash_vectors_10k() {
    let vectors_path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-vectors/hash_vectors.json");
    let data = std::fs::read_to_string(vectors_path)
        .unwrap_or_else(|e| panic!("Failed to read {vectors_path}: {e}"));

    let vectors: Vec<HashVector> =
        serde_json::from_str(&data).unwrap_or_else(|e| panic!("Failed to parse: {e}"));

    assert!(!vectors.is_empty(), "test vector file is empty");

    let mut failures = 0;
    for (i, v) in vectors.iter().enumerate() {
        let c_user_id = CString::new(v.user_id.as_str()).unwrap();
        let c_salt = CString::new(v.salt.as_str()).unwrap();

        let actual = unsafe {
            experimentation_bucket(c_user_id.as_ptr(), c_salt.as_ptr(), v.total_buckets)
        };

        if actual != v.expected_bucket {
            failures += 1;
            if failures <= 10 {
                eprintln!(
                    "FAIL vector[{i}]: user={:?} salt={:?} expected={} got={}",
                    v.user_id, v.salt, v.expected_bucket, actual
                );
            }
        }
    }

    assert_eq!(
        failures, 0,
        "{failures}/{} FFI test vectors failed. First 10 failures shown above.",
        vectors.len()
    );

    eprintln!("All {} FFI hash test vectors passed.", vectors.len());
}

#[test]
fn ffi_null_safety() {
    let valid = CString::new("test").unwrap();

    // Null user_id
    let result = unsafe { experimentation_bucket(std::ptr::null(), valid.as_ptr(), 10_000) };
    assert_eq!(result, EXPERIMENTATION_BUCKET_ERROR);

    // Null salt
    let result = unsafe { experimentation_bucket(valid.as_ptr(), std::ptr::null(), 10_000) };
    assert_eq!(result, EXPERIMENTATION_BUCKET_ERROR);

    // Both null
    let result = unsafe { experimentation_bucket(std::ptr::null(), std::ptr::null(), 10_000) };
    assert_eq!(result, EXPERIMENTATION_BUCKET_ERROR);

    // Zero total_buckets
    let result = unsafe { experimentation_bucket(valid.as_ptr(), valid.as_ptr(), 0) };
    assert_eq!(result, EXPERIMENTATION_BUCKET_ERROR);
}

#[test]
fn ffi_allocation_check() {
    assert_eq!(experimentation_is_in_allocation(50, 0, 100), 1);
    assert_eq!(experimentation_is_in_allocation(0, 0, 100), 1);
    assert_eq!(experimentation_is_in_allocation(100, 0, 100), 1);
    assert_eq!(experimentation_is_in_allocation(101, 0, 100), 0);

    // Single-bucket
    assert_eq!(experimentation_is_in_allocation(42, 42, 42), 1);
    assert_eq!(experimentation_is_in_allocation(41, 42, 42), 0);
    assert_eq!(experimentation_is_in_allocation(43, 42, 42), 0);
}
