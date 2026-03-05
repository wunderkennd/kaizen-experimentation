//go:build !cgo || !has_ffi

package hash

import "fmt"

// Bucket computes a deterministic bucket assignment for a user+salt pair.
// Key format: "{userID}\x00{salt}", matching the Rust implementation.
// This is the pure-Go fallback; the CGo version overrides when Rust FFI is available.
func Bucket(userID, salt string, totalBuckets uint32) uint32 {
	if totalBuckets == 0 {
		panic("totalBuckets must be > 0")
	}
	key := fmt.Sprintf("%s\x00%s", userID, salt)
	h := MurmurHash3X86_32([]byte(key), 0)
	return h % totalBuckets
}
