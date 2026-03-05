package hash

import (
	"encoding/json"
	"os"
	"testing"
)

type hashVector struct {
	UserID       string `json:"user_id"`
	Salt         string `json:"salt"`
	TotalBuckets uint32 `json:"total_buckets"`
	Expected     uint32 `json:"expected_bucket"`
}

func TestHashVectors(t *testing.T) {
	data, err := os.ReadFile("../../../../test-vectors/hash_vectors.json")
	if err != nil {
		t.Fatalf("failed to read test vectors: %v", err)
	}

	var vectors []hashVector
	if err := json.Unmarshal(data, &vectors); err != nil {
		t.Fatalf("failed to parse test vectors: %v", err)
	}

	t.Logf("validating %d test vectors", len(vectors))

	for i, v := range vectors {
		got := Bucket(v.UserID, v.Salt, v.TotalBuckets)
		if got != v.Expected {
			t.Errorf("vector %d: Bucket(%q, %q, %d) = %d, want %d",
				i, v.UserID, v.Salt, v.TotalBuckets, got, v.Expected)
		}
	}
}
