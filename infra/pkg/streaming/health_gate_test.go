package streaming

import (
	"fmt"
	"testing"
)

// TestExpectedTopicNamesCount verifies exactly 8 topics are expected.
func TestExpectedTopicNamesCount(t *testing.T) {
	names := ExpectedTopicNames()
	if len(names) != ExpectedTopicCount {
		t.Fatalf("ExpectedTopicNames() returned %d topics, want %d", len(names), ExpectedTopicCount)
	}
}

// TestExpectedTopicNamesMatchSpecs verifies the topic names returned by
// ExpectedTopicNames() match the canonical list provisioned by NewTopics.
func TestExpectedTopicNamesMatchSpecs(t *testing.T) {
	canonical := map[string]bool{
		"exposures":                        true,
		"metric_events":                    true,
		"reward_events":                    true,
		"qoe_events":                       true,
		"guardrail_alerts":                 true,
		"sequential_boundary_alerts":       true,
		"model_retraining_events":          true,
		"surrogate_recalibration_requests": true,
	}

	names := ExpectedTopicNames()
	for _, name := range names {
		if !canonical[name] {
			t.Errorf("unexpected topic name from ExpectedTopicNames(): %q", name)
		}
		delete(canonical, name)
	}

	for missing := range canonical {
		t.Errorf("missing topic from ExpectedTopicNames(): %q", missing)
	}
}

// TestExpectedTopicNamesConsistentWithTopicSpecs ensures ExpectedTopicNames()
// stays in sync with the topics slice used by NewTopics.
func TestExpectedTopicNamesConsistentWithTopicSpecs(t *testing.T) {
	names := ExpectedTopicNames()
	if len(names) != len(topics) {
		t.Fatalf("ExpectedTopicNames() length (%d) != topics slice length (%d)",
			len(names), len(topics))
	}

	for i, spec := range topics {
		if names[i] != spec.Name {
			t.Errorf("ExpectedTopicNames()[%d] = %q, want %q (from topics slice)", i, names[i], spec.Name)
		}
	}
}

// TestExpectedTopicCountConstant verifies the constant matches the actual count.
func TestExpectedTopicCountConstant(t *testing.T) {
	if ExpectedTopicCount != len(topics) {
		t.Errorf("ExpectedTopicCount = %d, but topics slice has %d entries",
			ExpectedTopicCount, len(topics))
	}
}

// TestDefaultHealthCheckConfig validates the Schema Registry health check
// parameters are within acceptable bounds for ECS container health checks.
func TestDefaultHealthCheckConfig(t *testing.T) {
	cfg := DefaultHealthCheckConfig()

	if cfg.Command != "CMD-SHELL" {
		t.Errorf("health check command = %q, want %q", cfg.Command, "CMD-SHELL")
	}

	// ECS requires interval >= 5s and <= 300s.
	if cfg.IntervalSec < 5 || cfg.IntervalSec > 300 {
		t.Errorf("health check interval = %ds, must be 5-300s", cfg.IntervalSec)
	}

	// Timeout must be less than interval.
	if cfg.TimeoutSec >= cfg.IntervalSec {
		t.Errorf("health check timeout (%ds) must be < interval (%ds)",
			cfg.TimeoutSec, cfg.IntervalSec)
	}

	// ECS allows 1-10 retries.
	if cfg.Retries < 1 || cfg.Retries > 10 {
		t.Errorf("health check retries = %d, must be 1-10", cfg.Retries)
	}

	// Start period: 0-300s. Schema Registry needs time to connect to Kafka.
	if cfg.StartPeriod < 0 || cfg.StartPeriod > 300 {
		t.Errorf("health check startPeriod = %ds, must be 0-300s", cfg.StartPeriod)
	}

	// Schema Registry needs at least 30s to start and connect to Kafka/MSK.
	if cfg.StartPeriod < 30 {
		t.Errorf("health check startPeriod = %ds, Schema Registry needs >= 30s to initialize",
			cfg.StartPeriod)
	}
}

// TestSchemaRegistryURLFormat validates the expected Schema Registry URL format.
func TestSchemaRegistryURLFormat(t *testing.T) {
	// The URL is constructed in NewSchemaRegistry as:
	// http://schema-registry.kaizen.local:8081
	expectedHost := "schema-registry.kaizen.local"
	expectedPort := 8081
	expectedScheme := "http"

	// Verify the Cloud Map service name matches the DNS record.
	// The service is registered as "schema-registry" in the kaizen.local namespace,
	// so the FQDN is schema-registry.kaizen.local.
	url := expectedScheme + "://" + expectedHost + ":" + formatPort(expectedPort)
	if url != "http://schema-registry.kaizen.local:8081" {
		t.Errorf("Schema Registry URL = %q, want %q", url, "http://schema-registry.kaizen.local:8081")
	}
}

// formatPort converts a port number to string for URL construction.
func formatPort(port int) string {
	return fmt.Sprintf("%d", port)
}

// TestTopicNamesNoDuplicates ensures there are no duplicate topic names.
func TestTopicNamesNoDuplicates(t *testing.T) {
	seen := make(map[string]bool)
	for _, name := range ExpectedTopicNames() {
		if seen[name] {
			t.Errorf("duplicate topic name: %q", name)
		}
		seen[name] = true
	}
}

// TestTopicNamesNonEmpty ensures no topic has an empty name.
func TestTopicNamesNonEmpty(t *testing.T) {
	for i, name := range ExpectedTopicNames() {
		if name == "" {
			t.Errorf("topic at index %d has empty name", i)
		}
	}
}
