package hash

import "testing"

func TestMurmurHash3KnownValues(t *testing.T) {
	tests := []struct {
		data []byte
		seed uint32
		want uint32
	}{
		{[]byte(""), 0, 0},
		{[]byte("hello"), 0, 0x248bfa47},
		{[]byte("hello"), 1, 0xbb4abcad},
	}
	for _, tt := range tests {
		got := MurmurHash3X86_32(tt.data, tt.seed)
		if got != tt.want {
			t.Errorf("MurmurHash3X86_32(%q, %d) = 0x%08x, want 0x%08x", tt.data, tt.seed, got, tt.want)
		}
	}
}

func TestMurmurHash3Deterministic(t *testing.T) {
	h1 := MurmurHash3X86_32([]byte("test_input"), 42)
	h2 := MurmurHash3X86_32([]byte("test_input"), 42)
	if h1 != h2 {
		t.Errorf("non-deterministic: got 0x%08x and 0x%08x", h1, h2)
	}
}

func TestBucketDeterministic(t *testing.T) {
	b1 := Bucket("user_123", "salt_abc", 10000)
	b2 := Bucket("user_123", "salt_abc", 10000)
	if b1 != b2 {
		t.Errorf("Bucket not deterministic: got %d and %d", b1, b2)
	}
}

func TestBucketRange(t *testing.T) {
	for i := 0; i < 1000; i++ {
		b := Bucket("user", "salt_"+string(rune(i)), 10000)
		if b >= 10000 {
			t.Errorf("Bucket out of range: got %d", b)
		}
	}
}

func TestBucketPanicsOnZero(t *testing.T) {
	defer func() {
		if r := recover(); r == nil {
			t.Error("expected panic on totalBuckets=0")
		}
	}()
	Bucket("user", "salt", 0)
}

func TestMonotonicRollout(t *testing.T) {
	// Users in rollout at 10% must still be in rollout at 20%.
	salt := "monotonic_test_salt"
	var usersAt10 []string

	for i := 0; i < 10000; i++ {
		userID := "user_" + string(rune('A'+i/26/26)) + string(rune('A'+i/26%26)) + string(rune('A'+i%26))
		bucket := Bucket(userID, salt, 10000)
		if bucket < 1000 { // 10% rollout
			usersAt10 = append(usersAt10, userID)
		}
	}

	for _, userID := range usersAt10 {
		bucket := Bucket(userID, salt, 10000)
		if bucket >= 2000 { // 20% rollout
			t.Errorf("user %s was in 10%% rollout (bucket %d) but not in 20%% rollout", userID, bucket)
		}
	}
}
