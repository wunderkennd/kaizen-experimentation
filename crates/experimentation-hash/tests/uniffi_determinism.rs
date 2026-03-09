//! UniFFI binding validation against the 10,000 hash test vectors.
//!
//! These tests call the UniFFI-exported wrapper functions to confirm they
//! produce identical results to the native Rust implementation.
//!
//! Run: cargo test --package experimentation-hash --features uniffi

#![cfg(feature = "uniffi")]

use serde::Deserialize;

#[derive(Deserialize)]
struct HashVector {
    user_id: String,
    salt: String,
    total_buckets: u32,
    expected_bucket: u32,
}

#[test]
fn uniffi_hash_vectors_10k() {
    let vectors_path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-vectors/hash_vectors.json");
    let data = std::fs::read_to_string(vectors_path)
        .unwrap_or_else(|e| panic!("Failed to read {vectors_path}: {e}"));

    let vectors: Vec<HashVector> =
        serde_json::from_str(&data).unwrap_or_else(|e| panic!("Failed to parse hash vectors: {e}"));

    assert!(!vectors.is_empty(), "test vector file is empty");

    let mut failures = 0;
    for (i, v) in vectors.iter().enumerate() {
        let actual =
            experimentation_hash::uniffi_bucket(v.user_id.clone(), v.salt.clone(), v.total_buckets);
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
        "{failures}/{} UniFFI test vectors failed. First 10 failures shown above.",
        vectors.len()
    );

    eprintln!("All {} UniFFI hash test vectors passed.", vectors.len());
}

#[test]
fn uniffi_allocation_boundary() {
    use experimentation_hash::uniffi_is_in_allocation;

    // Inclusive range: [0, 100]
    assert!(uniffi_is_in_allocation(0, 0, 100));
    assert!(uniffi_is_in_allocation(50, 0, 100));
    assert!(uniffi_is_in_allocation(100, 0, 100));
    assert!(!uniffi_is_in_allocation(101, 0, 100));

    // Single-bucket allocation
    assert!(uniffi_is_in_allocation(42, 42, 42));
    assert!(!uniffi_is_in_allocation(41, 42, 42));
    assert!(!uniffi_is_in_allocation(43, 42, 42));

    // Full range
    assert!(uniffi_is_in_allocation(0, 0, u32::MAX));
    assert!(uniffi_is_in_allocation(u32::MAX, 0, u32::MAX));
}

#[test]
fn uniffi_raw_hash_matches_native() {
    use experimentation_hash::murmur3::murmurhash3_x86_32;
    use experimentation_hash::uniffi_murmurhash3_x86_32;

    // Known values
    assert_eq!(uniffi_murmurhash3_x86_32(b"".to_vec(), 0), 0);
    assert_eq!(uniffi_murmurhash3_x86_32(b"hello".to_vec(), 0), 0x248b_fa47);
    assert_eq!(uniffi_murmurhash3_x86_32(b"hello".to_vec(), 1), 0xbb4a_bcad);

    // Parity with native
    let inputs: &[(&[u8], u32)] = &[
        (b"user_123\x00experiment_1", 0),
        (b"test", 42),
        (b"", 1),
        (b"a]longer input string for coverage", 99),
    ];
    for &(data, seed) in inputs {
        assert_eq!(
            uniffi_murmurhash3_x86_32(data.to_vec(), seed),
            murmurhash3_x86_32(data, seed),
            "UniFFI/native mismatch for data={data:?} seed={seed}"
        );
    }
}

#[test]
fn uniffi_bucket_matches_native() {
    use experimentation_hash::{bucket, uniffi_bucket};

    let cases = [
        ("user_123", "salt_abc", 10_000),
        ("alice", "exp_001", 1_000),
        ("", "empty_user", 100),
        ("bob", "", 5_000),
    ];
    for (user_id, salt, total_buckets) in cases {
        assert_eq!(
            uniffi_bucket(user_id.to_string(), salt.to_string(), total_buckets),
            bucket(user_id, salt, total_buckets),
            "UniFFI/native bucket mismatch for user={user_id:?} salt={salt:?}"
        );
    }
}
