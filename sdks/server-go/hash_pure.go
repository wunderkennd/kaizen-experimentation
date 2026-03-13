//go:build !cgo || !has_ffi

package experimentation

import "fmt"

// computeBucket uses the pure Go MurmurHash3 implementation when CGo/FFI is unavailable.
func computeBucket(userID, salt string, totalBuckets uint32) uint32 {
	if totalBuckets == 0 {
		panic("totalBuckets must be > 0")
	}

	key := fmt.Sprintf("%s\x00%s", userID, salt)
	hash := Murmurhash3X86_32([]byte(key), 0)
	return hash % totalBuckets
}

// isInAllocation checks if a bucket is within [start, end] (inclusive).
func isInAllocation(bucket, start, end uint32) bool {
	return bucket >= start && bucket <= end
}
