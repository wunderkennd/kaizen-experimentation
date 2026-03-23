package adaptive_test

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/org/experimentation-platform/services/management/internal/adaptive"
)

// ---------------------------------------------------------------------------
// Stub ConditionalPowerClient for unit tests
// ---------------------------------------------------------------------------

type stubCPClient struct {
	resp adaptive.ConditionalPowerResponse
	err  error
}

func (s *stubCPClient) ComputeZone(_ context.Context, _ adaptive.ConditionalPowerRequest) (adaptive.ConditionalPowerResponse, error) {
	return s.resp, s.err
}

// ---------------------------------------------------------------------------
// Zone constant validation
// ---------------------------------------------------------------------------

func TestZoneConstants(t *testing.T) {
	zones := []adaptive.Zone{
		adaptive.ZoneFavorable,
		adaptive.ZonePromising,
		adaptive.ZoneFutile,
	}
	expected := []string{"favorable", "promising", "futile"}
	for i, z := range zones {
		if string(z) != expected[i] {
			t.Errorf("zone[%d] = %q, want %q", i, z, expected[i])
		}
	}
}

// ---------------------------------------------------------------------------
// AdaptiveNConfig JSON round-trip
// ---------------------------------------------------------------------------

func TestAdaptiveNConfigRoundTrip(t *testing.T) {
	cfg := map[string]any{
		"adaptive_n_config": map[string]any{
			"interim_fraction":         0.5,
			"n_max_per_arm":            400.0,
			"alpha":                    0.05,
			"extension_ceiling":        2.0,
			"planned_duration_seconds": 1_209_600.0, // 14 days
			"interim_fired":            false,
		},
	}
	data, err := json.Marshal(cfg)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var tc map[string]json.RawMessage
	if err := json.Unmarshal(data, &tc); err != nil {
		t.Fatalf("unmarshal outer: %v", err)
	}

	raw, ok := tc["adaptive_n_config"]
	if !ok {
		t.Fatal("adaptive_n_config key missing")
	}

	type config struct {
		InterimFraction        float64 `json:"interim_fraction"`
		NMaxPerArm             float64 `json:"n_max_per_arm"`
		Alpha                  float64 `json:"alpha"`
		ExtensionCeiling       float64 `json:"extension_ceiling"`
		PlannedDurationSeconds float64 `json:"planned_duration_seconds"`
		InterimFired           bool    `json:"interim_fired"`
	}
	var parsed config
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatalf("unmarshal adaptive_n_config: %v", err)
	}
	if parsed.InterimFraction != 0.5 {
		t.Errorf("interim_fraction = %v, want 0.5", parsed.InterimFraction)
	}
	if parsed.NMaxPerArm != 400.0 {
		t.Errorf("n_max_per_arm = %v, want 400", parsed.NMaxPerArm)
	}
	if parsed.InterimFired {
		t.Error("interim_fired should be false")
	}
}

// ---------------------------------------------------------------------------
// ProcessResult skip path
// ---------------------------------------------------------------------------

func TestProcessResultSkipFields(t *testing.T) {
	r := adaptive.ProcessResult{
		ExperimentID: "exp-001",
		Skipped:      true,
		SkipReason:   "not RUNNING",
	}
	if !r.Skipped {
		t.Error("expected Skipped=true")
	}
	if r.SkipReason == "" {
		t.Error("SkipReason should be non-empty")
	}
}

// ---------------------------------------------------------------------------
// ConditionalPowerRequest field validation
// ---------------------------------------------------------------------------

func TestConditionalPowerRequestFields(t *testing.T) {
	req := adaptive.ConditionalPowerRequest{
		ExperimentID:    "exp-123",
		ObservedEffect:  0.25,
		BlinderVariance: 1.5,
		NMaxPerArm:      400.0,
		Alpha:           0.05,
	}
	if req.ExperimentID == "" {
		t.Error("ExperimentID must be set")
	}
	if req.BlinderVariance <= 0 {
		t.Error("BlinderVariance must be positive")
	}
	if req.Alpha <= 0 || req.Alpha >= 1 {
		t.Error("Alpha must be in (0,1)")
	}
}

// ---------------------------------------------------------------------------
// Zone constants are distinct
// ---------------------------------------------------------------------------

func TestZonesAreDistinct(t *testing.T) {
	zones := map[adaptive.Zone]struct{}{
		adaptive.ZoneFavorable: {},
		adaptive.ZonePromising: {},
		adaptive.ZoneFutile:    {},
	}
	if len(zones) != 3 {
		t.Error("zone constants are not distinct")
	}
}
