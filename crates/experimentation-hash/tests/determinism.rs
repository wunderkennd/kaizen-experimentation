//! Load test-vectors/hash_vectors.json and verify every vector matches.
//!
//! Run: cargo test --package experimentation-hash -- hash_vectors

use experimentation_hash::bucket;
use serde::Deserialize;

#[derive(Deserialize)]
struct HashVector {
    user_id: String,
    salt: String,
    total_buckets: u32,
    expected_bucket: u32,
}

#[test]
fn hash_vectors() {
    let vectors_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test-vectors/hash_vectors.json");
    let data = std::fs::read_to_string(vectors_path)
        .unwrap_or_else(|e| panic!("Failed to read {vectors_path}: {e}"));

    let vectors: Vec<HashVector> = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse hash vectors: {e}"));

    assert!(!vectors.is_empty(), "test vector file is empty");

    let mut failures = 0;
    for (i, v) in vectors.iter().enumerate() {
        let actual = bucket(&v.user_id, &v.salt, v.total_buckets);
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
        "{failures}/{} test vectors failed. First 10 failures shown above.",
        vectors.len()
    );

    eprintln!("All {} hash test vectors passed.", vectors.len());
}
