#!/usr/bin/env python3
"""
Generate 10,000 hash test vectors for cross-language parity validation.

This is the REFERENCE implementation. The Rust MurmurHash3 in
experimentation-hash/src/murmur3.rs must produce identical results.

Output: test-vectors/hash_vectors.json
Format: [{"user_id": "...", "salt": "...", "total_buckets": 10000, "expected_bucket": N}, ...]

Run: python scripts/generate_hash_vectors.py
"""

import json
import struct
import hashlib

def murmurhash3_x86_32(data: bytes, seed: int = 0) -> int:
    """MurmurHash3 x86 32-bit, matching the Rust implementation."""
    C1 = 0xcc9e2d51
    C2 = 0x1b873593
    MASK32 = 0xFFFFFFFF

    def rotl32(x, r):
        return ((x << r) | (x >> (32 - r))) & MASK32

    def fmix32(h):
        h ^= h >> 16
        h = (h * 0x85ebca6b) & MASK32
        h ^= h >> 13
        h = (h * 0xc2b2ae35) & MASK32
        h ^= h >> 16
        return h

    length = len(data)
    nblocks = length // 4
    h1 = seed & MASK32

    # Body
    for i in range(nblocks):
        offset = i * 4
        k1 = struct.unpack_from('<I', data, offset)[0]

        k1 = (k1 * C1) & MASK32
        k1 = rotl32(k1, 15)
        k1 = (k1 * C2) & MASK32

        h1 ^= k1
        h1 = rotl32(h1, 13)
        h1 = (h1 * 5 + 0xe6546b64) & MASK32

    # Tail
    tail = data[nblocks * 4:]
    k1 = 0
    tlen = len(tail)

    if tlen >= 3:
        k1 ^= tail[2] << 16
    if tlen >= 2:
        k1 ^= tail[1] << 8
    if tlen >= 1:
        k1 ^= tail[0]
        k1 = (k1 * C1) & MASK32
        k1 = rotl32(k1, 15)
        k1 = (k1 * C2) & MASK32
        h1 ^= k1

    # Finalization
    h1 ^= length
    h1 = fmix32(h1) & MASK32

    return h1


def bucket(user_id: str, salt: str, total_buckets: int) -> int:
    key = f"{user_id}\x00{salt}"
    h = murmurhash3_x86_32(key.encode('utf-8'), 0)
    return h % total_buckets


def main():
    vectors = []
    total_buckets = 10000

    # Pattern 1: Sequential user IDs with fixed salt (5000 vectors)
    for i in range(5000):
        user_id = f"user_{i:06d}"
        salt = "experiment_default_salt"
        b = bucket(user_id, salt, total_buckets)
        vectors.append({
            "user_id": user_id,
            "salt": salt,
            "total_buckets": total_buckets,
            "expected_bucket": b,
        })

    # Pattern 2: Fixed user with varying salts (3000 vectors)
    for i in range(3000):
        user_id = "fixed_user_for_salt_test"
        salt = f"exp_{i:04d}_salt"
        b = bucket(user_id, salt, total_buckets)
        vectors.append({
            "user_id": user_id,
            "salt": salt,
            "total_buckets": total_buckets,
            "expected_bucket": b,
        })

    # Pattern 3: Edge cases (2000 vectors)
    edge_cases = [
        ("", "empty_user"),
        ("a", "single_char"),
        ("user" * 100, "long_user"),
        ("用户123", "unicode_user"),
        ("user\ttab", "tab_in_user"),
        ("user with spaces", "space_in_user"),
    ]
    for user_id, salt in edge_cases:
        b = bucket(user_id, salt, total_buckets)
        vectors.append({
            "user_id": user_id,
            "salt": salt,
            "total_buckets": total_buckets,
            "expected_bucket": b,
        })

    # Fill remaining with UUIDs
    import uuid
    # Use deterministic UUIDs via namespace
    ns = uuid.UUID("12345678-1234-5678-1234-567812345678")
    for i in range(2000 - len(edge_cases)):
        user_id = str(uuid.uuid5(ns, f"user_{i}"))
        salt = str(uuid.uuid5(ns, f"salt_{i}"))
        b = bucket(user_id, salt, total_buckets)
        vectors.append({
            "user_id": user_id,
            "salt": salt,
            "total_buckets": total_buckets,
            "expected_bucket": b,
        })

    output_path = "test-vectors/hash_vectors.json"
    with open(output_path, 'w') as f:
        json.dump(vectors, f, indent=None, ensure_ascii=False)

    print(f"Generated {len(vectors)} test vectors → {output_path}")

    # Sanity check: verify determinism
    for v in vectors[:10]:
        b2 = bucket(v["user_id"], v["salt"], v["total_buckets"])
        assert b2 == v["expected_bucket"], f"Determinism check failed for {v['user_id']}"
    print("Determinism check passed.")


if __name__ == "__main__":
    main()
