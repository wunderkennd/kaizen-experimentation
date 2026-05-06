package compute

import (
	"strings"
	"testing"
)

// ---------------------------------------------------------------------------
// Tier assignments
// ---------------------------------------------------------------------------

func TestServiceSpecsTierAssignment(t *testing.T) {
	specs := serviceSpecs()

	// Expected tier assignments per the dependency graph:
	//   Tier 0: M5
	//   Tier 1: M1, M2, M2-Orch
	//   Tier 2: M3, M4a, M6, M7
	expected := map[string]int{
		"m5":      TierFoundation,
		"m1":      TierCore,
		"m2":      TierCore,
		"m2-orch": TierCore,
		"m3":      TierDependent,
		"m4a":     TierDependent,
		"m6":      TierDependent,
		"m7":      TierDependent,
	}

	for _, spec := range specs {
		want, ok := expected[spec.key]
		if !ok {
			t.Errorf("unexpected service key %q in specs", spec.key)
			continue
		}
		if spec.tier != want {
			t.Errorf("service %q: tier = %d, want %d", spec.key, spec.tier, want)
		}
		delete(expected, spec.key)
	}

	for key := range expected {
		t.Errorf("missing service %q in specs", key)
	}
}

func TestAllEightFargateServicesPresent(t *testing.T) {
	specs := serviceSpecs()
	if len(specs) != 8 {
		t.Fatalf("expected 8 Fargate service specs, got %d", len(specs))
	}

	keys := make(map[string]bool, len(specs))
	for _, s := range specs {
		if keys[s.key] {
			t.Errorf("duplicate service key: %q", s.key)
		}
		keys[s.key] = true
	}
}

func TestTierCounts(t *testing.T) {
	specs := serviceSpecs()
	counts := map[int]int{}
	for _, s := range specs {
		counts[s.tier]++
	}

	tests := []struct {
		tier int
		want int
	}{
		{TierFoundation, 1}, // M5 only
		{TierCore, 3},       // M1, M2, M2-Orch
		{TierDependent, 4},  // M3, M4a, M6, M7
	}

	for _, tt := range tests {
		if got := counts[tt.tier]; got != tt.want {
			t.Errorf("tier %d: got %d services, want %d", tt.tier, got, tt.want)
		}
	}
}

// ---------------------------------------------------------------------------
// Dependency graph integrity
// ---------------------------------------------------------------------------

func TestTier0HasNoDeps(t *testing.T) {
	specs := serviceSpecs()
	for _, s := range specs {
		if s.tier == TierFoundation && len(s.deps) > 0 {
			t.Errorf("tier 0 service %q should have no deps, has %d", s.key, len(s.deps))
		}
	}
}

func TestTier1DependsOnM5(t *testing.T) {
	specs := serviceSpecs()
	for _, s := range specs {
		if s.tier != TierCore {
			continue
		}
		if len(s.deps) == 0 {
			t.Errorf("tier 1 service %q has no deps (should depend on M5)", s.key)
			continue
		}

		found := false
		for _, d := range s.deps {
			if strings.Contains(d.host, "m5-management") {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("tier 1 service %q deps do not include M5", s.key)
		}
	}
}

func TestTier2DependsOnTier1Services(t *testing.T) {
	specs := serviceSpecs()

	requiredHosts := map[string]bool{
		"m1-assignment.kaizen.local": true,
		"m2-pipeline.kaizen.local":   true,
		"m4b-policy.kaizen.local":    true,
	}

	for _, s := range specs {
		if s.tier != TierDependent {
			continue
		}
		if len(s.deps) == 0 {
			t.Errorf("tier 2 service %q has no deps", s.key)
			continue
		}

		foundHosts := map[string]bool{}
		for _, d := range s.deps {
			foundHosts[d.host] = true
		}

		for host := range requiredHosts {
			if !foundHosts[host] {
				t.Errorf("tier 2 service %q missing dep on %s", s.key, host)
			}
		}
	}
}

func TestNoCyclicDependencies(t *testing.T) {
	// Services in tier N should never depend on services in tier >= N.
	specs := serviceSpecs()

	// Build a map of Cloud Map host → tier.
	hostTier := map[string]int{}
	for _, s := range specs {
		host := s.name + ".kaizen.local"
		hostTier[host] = s.tier
	}
	// M4b is logically Tier 1 (not in Fargate specs).
	hostTier["m4b-policy.kaizen.local"] = TierCore

	for _, s := range specs {
		for _, d := range s.deps {
			depTier, ok := hostTier[d.host]
			if !ok {
				continue // External dep, skip.
			}
			if depTier >= s.tier {
				t.Errorf("service %q (tier %d) depends on %s (tier %d): would create cycle",
					s.key, s.tier, d.host, depTier)
			}
		}
	}
}

// ---------------------------------------------------------------------------
// Health-gate command generation
// ---------------------------------------------------------------------------

func TestBuildHealthGateCmdHTTP(t *testing.T) {
	deps := []healthDep{
		{name: "M5", host: "m5-management.kaizen.local", port: 50055, proto: "http", path: "/healthz"},
	}
	cmd := buildHealthGateCmd(deps)

	if !strings.Contains(cmd, "wget") {
		t.Error("HTTP dep should use wget")
	}
	if !strings.Contains(cmd, "m5-management.kaizen.local") {
		t.Error("command should contain M5 host")
	}
	if !strings.Contains(cmd, "50055") {
		t.Error("command should contain M5 port")
	}
	if !strings.Contains(cmd, "/healthz") {
		t.Error("command should contain health path")
	}
	if !strings.Contains(cmd, "All dependencies healthy") {
		t.Error("command should end with success message")
	}
}

func TestBuildHealthGateCmdTCP(t *testing.T) {
	deps := []healthDep{
		{name: "M1", host: "m1-assignment.kaizen.local", port: 50051, proto: "tcp"},
	}
	cmd := buildHealthGateCmd(deps)

	if !strings.Contains(cmd, "nc") {
		t.Error("TCP dep should use nc")
	}
	if !strings.Contains(cmd, "m1-assignment.kaizen.local") {
		t.Error("command should contain M1 host")
	}
	if !strings.Contains(cmd, "50051") {
		t.Error("command should contain M1 port")
	}
}

func TestBuildHealthGateCmdMultipleDeps(t *testing.T) {
	cmd := buildHealthGateCmd(tier1Deps)

	// Should contain all 3 tier 1 deps.
	for _, dep := range tier1Deps {
		if !strings.Contains(cmd, dep.host) {
			t.Errorf("command missing dep host: %s", dep.host)
		}
	}
	if !strings.Contains(cmd, "set -e") {
		t.Error("command should start with set -e for fail-fast")
	}
}

func TestBuildHealthGateCmdEmpty(t *testing.T) {
	cmd := buildHealthGateCmd(nil)
	// Empty deps should just have the success message.
	if !strings.Contains(cmd, "All dependencies healthy") {
		t.Error("empty deps should still produce success message")
	}
}

// ---------------------------------------------------------------------------
// GroupByTier
// ---------------------------------------------------------------------------

func TestGroupSpecsByTier(t *testing.T) {
	specs := serviceSpecs()
	grouped := groupSpecsByTier(specs)

	if len(grouped) != 3 {
		t.Fatalf("expected 3 tiers, got %d", len(grouped))
	}

	// Verify total count across all tiers.
	total := 0
	for _, tierSpecs := range grouped {
		total += len(tierSpecs)
	}
	if total != 8 {
		t.Errorf("total services across tiers: got %d, want 8", total)
	}
}

func TestSortedTierKeys(t *testing.T) {
	m := map[int][]serviceSpec{
		2: {{key: "m3"}},
		0: {{key: "m5"}},
		1: {{key: "m1"}},
	}
	keys := sortedTierKeys(m)
	if len(keys) != 3 {
		t.Fatalf("expected 3 keys, got %d", len(keys))
	}
	if keys[0] != 0 || keys[1] != 1 || keys[2] != 2 {
		t.Errorf("keys not sorted: %v", keys)
	}
}

// ---------------------------------------------------------------------------
// Service spec consistency
// ---------------------------------------------------------------------------

func TestAllSpecsHaveRequiredFields(t *testing.T) {
	specs := serviceSpecs()
	for _, s := range specs {
		if s.key == "" {
			t.Error("spec has empty key")
		}
		if s.name == "" {
			t.Errorf("spec %q has empty name", s.key)
		}
		if s.ecrKey == "" {
			t.Errorf("spec %q has empty ecrKey", s.key)
		}
		if s.cpu == "" {
			t.Errorf("spec %q has empty cpu", s.key)
		}
		if s.memoryMB == "" {
			t.Errorf("spec %q has empty memoryMB", s.key)
		}
		if len(s.ports) == 0 {
			t.Errorf("spec %q has no ports", s.key)
		}
		if s.lang == "" {
			t.Errorf("spec %q has empty lang", s.key)
		}
		if len(s.healthCmd) == 0 {
			t.Errorf("spec %q has no healthCmd", s.key)
		}
	}
}

func TestM5IsFirstInSpecOrder(t *testing.T) {
	specs := serviceSpecs()
	if len(specs) == 0 {
		t.Fatal("no specs")
	}
	if specs[0].key != "m5" {
		t.Errorf("first spec should be M5 (tier 0), got %q", specs[0].key)
	}
}
