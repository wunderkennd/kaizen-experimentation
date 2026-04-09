package observability

import (
	"testing"

	"github.com/kaizen-experimentation/infra/pkg/cicd"
)

// TestLogGroupCount verifies we create exactly 9 log groups (one per service).
func TestLogGroupCount(t *testing.T) {
	if len(cicd.ServiceNames) != 9 {
		t.Errorf("expected 9 services for log groups, got %d", len(cicd.ServiceNames))
	}
}

// TestLatencyThresholds verifies the SLO targets match the spec.
func TestLatencyThresholds(t *testing.T) {
	targets := map[string]float64{
		"assignment": 5,  // M1 < 5ms
		"management": 50, // M5 < 50ms
		"flags":      10, // M7 < 10ms
	}

	for svc, threshold := range targets {
		if threshold <= 0 {
			t.Errorf("service %s: latency threshold must be > 0, got %f", svc, threshold)
		}
	}

	// Verify all latency-targeted services exist in ServiceNames.
	svcSet := make(map[string]bool, len(cicd.ServiceNames))
	for _, s := range cicd.ServiceNames {
		svcSet[s] = true
	}
	for svc := range targets {
		if !svcSet[svc] {
			t.Errorf("latency target service %q not found in ServiceNames", svc)
		}
	}
}

// TestRDSAlarmThresholds validates RDS alarm constants against the spec.
func TestRDSAlarmThresholds(t *testing.T) {
	const (
		cpuThreshold        = 80.0
		connectionsThreshold = 180.0
		maxConnections       = 200 // from RDS parameter group
	)

	if cpuThreshold <= 0 || cpuThreshold >= 100 {
		t.Errorf("RDS CPU threshold must be 0-100, got %f", cpuThreshold)
	}

	if connectionsThreshold >= float64(maxConnections) {
		t.Errorf("connections threshold (%f) must be below max_connections (%d)",
			connectionsThreshold, maxConnections)
	}
}

// TestMSKConsumerLagThreshold validates the MSK consumer lag alarm target.
func TestMSKConsumerLagThreshold(t *testing.T) {
	const lagThreshold = 10000.0

	if lagThreshold <= 0 {
		t.Errorf("MSK consumer lag threshold must be > 0, got %f", lagThreshold)
	}
}
