//! MurmurHash3 x86 32-bit implementation.
//!
//! Reference: https://github.com/aappleby/smhasher/blob/master/src/MurmurHash3.cpp
//!
//! This implementation uses LITTLE-ENDIAN byte order.
//! WASM also runs little-endian, so results are consistent across targets.

const C1: u32 = 0xcc9e_2d51;
const C2: u32 = 0x1b87_3593;

/// MurmurHash3 x86 32-bit hash function.
///
/// This is the reference implementation. All other targets (WASM, FFI, UniFFI, PyO3)
/// must produce identical output for the same inputs.
pub fn murmurhash3_x86_32(data: &[u8], seed: u32) -> u32 {
    let len = data.len();
    let n_blocks = len / 4;
    let mut h1 = seed;

    // Body: process 4-byte blocks
    for i in 0..n_blocks {
        let offset = i * 4;
        let k1 = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);

        let k1 = k1.wrapping_mul(C1);
        let k1 = k1.rotate_left(15);
        let k1 = k1.wrapping_mul(C2);

        h1 ^= k1;
        h1 = h1.rotate_left(13);
        h1 = h1.wrapping_mul(5).wrapping_add(0xe654_6b64);
    }

    // Tail: process remaining bytes
    let tail = &data[n_blocks * 4..];
    let mut k1: u32 = 0;

    match tail.len() {
        3 => {
            k1 ^= (tail[2] as u32) << 16;
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        2 => {
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        1 => {
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        _ => {}
    }

    // Finalization mix
    h1 ^= len as u32;
    h1 = fmix32(h1);

    h1
}

#[inline]
fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_values() {
        // Empty string with seed 0
        assert_eq!(murmurhash3_x86_32(b"", 0), 0);

        // Known test vectors from the reference implementation
        assert_eq!(murmurhash3_x86_32(b"hello", 0), 0x248b_fa47);
        assert_eq!(murmurhash3_x86_32(b"hello", 1), 0xbb4a_bccb);
    }

    #[test]
    fn test_deterministic() {
        let h1 = murmurhash3_x86_32(b"test_input", 42);
        let h2 = murmurhash3_x86_32(b"test_input", 42);
        assert_eq!(h1, h2);
    }
}
