package experimentation

import (
	"context"
	"fmt"
	"testing"
)

// ---------------------------------------------------------------------------
// MurmurHash3 parity tests
// ---------------------------------------------------------------------------

func TestMurmurhash3KnownValues(t *testing.T) {
	tests := []struct {
		data     string
		seed     uint32
		expected uint32
	}{
		{"", 0, 0},
		{"hello", 0, 0x248bfa47},
		{"hello", 1, 0xbb4abcad},
	}
	for _, tt := range tests {
		got := Murmurhash3X86_32([]byte(tt.data), tt.seed)
		if got != tt.expected {
			t.Errorf("Murmurhash3X86_32(%q, %d) = 0x%08x, want 0x%08x", tt.data, tt.seed, got, tt.expected)
		}
	}
}

func TestMurmurhash3Deterministic(t *testing.T) {
	h1 := Murmurhash3X86_32([]byte("test_input"), 42)
	h2 := Murmurhash3X86_32([]byte("test_input"), 42)
	if h1 != h2 {
		t.Errorf("non-deterministic: got %d and %d", h1, h2)
	}
}

// ---------------------------------------------------------------------------
// Bucket parity with test vectors (from test-vectors/hash_vectors.json)
// ---------------------------------------------------------------------------

func TestBucketParity(t *testing.T) {
	vectors := []struct {
		userID         string
		salt           string
		totalBuckets   uint32
		expectedBucket uint32
	}{
		{"user_000000", "experiment_default_salt", 10000, 3913},
		{"user_000001", "experiment_default_salt", 10000, 4234},
		{"user_000002", "experiment_default_salt", 10000, 5578},
		{"user_000003", "experiment_default_salt", 10000, 8009},
		{"user_000004", "experiment_default_salt", 10000, 2419},
		{"user_000005", "experiment_default_salt", 10000, 5885},
		{"user_000006", "experiment_default_salt", 10000, 5586},
		{"user_000007", "experiment_default_salt", 10000, 9853},
		{"user_000008", "experiment_default_salt", 10000, 2730},
		{"user_000009", "experiment_default_salt", 10000, 27},
	}
	for _, v := range vectors {
		got := computeBucket(v.userID, v.salt, v.totalBuckets)
		if got != v.expectedBucket {
			t.Errorf("computeBucket(%q, %q, %d) = %d, want %d",
				v.userID, v.salt, v.totalBuckets, got, v.expectedBucket)
		}
	}
}

// ---------------------------------------------------------------------------
// LocalProvider tests
// ---------------------------------------------------------------------------

var twoVariantConfig = ExperimentConfig{
	ExperimentID:    "exp_ab_test",
	HashSalt:        "salt_ab",
	LayerName:       "default",
	AllocationStart: 0,
	AllocationEnd:   9999,
	TotalBuckets:    10000,
	Variants: []VariantConfig{
		{Name: "control", TrafficFraction: 0.5, IsControl: true, Payload: map[string]any{"color": "blue"}},
		{Name: "treatment", TrafficFraction: 0.5, IsControl: false, Payload: map[string]any{"color": "red"}},
	},
}

var threeVariantConfig = ExperimentConfig{
	ExperimentID:    "exp_abc",
	HashSalt:        "salt_abc",
	LayerName:       "default",
	AllocationStart: 0,
	AllocationEnd:   9999,
	TotalBuckets:    10000,
	Variants: []VariantConfig{
		{Name: "control", TrafficFraction: 0.34, IsControl: true},
		{Name: "variant_a", TrafficFraction: 0.33, IsControl: false},
		{Name: "variant_b", TrafficFraction: 0.33, IsControl: false},
	},
}

func TestLocalProviderUnknownExperiment(t *testing.T) {
	p := NewLocalProvider([]ExperimentConfig{twoVariantConfig})
	a, err := p.GetAssignment(context.Background(), "nonexistent", UserAttributes{UserID: "user1"})
	if err != nil {
		t.Fatal(err)
	}
	if a != nil {
		t.Error("expected nil for unknown experiment")
	}
}

func TestLocalProviderDeterministic(t *testing.T) {
	p := NewLocalProvider([]ExperimentConfig{twoVariantConfig})
	ctx := context.Background()
	attrs := UserAttributes{UserID: "user_stable_123"}

	a1, err := p.GetAssignment(ctx, "exp_ab_test", attrs)
	if err != nil {
		t.Fatal(err)
	}
	a2, err := p.GetAssignment(ctx, "exp_ab_test", attrs)
	if err != nil {
		t.Fatal(err)
	}
	if a1 == nil || a2 == nil {
		t.Fatal("expected non-nil assignment")
	}
	if a1.VariantName != a2.VariantName {
		t.Errorf("non-deterministic: got %q and %q", a1.VariantName, a2.VariantName)
	}
}

func TestLocalProviderFromCache(t *testing.T) {
	p := NewLocalProvider([]ExperimentConfig{twoVariantConfig})
	a, err := p.GetAssignment(context.Background(), "exp_ab_test", UserAttributes{UserID: "user1"})
	if err != nil {
		t.Fatal(err)
	}
	if a == nil {
		t.Fatal("expected non-nil")
	}
	if !a.FromCache {
		t.Error("expected FromCache=true")
	}
}

func TestLocalProviderExclusion(t *testing.T) {
	narrow := ExperimentConfig{
		ExperimentID:    "exp_narrow",
		HashSalt:        "salt_ab",
		LayerName:       "default",
		AllocationStart: 0,
		AllocationEnd:   0, // only bucket 0
		TotalBuckets:    10000,
		Variants:        twoVariantConfig.Variants,
	}
	p := NewLocalProvider([]ExperimentConfig{narrow})
	ctx := context.Background()

	nullCount := 0
	for i := 0; i < 50; i++ {
		a, err := p.GetAssignment(ctx, "exp_narrow", UserAttributes{UserID: fmt.Sprintf("exclude_test_%d", i)})
		if err != nil {
			t.Fatal(err)
		}
		if a == nil {
			nullCount++
		}
	}
	if nullCount < 40 {
		t.Errorf("expected most users excluded, got %d/50 nil", nullCount)
	}
}

func TestLocalProviderDistribution(t *testing.T) {
	p := NewLocalProvider([]ExperimentConfig{twoVariantConfig})
	ctx := context.Background()
	counts := map[string]int{"control": 0, "treatment": 0}

	for i := 0; i < 1000; i++ {
		a, err := p.GetAssignment(ctx, "exp_ab_test", UserAttributes{UserID: fmt.Sprintf("dist_user_%d", i)})
		if err != nil {
			t.Fatal(err)
		}
		if a != nil {
			counts[a.VariantName]++
		}
	}

	if counts["control"] < 350 {
		t.Errorf("control count too low: %d", counts["control"])
	}
	if counts["treatment"] < 350 {
		t.Errorf("treatment count too low: %d", counts["treatment"])
	}
}

func TestLocalProviderThreeVariants(t *testing.T) {
	p := NewLocalProvider([]ExperimentConfig{threeVariantConfig})
	ctx := context.Background()
	variants := make(map[string]bool)

	for i := 0; i < 500; i++ {
		a, err := p.GetAssignment(ctx, "exp_abc", UserAttributes{UserID: fmt.Sprintf("three_var_%d", i)})
		if err != nil {
			t.Fatal(err)
		}
		if a != nil {
			variants[a.VariantName] = true
		}
	}

	if len(variants) != 3 {
		t.Errorf("expected 3 variants, got %d: %v", len(variants), variants)
	}
}

func TestLocalProviderFPRoundingFallback(t *testing.T) {
	fpConfig := ExperimentConfig{
		ExperimentID:    "exp_fp",
		HashSalt:        "salt_fp",
		LayerName:       "default",
		AllocationStart: 0,
		AllocationEnd:   9999,
		TotalBuckets:    10000,
		Variants: []VariantConfig{
			{Name: "a", TrafficFraction: 0.333, IsControl: true},
			{Name: "b", TrafficFraction: 0.333, IsControl: false},
			{Name: "c", TrafficFraction: 0.334, IsControl: false},
		},
	}
	p := NewLocalProvider([]ExperimentConfig{fpConfig})
	ctx := context.Background()

	valid := map[string]bool{"a": true, "b": true, "c": true}
	for i := 0; i < 100; i++ {
		a, err := p.GetAssignment(ctx, "exp_fp", UserAttributes{UserID: fmt.Sprintf("fp_user_%d", i)})
		if err != nil {
			t.Fatal(err)
		}
		if a == nil {
			t.Fatalf("expected non-nil for fp_user_%d", i)
		}
		if !valid[a.VariantName] {
			t.Errorf("unexpected variant %q", a.VariantName)
		}
	}
}

func TestLocalProviderGetAllAssignments(t *testing.T) {
	p := NewLocalProvider([]ExperimentConfig{twoVariantConfig, threeVariantConfig})
	ctx := context.Background()
	results, err := p.GetAllAssignments(ctx, UserAttributes{UserID: "multi_user_1"})
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 2 {
		t.Errorf("expected 2 assignments, got %d", len(results))
	}
	if _, ok := results["exp_ab_test"]; !ok {
		t.Error("missing exp_ab_test")
	}
	if _, ok := results["exp_abc"]; !ok {
		t.Error("missing exp_abc")
	}
}
