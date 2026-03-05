package hash

import (
	"encoding/binary"
	"math/bits"
)

const (
	c1 uint32 = 0xcc9e2d51
	c2 uint32 = 0x1b873593
)

// MurmurHash3X86_32 computes the MurmurHash3 x86 32-bit hash.
// This is a direct port of the Rust implementation at
// crates/experimentation-hash/src/murmur3.rs.
func MurmurHash3X86_32(data []byte, seed uint32) uint32 {
	length := len(data)
	nBlocks := length / 4
	h1 := seed

	// Body: process 4-byte blocks.
	for i := 0; i < nBlocks; i++ {
		offset := i * 4
		k1 := binary.LittleEndian.Uint32(data[offset : offset+4])

		k1 *= c1
		k1 = bits.RotateLeft32(k1, 15)
		k1 *= c2

		h1 ^= k1
		h1 = bits.RotateLeft32(h1, 13)
		h1 = h1*5 + 0xe6546b64
	}

	// Tail: process remaining bytes.
	tail := data[nBlocks*4:]
	var k1 uint32

	switch len(tail) {
	case 3:
		k1 ^= uint32(tail[2]) << 16
		k1 ^= uint32(tail[1]) << 8
		k1 ^= uint32(tail[0])
		k1 *= c1
		k1 = bits.RotateLeft32(k1, 15)
		k1 *= c2
		h1 ^= k1
	case 2:
		k1 ^= uint32(tail[1]) << 8
		k1 ^= uint32(tail[0])
		k1 *= c1
		k1 = bits.RotateLeft32(k1, 15)
		k1 *= c2
		h1 ^= k1
	case 1:
		k1 ^= uint32(tail[0])
		k1 *= c1
		k1 = bits.RotateLeft32(k1, 15)
		k1 *= c2
		h1 ^= k1
	}

	// Finalization mix.
	h1 ^= uint32(length)
	h1 = fmix32(h1)

	return h1
}

func fmix32(h uint32) uint32 {
	h ^= h >> 16
	h *= 0x85ebca6b
	h ^= h >> 13
	h *= 0xc2b2ae35
	h ^= h >> 16
	return h
}
