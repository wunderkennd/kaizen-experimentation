package cicd

import (
	"encoding/json"
	"testing"
)

func TestBuildLifecyclePolicy(t *testing.T) {
	policyJSON, err := buildLifecyclePolicy()
	if err != nil {
		t.Fatalf("buildLifecyclePolicy() error: %v", err)
	}

	var policy lifecyclePolicy
	if err := json.Unmarshal([]byte(policyJSON), &policy); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}

	if len(policy.Rules) != 2 {
		t.Fatalf("expected 2 rules, got %d", len(policy.Rules))
	}

	// Rule 1: untagged images expire after 7 days
	r1 := policy.Rules[0]
	if r1.RulePriority != 1 {
		t.Errorf("rule 1 priority: got %d, want 1", r1.RulePriority)
	}
	if r1.Selection.TagStatus != "untagged" {
		t.Errorf("rule 1 tagStatus: got %q, want %q", r1.Selection.TagStatus, "untagged")
	}
	if r1.Selection.CountType != "sinceImagePushed" {
		t.Errorf("rule 1 countType: got %q, want %q", r1.Selection.CountType, "sinceImagePushed")
	}
	if r1.Selection.CountUnit != "days" {
		t.Errorf("rule 1 countUnit: got %q, want %q", r1.Selection.CountUnit, "days")
	}
	if r1.Selection.CountNumber != 7 {
		t.Errorf("rule 1 countNumber: got %d, want 7", r1.Selection.CountNumber)
	}

	// Rule 2: keep last 10 tagged images
	r2 := policy.Rules[1]
	if r2.RulePriority != 2 {
		t.Errorf("rule 2 priority: got %d, want 2", r2.RulePriority)
	}
	if r2.Selection.TagStatus != "tagged" {
		t.Errorf("rule 2 tagStatus: got %q, want %q", r2.Selection.TagStatus, "tagged")
	}
	if r2.Selection.CountType != "imageCountMoreThan" {
		t.Errorf("rule 2 countType: got %q, want %q", r2.Selection.CountType, "imageCountMoreThan")
	}
	if r2.Selection.CountNumber != 10 {
		t.Errorf("rule 2 countNumber: got %d, want 10", r2.Selection.CountNumber)
	}
	if len(r2.Selection.TagPrefixList) == 0 {
		t.Error("rule 2 tagPrefixList must not be empty for tagged status")
	}
}

func TestServiceNamesCount(t *testing.T) {
	if len(ServiceNames) != 9 {
		t.Errorf("expected 9 services, got %d", len(ServiceNames))
	}

	expected := map[string]bool{
		"assignment":    true,
		"pipeline":      true,
		"orchestration": true,
		"metrics":       true,
		"analysis":      true,
		"policy":        true,
		"management":    true,
		"ui":            true,
		"flags":         true,
	}

	for _, svc := range ServiceNames {
		if !expected[svc] {
			t.Errorf("unexpected service name: %q", svc)
		}
		delete(expected, svc)
	}

	for svc := range expected {
		t.Errorf("missing service: %q", svc)
	}
}
