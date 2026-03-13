/**
 * Pure TypeScript MurmurHash3 x86 32-bit implementation.
 *
 * Reference: crates/experimentation-hash/src/murmur3.rs
 * This must produce identical output for all inputs.
 */

const C1 = 0xcc9e2d51;
const C2 = 0x1b873593;

function imul(a: number, b: number): number {
  return Math.imul(a, b);
}

function rotl32(x: number, r: number): number {
  return (x << r) | (x >>> (32 - r));
}

function fmix32(h: number): number {
  h ^= h >>> 16;
  h = imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}

/**
 * MurmurHash3 x86 32-bit, little-endian.
 * Matches the Rust reference implementation exactly.
 */
export function murmurhash3_x86_32(data: Uint8Array, seed: number): number {
  const len = data.length;
  const nBlocks = Math.floor(len / 4);
  let h1 = seed >>> 0;

  // Body: process 4-byte blocks (little-endian)
  for (let i = 0; i < nBlocks; i++) {
    const offset = i * 4;
    let k1 =
      data[offset] |
      (data[offset + 1] << 8) |
      (data[offset + 2] << 16) |
      (data[offset + 3] << 24);

    k1 = imul(k1, C1);
    k1 = rotl32(k1, 15);
    k1 = imul(k1, C2);

    h1 ^= k1;
    h1 = rotl32(h1, 13);
    h1 = (imul(h1, 5) + 0xe6546b64) | 0;
  }

  // Tail: remaining bytes
  const tailStart = nBlocks * 4;
  let k1 = 0;
  switch (len - tailStart) {
    case 3:
      k1 ^= data[tailStart + 2] << 16;
    // fallthrough
    case 2:
      k1 ^= data[tailStart + 1] << 8;
    // fallthrough
    case 1:
      k1 ^= data[tailStart];
      k1 = imul(k1, C1);
      k1 = rotl32(k1, 15);
      k1 = imul(k1, C2);
      h1 ^= k1;
  }

  // Finalization
  h1 ^= len;
  h1 = fmix32(h1);

  return h1 >>> 0;
}
