package compute

import (
	"testing"
)

// ---------------------------------------------------------------------------
// DefaultAutoscalingArgs — dev environment
// ---------------------------------------------------------------------------

func TestDefaultAutoscalingArgsDev(t *testing.T) {
	args := DefaultAutoscalingArgs("dev")

	if args.Environment != "dev" {
		t.Fatalf("Environment = %q, want %q", args.Environment, "dev")
	}

	// Dev should have reduced M1 capacity.
	if args.M1Assignment.MinCapacity != 1 {
		t.Errorf("dev M1Assignment.MinCapacity = %d, want 1", args.M1Assignment.MinCapacity)
	}
	if args.M1Assignment.MaxCapacity != 5 {
		t.Errorf("dev M1Assignment.MaxCapacity = %d, want 5", args.M1Assignment.MaxCapacity)
	}

	// Dev should have reduced M2 Pipeline capacity.
	if args.M2Pipeline.MinCapacity != 1 {
		t.Errorf("dev M2Pipeline.MinCapacity = %d, want 1", args.M2Pipeline.MinCapacity)
	}
	if args.M2Pipeline.MaxCapacity != 3 {
		t.Errorf("dev M2Pipeline.MaxCapacity = %d, want 3", args.M2Pipeline.MaxCapacity)
	}

	// Dev should have reduced M7 Flags capacity.
	if args.M7Flags.MinCapacity != 1 {
		t.Errorf("dev M7Flags.MinCapacity = %d, want 1", args.M7Flags.MinCapacity)
	}
	if args.M7Flags.MaxCapacity != 3 {
		t.Errorf("dev M7Flags.MaxCapacity = %d, want 3", args.M7Flags.MaxCapacity)
	}

	// Dev min should always be less than or equal to max for all services.
	services := []struct {
		name string
		cfg  ServiceScalingConfig
	}{
		{"M1Assignment", args.M1Assignment},
		{"M2Pipeline", args.M2Pipeline},
		{"M2Orch", args.M2Orch},
		{"M3Metrics", args.M3Metrics},
		{"M4aAnalysis", args.M4aAnalysis},
		{"M5Management", args.M5Management},
		{"M6UI", args.M6UI},
		{"M7Flags", args.M7Flags},
	}

	for _, svc := range services {
		if svc.cfg.MinCapacity > svc.cfg.MaxCapacity {
			t.Errorf("dev %s: MinCapacity (%d) > MaxCapacity (%d)",
				svc.name, svc.cfg.MinCapacity, svc.cfg.MaxCapacity)
		}
	}
}

// ---------------------------------------------------------------------------
// DefaultAutoscalingArgs — prod environment
// ---------------------------------------------------------------------------

func TestDefaultAutoscalingArgsProd(t *testing.T) {
	args := DefaultAutoscalingArgs("prod")

	if args.Environment != "prod" {
		t.Fatalf("Environment = %q, want %q", args.Environment, "prod")
	}

	// Prod M1 should have higher capacity than dev.
	devArgs := DefaultAutoscalingArgs("dev")

	if args.M1Assignment.MinCapacity <= devArgs.M1Assignment.MinCapacity {
		t.Errorf("prod M1Assignment.MinCapacity (%d) should exceed dev (%d)",
			args.M1Assignment.MinCapacity, devArgs.M1Assignment.MinCapacity)
	}
	if args.M1Assignment.MaxCapacity <= devArgs.M1Assignment.MaxCapacity {
		t.Errorf("prod M1Assignment.MaxCapacity (%d) should exceed dev (%d)",
			args.M1Assignment.MaxCapacity, devArgs.M1Assignment.MaxCapacity)
	}

	// Prod M7 should have higher capacity than dev.
	if args.M7Flags.MinCapacity <= devArgs.M7Flags.MinCapacity {
		t.Errorf("prod M7Flags.MinCapacity (%d) should exceed dev (%d)",
			args.M7Flags.MinCapacity, devArgs.M7Flags.MinCapacity)
	}
	if args.M7Flags.MaxCapacity <= devArgs.M7Flags.MaxCapacity {
		t.Errorf("prod M7Flags.MaxCapacity (%d) should exceed dev (%d)",
			args.M7Flags.MaxCapacity, devArgs.M7Flags.MaxCapacity)
	}

	// Prod min should always be less than or equal to max.
	services := []struct {
		name string
		cfg  ServiceScalingConfig
	}{
		{"M1Assignment", args.M1Assignment},
		{"M2Pipeline", args.M2Pipeline},
		{"M2Orch", args.M2Orch},
		{"M3Metrics", args.M3Metrics},
		{"M4aAnalysis", args.M4aAnalysis},
		{"M5Management", args.M5Management},
		{"M6UI", args.M6UI},
		{"M7Flags", args.M7Flags},
	}

	for _, svc := range services {
		if svc.cfg.MinCapacity > svc.cfg.MaxCapacity {
			t.Errorf("prod %s: MinCapacity (%d) > MaxCapacity (%d)",
				svc.name, svc.cfg.MinCapacity, svc.cfg.MaxCapacity)
		}
	}
}

// ---------------------------------------------------------------------------
// CPU target tracking percentage
// ---------------------------------------------------------------------------

func TestAutoscalingArgsCPUTarget(t *testing.T) {
	// The CPU target is hardcoded at 70% in newCPUScalingPolicy.
	// Verify through DefaultAutoscalingArgs that all services have
	// reasonable scaling ranges, which is the contract tested at the
	// unit level. The 70% target itself is validated by integration tests
	// against the actual Pulumi resource.
	//
	// Here we verify that the CPU-tracked services (M2, M2-Orch, M3,
	// M4a, M5, M6) all have min >= 1 and max >= 2, which ensures
	// autoscaling has room to operate.

	envs := []string{"dev", "staging", "prod"}

	for _, env := range envs {
		t.Run(env, func(t *testing.T) {
			args := DefaultAutoscalingArgs(env)

			cpuServices := []struct {
				name string
				cfg  ServiceScalingConfig
			}{
				{"M2Pipeline", args.M2Pipeline},
				{"M2Orch", args.M2Orch},
				{"M3Metrics", args.M3Metrics},
				{"M4aAnalysis", args.M4aAnalysis},
				{"M5Management", args.M5Management},
				{"M6UI", args.M6UI},
			}

			for _, svc := range cpuServices {
				if svc.cfg.MinCapacity < 1 {
					t.Errorf("%s %s: MinCapacity (%d) < 1",
						env, svc.name, svc.cfg.MinCapacity)
				}
				if svc.cfg.MaxCapacity < 2 {
					t.Errorf("%s %s: MaxCapacity (%d) < 2 (autoscaling needs headroom)",
						env, svc.name, svc.cfg.MaxCapacity)
				}
			}
		})
	}
}
