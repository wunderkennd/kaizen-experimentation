//go:build cgo && has_ffi

package hash

import (
	"fmt"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
)

// BenchmarkCGoBucket benchmarks the CGo bridge path (Rust FFI via C).
func BenchmarkCGoBucket(b *testing.B) {
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		Bucket("user_123", "flag_salt_abc123", 10000)
	}
}

// BenchmarkCGoBucket_Parallel measures CGo bridge throughput under goroutine contention.
func BenchmarkCGoBucket_Parallel(b *testing.B) {
	b.RunParallel(func(pb *testing.PB) {
		i := 0
		for pb.Next() {
			Bucket(fmt.Sprintf("user_%d", i), "flag_salt_abc123", 10000)
			i++
		}
	})
}

// BenchmarkPureGoBucket benchmarks the pure-Go MurmurHash3 path directly.
// Since MurmurHash3X86_32 is always compiled (no build tag), we can call it
// even when the CGo bridge is active, allowing direct comparison.
func BenchmarkPureGoBucket(b *testing.B) {
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		key := fmt.Sprintf("%s\x00%s", "user_123", "flag_salt_abc123")
		h := MurmurHash3X86_32([]byte(key), 0)
		_ = h % 10000
	}
}

// BenchmarkPureGoBucket_Parallel measures pure-Go throughput under goroutine contention.
func BenchmarkPureGoBucket_Parallel(b *testing.B) {
	b.RunParallel(func(pb *testing.PB) {
		i := 0
		for pb.Next() {
			key := fmt.Sprintf("%s\x00%s", fmt.Sprintf("user_%d", i), "flag_salt_abc123")
			h := MurmurHash3X86_32([]byte(key), 0)
			_ = h % 10000
			i++
		}
	})
}

// BenchmarkCGoBucket_PreallocatedKeys isolates CGo call overhead from string
// allocation by pre-generating keys. This measures the true FFI call cost.
func BenchmarkCGoBucket_PreallocatedKeys(b *testing.B) {
	userIDs := make([]string, 1000)
	for i := range userIDs {
		userIDs[i] = fmt.Sprintf("user_%d", i)
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		Bucket(userIDs[i%1000], "flag_salt_abc123", 10000)
	}
}

// BenchmarkPureGoBucket_PreallocatedKeys isolates pure-Go hash from allocation.
func BenchmarkPureGoBucket_PreallocatedKeys(b *testing.B) {
	keys := make([][]byte, 1000)
	for i := range keys {
		keys[i] = []byte(fmt.Sprintf("user_%d\x00flag_salt_abc123", i))
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		h := MurmurHash3X86_32(keys[i%1000], 0)
		_ = h % 10000
	}
}

// TestCGo_OverheadSubMicrosecond measures CGo bridge overhead and asserts it
// is under 1μs per call. The overhead is the difference between CGo FFI and
// pure-Go hash calls.
func TestCGo_OverheadSubMicrosecond(t *testing.T) {
	const iterations = 100000

	// Pre-generate keys to isolate hash cost.
	userIDs := make([]string, iterations)
	keys := make([][]byte, iterations)
	for i := range userIDs {
		userIDs[i] = fmt.Sprintf("user_%d", i)
		keys[i] = []byte(fmt.Sprintf("user_%d\x00test_salt", i))
	}

	// Warm up both paths.
	for i := 0; i < 1000; i++ {
		Bucket(userIDs[i], "test_salt", 10000)
		h := MurmurHash3X86_32(keys[i], 0)
		_ = h % 10000
	}

	// Measure CGo bridge.
	startCGo := time.Now()
	for i := 0; i < iterations; i++ {
		Bucket(userIDs[i], "test_salt", 10000)
	}
	cgoElapsed := time.Since(startCGo)

	// Measure pure Go.
	startPureGo := time.Now()
	for i := 0; i < iterations; i++ {
		h := MurmurHash3X86_32(keys[i], 0)
		_ = h % 10000
	}
	pureGoElapsed := time.Since(startPureGo)

	cgoAvg := float64(cgoElapsed.Nanoseconds()) / float64(iterations)
	pureGoAvg := float64(pureGoElapsed.Nanoseconds()) / float64(iterations)
	overheadNs := cgoAvg - pureGoAvg

	t.Logf("CGo bridge:     %.1f ns/call (n=%d)", cgoAvg, iterations)
	t.Logf("Pure Go hash:   %.1f ns/call (n=%d)", pureGoAvg, iterations)
	t.Logf("CGo overhead:   %.1f ns/call (%.1f%%)", overheadNs, (overheadNs/pureGoAvg)*100)
	t.Logf("CGo total:      %.1f ns/call (target: < 1000ns = 1μs)", cgoAvg)

	// Assert total CGo call time is under 1μs.
	assert.Less(t, cgoAvg, 1000.0,
		"SLA violation: CGo Bucket() = %.1f ns (target: < 1000ns = 1μs)", cgoAvg)
}

// TestCGo_DeterminismParity ensures CGo bridge and pure-Go produce identical results.
func TestCGo_DeterminismParity(t *testing.T) {
	testCases := []struct {
		userID       string
		salt         string
		totalBuckets uint32
	}{
		{"user_123", "salt_abc", 10000},
		{"", "empty_user", 10000},
		{"user_with_unicode_日本語", "salt", 10000},
		{"user_123", "", 10000},
		{"a", "b", 100},
		{"long_user_id_1234567890_abcdefghijklmnopqrstuvwxyz", "long_salt_0987654321", 10000},
	}

	for _, tc := range testCases {
		t.Run(fmt.Sprintf("%s/%s/%d", tc.userID, tc.salt, tc.totalBuckets), func(t *testing.T) {
			// CGo bridge result.
			cgoBucket := Bucket(tc.userID, tc.salt, tc.totalBuckets)

			// Pure Go result.
			key := fmt.Sprintf("%s\x00%s", tc.userID, tc.salt)
			h := MurmurHash3X86_32([]byte(key), 0)
			pureGoBucket := h % tc.totalBuckets

			assert.Equal(t, pureGoBucket, cgoBucket,
				"Parity mismatch: CGo=%d PureGo=%d for user=%q salt=%q buckets=%d",
				cgoBucket, pureGoBucket, tc.userID, tc.salt, tc.totalBuckets)
		})
	}
}
