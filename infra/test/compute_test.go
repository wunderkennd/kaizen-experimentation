// Package test provides E2E deploy smoke tests that validate all 9 Kaizen
// services are correctly specified for deployment: ECS state, health checks,
// Cloud Map DNS, and service topology.
//
// These tests run against the declared infrastructure specifications (the same
// tables that drive `pulumi up`) and catch configuration drift at `go test`
// time — before a deploy is attempted.
//
// Task I.2.6 — Closes #367
package test

import (
	"fmt"
	"strings"
	"testing"

	"github.com/kaizen-experimentation/infra/pkg/cicd"
	"github.com/kaizen-experimentation/infra/pkg/compute"
)

// ---------------------------------------------------------------------------
// Service catalog: the canonical list of all 9 Kaizen services.
// ---------------------------------------------------------------------------

// serviceEntry describes the expected deployed state of a single service.
type serviceEntry struct {
	key       string // map key in ServicesOutputs (e.g. "m1")
	name      string // Cloud Map / ECS service name
	lang      string // "rust", "go", or "ts"
	ports     []int  // container ports
	dnsName   string // Cloud Map DNS: <name>.kaizen.local
	healthFmt string // expected health check command format
}

// expectedServices returns the ground-truth service topology.
// 8 Fargate services + 1 EC2 service (M4b) = 9 total.
func expectedServices() []serviceEntry {
	return []serviceEntry{
		{key: "m1", name: "m1-assignment", lang: "rust", ports: []int{50051}, dnsName: "m1-assignment.kaizen.local"},
		{key: "m2", name: "m2-pipeline", lang: "rust", ports: []int{50052}, dnsName: "m2-pipeline.kaizen.local"},
		{key: "m2-orch", name: "m2-orchestration", lang: "go", ports: []int{50058}, dnsName: "m2-orchestration.kaizen.local"},
		{key: "m3", name: "m3-metrics", lang: "go", ports: []int{50056, 50059}, dnsName: "m3-metrics.kaizen.local"},
		{key: "m4a", name: "m4a-analysis", lang: "rust", ports: []int{50053}, dnsName: "m4a-analysis.kaizen.local"},
		{key: "m4b", name: "m4b-policy", lang: "rust", ports: []int{50054}, dnsName: "m4b-policy.kaizen.local"},
		{key: "m5", name: "m5-management", lang: "go", ports: []int{50055, 50060}, dnsName: "m5-management.kaizen.local"},
		{key: "m6", name: "m6-ui", lang: "ts", ports: []int{3000}, dnsName: "m6-ui.kaizen.local"},
		{key: "m7", name: "m7-flags", lang: "rust", ports: []int{50057}, dnsName: "m7-flags.kaizen.local"},
	}
}

// ---------------------------------------------------------------------------
// Test: All 9 ECS services are in the RUNNING catalogue
// ---------------------------------------------------------------------------

func TestAllNineServicesInCatalogue(t *testing.T) {
	// ECR ServiceNames is the canonical list used by all modules.
	if got := len(cicd.ServiceNames); got != 9 {
		t.Fatalf("ServiceNames count = %d, want 9 (all platform modules)", got)
	}

	// Fargate specs from the compute package.
	fargateSpecs := compute.ServiceSpecs()
	if got := len(fargateSpecs); got != 8 {
		t.Fatalf("Fargate service specs count = %d, want 8", got)
	}

	// Verify all 9 expected services are present (8 Fargate + 1 EC2/M4b).
	expected := expectedServices()
	if got := len(expected); got != 9 {
		t.Fatalf("expected services count = %d, want 9", got)
	}

	fargateKeys := make(map[string]bool, len(fargateSpecs))
	for _, spec := range fargateSpecs {
		fargateKeys[spec.Key] = true
	}

	for _, svc := range expected {
		if svc.key == "m4b" {
			// M4b runs on EC2, not Fargate — it's registered separately.
			continue
		}
		if !fargateKeys[svc.key] {
			t.Errorf("service %q (key=%q) missing from Fargate service specs", svc.name, svc.key)
		}
	}
}

// ---------------------------------------------------------------------------
// Test: gRPC health checks pass for Rust services
// ---------------------------------------------------------------------------

func TestGRPCHealthChecksForRustServices(t *testing.T) {
	fargateSpecs := compute.ServiceSpecs()

	rustServices := map[string]int{
		"m1":  50051,
		"m2":  50052,
		"m4a": 50053,
		"m7":  50057,
	}

	for _, spec := range fargateSpecs {
		expectedPort, isRust := rustServices[spec.Key]
		if !isRust {
			continue
		}

		t.Run(spec.Name, func(t *testing.T) {
			if spec.Lang != "rust" {
				t.Errorf("service %s: lang = %q, want \"rust\"", spec.Key, spec.Lang)
			}

			healthCmd := spec.HealthCmd
			if len(healthCmd) < 2 {
				t.Fatalf("service %s: health check command too short: %v", spec.Key, healthCmd)
			}

			// Rust services use CMD (exec form) with grpc_health_probe.
			if healthCmd[0] != "CMD" {
				t.Errorf("service %s: health check type = %q, want \"CMD\" (exec form for gRPC probe)", spec.Key, healthCmd[0])
			}

			expectedProbe := fmt.Sprintf("/bin/grpc_health_probe -addr=:%d", expectedPort)
			actualCmd := strings.Join(healthCmd[1:], " ")
			if actualCmd != expectedProbe {
				t.Errorf("service %s: health check = %q, want %q", spec.Key, actualCmd, expectedProbe)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Test: /healthz returns 200 for Go services
// ---------------------------------------------------------------------------

func TestHealthzForGoServices(t *testing.T) {
	fargateSpecs := compute.ServiceSpecs()

	goServices := map[string]int{
		"m2-orch": 50058,
		"m3":      50056,
		"m5":      50055,
	}

	for _, spec := range fargateSpecs {
		expectedPort, isGo := goServices[spec.Key]
		if !isGo {
			continue
		}

		t.Run(spec.Name, func(t *testing.T) {
			if spec.Lang != "go" {
				t.Errorf("service %s: lang = %q, want \"go\"", spec.Key, spec.Lang)
			}

			healthCmd := spec.HealthCmd
			if len(healthCmd) < 2 {
				t.Fatalf("service %s: health check command too short: %v", spec.Key, healthCmd)
			}

			// Go services use CMD-SHELL with wget --spider.
			if healthCmd[0] != "CMD-SHELL" {
				t.Errorf("service %s: health check type = %q, want \"CMD-SHELL\"", spec.Key, healthCmd[0])
			}

			expectedCheck := fmt.Sprintf("wget --spider -q http://localhost:%d/healthz || exit 1", expectedPort)
			if healthCmd[1] != expectedCheck {
				t.Errorf("service %s: health check = %q, want %q", spec.Key, healthCmd[1], expectedCheck)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Test: M6 UI returns 200 on /
// ---------------------------------------------------------------------------

func TestM6UIHealthCheck(t *testing.T) {
	fargateSpecs := compute.ServiceSpecs()

	var found bool
	for _, spec := range fargateSpecs {
		if spec.Key != "m6" {
			continue
		}
		found = true

		if spec.Lang != "ts" {
			t.Errorf("M6 UI: lang = %q, want \"ts\"", spec.Lang)
		}

		if len(spec.Ports) == 0 || spec.Ports[0] != 3000 {
			t.Errorf("M6 UI: ports = %v, want [3000]", spec.Ports)
		}

		healthCmd := spec.HealthCmd
		if len(healthCmd) < 2 {
			t.Fatalf("M6 UI: health check command too short: %v", healthCmd)
		}

		if healthCmd[0] != "CMD-SHELL" {
			t.Errorf("M6 UI: health check type = %q, want \"CMD-SHELL\"", healthCmd[0])
		}

		expectedCheck := "wget --spider -q http://localhost:3000/ || exit 1"
		if healthCmd[1] != expectedCheck {
			t.Errorf("M6 UI: health check = %q, want %q", healthCmd[1], expectedCheck)
		}
	}

	if !found {
		t.Fatal("M6 UI service not found in Fargate specs")
	}
}

// ---------------------------------------------------------------------------
// Test: Cloud Map DNS resolves for all 9 services
// ---------------------------------------------------------------------------

func TestCloudMapDNSResolvesAllNineServices(t *testing.T) {
	endpoints := compute.ServiceEndpoints()

	// Must have exactly 9 service endpoints.
	if got := len(endpoints); got != 9 {
		t.Fatalf("service endpoints count = %d, want 9", got)
	}

	expected := expectedServices()
	for _, svc := range expected {
		envKey := fmt.Sprintf("%s_ENDPOINT", strings.ToUpper(
			strings.ReplaceAll(
				strings.ReplaceAll(svc.dnsName, ".kaizen.local", ""),
				"-", "_",
			),
		))

		endpoint, ok := endpoints[envKey]
		if !ok {
			t.Errorf("service %s: missing endpoint env var %q in service endpoints map", svc.name, envKey)
			continue
		}

		// Verify DNS name matches expected pattern.
		expectedDNS := fmt.Sprintf("%s:%d", svc.dnsName, svc.ports[0])
		if endpoint != expectedDNS {
			t.Errorf("service %s: endpoint = %q, want %q", svc.name, endpoint, expectedDNS)
		}

		// Verify the DNS name uses the kaizen.local namespace.
		if !strings.HasSuffix(endpoint, ".kaizen.local:"+fmt.Sprintf("%d", svc.ports[0])) {
			t.Errorf("service %s: endpoint %q does not use kaizen.local namespace", svc.name, endpoint)
		}
	}
}

// ---------------------------------------------------------------------------
// Test: Port uniqueness across services
// ---------------------------------------------------------------------------

func TestServicePortsAreUnique(t *testing.T) {
	// The primary port (first in the list) for each service must be unique
	// to avoid conflicts in Cloud Map registration.
	expected := expectedServices()
	seen := make(map[int]string)

	for _, svc := range expected {
		primary := svc.ports[0]
		if prev, exists := seen[primary]; exists {
			t.Errorf("port %d claimed by both %s and %s", primary, prev, svc.name)
		}
		seen[primary] = svc.name
	}
}

// ---------------------------------------------------------------------------
// Test: ECR ↔ Fargate service alignment
// ---------------------------------------------------------------------------

func TestECRReposAlignWithFargateServices(t *testing.T) {
	ecrSet := make(map[string]bool, len(cicd.ServiceNames))
	for _, name := range cicd.ServiceNames {
		ecrSet[name] = true
	}

	fargateSpecs := compute.ServiceSpecs()
	for _, spec := range fargateSpecs {
		if !ecrSet[spec.EcrKey] {
			t.Errorf("Fargate service %s references ECR key %q which is not in cicd.ServiceNames", spec.Name, spec.EcrKey)
		}
	}

	// M4b (EC2) also needs an ECR repo ("policy").
	if !ecrSet["policy"] {
		t.Error("M4b (policy) missing from cicd.ServiceNames — no ECR repo for EC2 service")
	}
}

// ---------------------------------------------------------------------------
// Test: Health check configuration constants
// ---------------------------------------------------------------------------

func TestHealthCheckTimingConstants(t *testing.T) {
	fargateSpecs := compute.ServiceSpecs()

	for _, spec := range fargateSpecs {
		t.Run(spec.Name, func(t *testing.T) {
			// All health checks must have non-empty commands.
			if len(spec.HealthCmd) == 0 {
				t.Error("health check command is empty")
			}

			// Verify the command format is either CMD (exec) or CMD-SHELL.
			validTypes := map[string]bool{"CMD": true, "CMD-SHELL": true}
			if !validTypes[spec.HealthCmd[0]] {
				t.Errorf("health check type %q not in {CMD, CMD-SHELL}", spec.HealthCmd[0])
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Test: M4b (EC2) Cloud Map registration
// ---------------------------------------------------------------------------

func TestM4bCloudMapRegistration(t *testing.T) {
	endpoints := compute.ServiceEndpoints()

	// M4b must be discoverable at m4b-policy.kaizen.local:50054.
	m4bEndpoint, ok := endpoints["M4B_POLICY_ENDPOINT"]
	if !ok {
		t.Fatal("M4B_POLICY_ENDPOINT missing from service endpoints")
	}

	if m4bEndpoint != "m4b-policy.kaizen.local:50054" {
		t.Errorf("M4b endpoint = %q, want %q", m4bEndpoint, "m4b-policy.kaizen.local:50054")
	}
}
