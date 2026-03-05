//go:build cgo && has_ffi

package hash

import (
	"encoding/json"
	"fmt"
	"os"
	"testing"
)

// TestCGoBridgeParityAllVectors validates that the CGo bridge (calling Rust FFI)
// produces identical bucket assignments to the pure-Go implementation for all
// 10,000 test vectors. This is the critical parity validation.
func TestCGoBridgeParityAllVectors(t *testing.T) {
	data, err := os.ReadFile("../../../../test-vectors/hash_vectors.json")
	if err != nil {
		t.Fatalf("failed to read test vectors: %v", err)
	}

	type vector struct {
		UserID       string `json:"user_id"`
		Salt         string `json:"salt"`
		TotalBuckets uint32 `json:"total_buckets"`
		Expected     uint32 `json:"expected_bucket"`
	}

	var vectors []vector
	if err := json.Unmarshal(data, &vectors); err != nil {
		t.Fatalf("failed to parse test vectors: %v", err)
	}

	t.Logf("validating %d test vectors through CGo bridge", len(vectors))

	// The Bucket function in this build uses the CGo bridge (bucket_cgo.go).
	// Verify it matches expected values from test vectors.
	mismatches := 0
	for i, v := range vectors {
		got := Bucket(v.UserID, v.Salt, v.TotalBuckets)
		if got != v.Expected {
			mismatches++
			if mismatches <= 10 {
				t.Errorf("vector %d: CGo Bucket(%q, %q, %d) = %d, want %d",
					i, v.UserID, v.Salt, v.TotalBuckets, got, v.Expected)
			}
		}
	}

	if mismatches > 0 {
		t.Fatalf("CGo bridge failed: %d/%d vectors mismatched", mismatches, len(vectors))
	}

	t.Logf("Hash parity confirmed: all %d vectors match between Go (CGo) and Rust native", len(vectors))
}

// TestCGoBridgeBasicValues validates basic known hash values through CGo.
func TestCGoBridgeBasicValues(t *testing.T) {
	// These should produce deterministic, known results.
	b1 := Bucket("user_123", "test_salt", 10000)
	b2 := Bucket("user_123", "test_salt", 10000)
	if b1 != b2 {
		t.Errorf("CGo bridge not deterministic: got %d and %d", b1, b2)
	}

	// Verify range.
	for i := 0; i < 100; i++ {
		b := Bucket(fmt.Sprintf("user_%d", i), "salt", 10000)
		if b >= 10000 {
			t.Errorf("CGo Bucket out of range: got %d", b)
		}
	}
}
