package experimentation

// Pure Go MurmurHash3 x86 32-bit implementation.
// Reference: crates/experimentation-hash/src/murmur3.rs
// Must produce identical output for all inputs.

const (
	c1 = 0xcc9e2d51
	c2 = 0x1b873593
)

// Murmurhash3X86_32 computes a MurmurHash3 x86 32-bit hash, little-endian.
func Murmurhash3X86_32(data []byte, seed uint32) uint32 {
	length := len(data)
	nBlocks := length / 4
	h1 := seed

	// Body: process 4-byte blocks (little-endian)
	for i := 0; i < nBlocks; i++ {
		off := i * 4
		k1 := uint32(data[off]) |
			uint32(data[off+1])<<8 |
			uint32(data[off+2])<<16 |
			uint32(data[off+3])<<24

		k1 *= c1
		k1 = (k1 << 15) | (k1 >> 17) // rotl32(k1, 15)
		k1 *= c2

		h1 ^= k1
		h1 = (h1 << 13) | (h1 >> 19) // rotl32(h1, 13)
		h1 = h1*5 + 0xe6546b64
	}

	// Tail: remaining bytes
	tail := data[nBlocks*4:]
	var k1 uint32
	switch len(tail) {
	case 3:
		k1 ^= uint32(tail[2]) << 16
		fallthrough
	case 2:
		k1 ^= uint32(tail[1]) << 8
		fallthrough
	case 1:
		k1 ^= uint32(tail[0])
		k1 *= c1
		k1 = (k1 << 15) | (k1 >> 17)
		k1 *= c2
		h1 ^= k1
	}

	// Finalization
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
