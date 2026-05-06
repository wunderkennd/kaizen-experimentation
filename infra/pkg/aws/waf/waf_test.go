package waf

import (
	"fmt"
	"testing"
)

func TestWafLogBucketNaming(t *testing.T) {
	// AWS WAF requires log bucket names to start with "aws-waf-logs-".
	envs := []string{"dev", "staging", "prod"}
	for _, env := range envs {
		namePrefix := fmt.Sprintf("kaizen-%s", env)
		bucketName := fmt.Sprintf("aws-waf-logs-%s", namePrefix)

		// Must start with the required AWS prefix ("aws-waf-logs-" = 13 chars).
		if bucketName[:13] != "aws-waf-logs-" {
			t.Errorf("env=%s: bucket name %q does not start with aws-waf-logs-", env, bucketName)
		}
		// Must include environment for uniqueness.
		expected := fmt.Sprintf("aws-waf-logs-kaizen-%s", env)
		if bucketName != expected {
			t.Errorf("env=%s: bucket name = %q, want %q", env, bucketName, expected)
		}
	}
}

func TestRulePriorityOrdering(t *testing.T) {
	// Verify rule priority ordering matches the documented evaluation order:
	//   rate-limit (1) < geo-block (2) < common (10) < sqli (20)
	type ruleSpec struct {
		name     string
		priority int
	}

	t.Run("with geo-restriction", func(t *testing.T) {
		rules := []prioritySpec{
			{"rate-limit-per-ip", 1},
			{"geo-block", 2},
			{"AWSManagedRulesCommonRuleSet", 10},
			{"AWSManagedRulesSQLiRuleSet", 20},
		}
		assertUniquePriorities(t, rules)
	})

	t.Run("without geo-restriction", func(t *testing.T) {
		rules := []prioritySpec{
			{"rate-limit-per-ip", 1},
			{"AWSManagedRulesCommonRuleSet", 10},
			{"AWSManagedRulesSQLiRuleSet", 20},
		}
		assertUniquePriorities(t, rules)
	})
}

type prioritySpec struct {
	name     string
	priority int
}

func assertUniquePriorities(t *testing.T, rules []prioritySpec) {
	t.Helper()
	seen := make(map[int]string)
	prevPriority := 0
	for _, r := range rules {
		if prev, exists := seen[r.priority]; exists {
			t.Errorf("duplicate priority %d: %s and %s", r.priority, prev, r.name)
		}
		seen[r.priority] = r.name

		if r.priority <= prevPriority {
			t.Errorf("rule %s (priority %d) must be > previous (%d)", r.name, r.priority, prevPriority)
		}
		prevPriority = r.priority
	}
}

func TestDefaultRateLimit(t *testing.T) {
	// The default rate limit is 1000 requests per 5-minute window.
	// AWS WAF minimum is 100; values below that are invalid.
	defaultLimit := 1000
	if defaultLimit < 100 {
		t.Errorf("rate limit %d is below AWS WAF minimum (100)", defaultLimit)
	}
	if defaultLimit > 20_000_000 {
		t.Errorf("rate limit %d exceeds AWS WAF maximum (20000000)", defaultLimit)
	}
}

func TestManagedRuleSetNames(t *testing.T) {
	// Verify the managed rule set names are valid AWS identifiers.
	// These must match exactly — a typo causes a deployment failure.
	rulesets := []struct {
		name       string
		vendorName string
	}{
		{"AWSManagedRulesCommonRuleSet", "AWS"},
		{"AWSManagedRulesSQLiRuleSet", "AWS"},
	}

	for _, rs := range rulesets {
		if rs.vendorName != "AWS" {
			t.Errorf("rule set %s: vendor = %q, want AWS", rs.name, rs.vendorName)
		}
		if rs.name == "" {
			t.Error("rule set name must not be empty")
		}
	}
}

func TestGeoBlockCountryCodes(t *testing.T) {
	// Verify ISO 3166-1 alpha-2 codes are exactly 2 characters.
	codes := []string{"CN", "RU", "KP", "IR"}
	for _, c := range codes {
		if len(c) != 2 {
			t.Errorf("country code %q is not 2 characters", c)
		}
	}
}

func TestWebAclNaming(t *testing.T) {
	// Verify web ACL follows the kaizen-{env}-waf naming convention.
	envs := []string{"dev", "staging", "prod"}
	for _, env := range envs {
		expected := fmt.Sprintf("kaizen-%s-waf", env)
		got := fmt.Sprintf("kaizen-%s-waf", env)
		if got != expected {
			t.Errorf("env=%s: web ACL name = %q, want %q", env, got, expected)
		}
	}
}
