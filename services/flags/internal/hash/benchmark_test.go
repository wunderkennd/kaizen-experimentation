package hash

import (
	"fmt"
	"testing"
)

// BenchmarkMurmurHash3 benchmarks the raw MurmurHash3 x86 32-bit function.
func BenchmarkMurmurHash3(b *testing.B) {
	data := []byte("user_123\x00flag_salt_abc123")
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		MurmurHash3X86_32(data, 0)
	}
}

// BenchmarkBucket benchmarks a single Bucket call (hash + modulo).
// Target: < 1us/op.
func BenchmarkBucket(b *testing.B) {
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		Bucket("user_123", "flag_salt_abc123", 10000)
	}
}

// BenchmarkBucket_Parallel measures Bucket throughput under goroutine contention.
func BenchmarkBucket_Parallel(b *testing.B) {
	b.RunParallel(func(pb *testing.PB) {
		i := 0
		for pb.Next() {
			Bucket(fmt.Sprintf("user_%d", i), "flag_salt_abc123", 10000)
			i++
		}
	})
}

// BenchmarkBucket_VaryingKeyLengths benchmarks Bucket with different user ID sizes.
func BenchmarkBucket_VaryingKeyLengths(b *testing.B) {
	cases := []struct {
		name   string
		userID string
	}{
		{"short", "u1"},
		{"medium", "user_1234567890"},
		{"long", "user_1234567890_abcdefghij_klmnopqrst_uvwxyz_0123456789"},
	}
	for _, tc := range cases {
		b.Run(tc.name, func(b *testing.B) {
			for i := 0; i < b.N; i++ {
				Bucket(tc.userID, "salt", 10000)
			}
		})
	}
}
