package streaming

import (
	"testing"
)

func TestTopicSpecsCount(t *testing.T) {
	if len(topics) != 8 {
		t.Fatalf("expected 8 topics, got %d", len(topics))
	}
}

func TestTopicNames(t *testing.T) {
	expected := map[string]bool{
		"exposures":                         true,
		"metric_events":                     true,
		"reward_events":                     true,
		"qoe_events":                        true,
		"guardrail_alerts":                  true,
		"sequential_boundary_alerts":        true,
		"model_retraining_events":           true,
		"surrogate_recalibration_requests":  true,
	}

	for _, spec := range topics {
		if !expected[spec.Name] {
			t.Errorf("unexpected topic: %q", spec.Name)
		}
		delete(expected, spec.Name)
	}

	for name := range expected {
		t.Errorf("missing topic: %q", name)
	}
}

func TestTopicPartitions(t *testing.T) {
	expectedPartitions := map[string]int{
		"exposures":                         64,
		"metric_events":                     128,
		"reward_events":                     32,
		"qoe_events":                        64,
		"guardrail_alerts":                  8,
		"sequential_boundary_alerts":        8,
		"model_retraining_events":           8,
		"surrogate_recalibration_requests":  4,
	}

	for _, spec := range topics {
		want, ok := expectedPartitions[spec.Name]
		if !ok {
			continue
		}
		if spec.Partitions != want {
			t.Errorf("topic %q: partitions = %d, want %d", spec.Name, spec.Partitions, want)
		}
	}
}

func TestTopicRetention(t *testing.T) {
	for _, spec := range topics {
		if spec.RetentionMs <= 0 {
			t.Errorf("topic %q: retention must be positive, got %d", spec.Name, spec.RetentionMs)
		}
	}

	// High-volume topics: 90d retention
	for _, name := range []string{"exposures", "metric_events", "qoe_events"} {
		for _, spec := range topics {
			if spec.Name == name && spec.RetentionMs != retention90d {
				t.Errorf("topic %q: retention = %d, want %d (90d)", name, spec.RetentionMs, retention90d)
			}
		}
	}

	// Long-retention topics: 180d
	for _, name := range []string{"reward_events", "model_retraining_events"} {
		for _, spec := range topics {
			if spec.Name == name && spec.RetentionMs != retention180d {
				t.Errorf("topic %q: retention = %d, want %d (180d)", name, spec.RetentionMs, retention180d)
			}
		}
	}

	// Short-retention alert topics: 30d
	for _, name := range []string{"guardrail_alerts", "sequential_boundary_alerts", "surrogate_recalibration_requests"} {
		for _, spec := range topics {
			if spec.Name == name && spec.RetentionMs != retention30d {
				t.Errorf("topic %q: retention = %d, want %d (30d)", name, spec.RetentionMs, retention30d)
			}
		}
	}
}

func TestReplicationAndCompressionConstants(t *testing.T) {
	if replicationFactor != 3 {
		t.Errorf("replicationFactor = %d, want 3", replicationFactor)
	}
	if minInsyncReplicas != 2 {
		t.Errorf("minInsyncReplicas = %d, want 2", minInsyncReplicas)
	}
	if cleanupPolicy != "delete" {
		t.Errorf("cleanupPolicy = %q, want %q", cleanupPolicy, "delete")
	}
	if compressionType != "lz4" {
		t.Errorf("compressionType = %q, want %q", compressionType, "lz4")
	}
}
