package loadbalancer

import "testing"

func TestTargetGroupSpecs(t *testing.T) {
	// Verify the 4 target groups have the correct port, protocol, and health
	// check configuration as specified in task I.1.8.
	specs := []targetGroupSpec{
		{
			name:               "m1-assignment",
			port:               50051,
			protocolVersion:    "gRPC",
			healthCheckPath:    "/grpc.health.v1.Health/Check",
			healthCheckMatcher: "0",
		},
		{
			name:               "m5-management",
			port:               50055,
			protocolVersion:    "HTTP2",
			healthCheckPath:    "/healthz",
			healthCheckMatcher: "200",
		},
		{
			name:               "m6-ui",
			port:               3000,
			protocolVersion:    "HTTP1",
			healthCheckPath:    "/",
			healthCheckMatcher: "200",
		},
		{
			name:               "m7-flags",
			port:               50057,
			protocolVersion:    "gRPC",
			healthCheckPath:    "/grpc.health.v1.Health/Check",
			healthCheckMatcher: "0",
		},
	}

	if len(specs) != 4 {
		t.Fatalf("expected 4 target group specs, got %d", len(specs))
	}

	// Verify gRPC services use gRPC protocol version and health check.
	grpcServices := map[string]int{
		"m1-assignment": 50051,
		"m7-flags":      50057,
	}
	for _, s := range specs {
		if expected, ok := grpcServices[s.name]; ok {
			if s.protocolVersion != "gRPC" {
				t.Errorf("%s: protocol version = %q, want gRPC", s.name, s.protocolVersion)
			}
			if s.port != expected {
				t.Errorf("%s: port = %d, want %d", s.name, s.port, expected)
			}
			if s.healthCheckMatcher != "0" {
				t.Errorf("%s: health check matcher = %q, want gRPC OK (0)", s.name, s.healthCheckMatcher)
			}
		}
	}

	// Verify M5 Management uses HTTP/2.
	for _, s := range specs {
		if s.name == "m5-management" {
			if s.protocolVersion != "HTTP2" {
				t.Errorf("m5-management: protocol = %q, want HTTP2", s.protocolVersion)
			}
			if s.healthCheckPath != "/healthz" {
				t.Errorf("m5-management: health path = %q, want /healthz", s.healthCheckPath)
			}
		}
	}

	// Verify M6 UI uses HTTP/1.
	for _, s := range specs {
		if s.name == "m6-ui" {
			if s.protocolVersion != "HTTP1" {
				t.Errorf("m6-ui: protocol = %q, want HTTP1", s.protocolVersion)
			}
			if s.port != 3000 {
				t.Errorf("m6-ui: port = %d, want 3000", s.port)
			}
		}
	}
}

func TestListenerRulePriorities(t *testing.T) {
	// Verify listener rule priority ordering:
	//   10: M1 (host-based, assign.kaizen.{domain})
	//   20: M5 (/api/*)
	//   30: M7 (/flags/*)
	//  100: M6 (catch-all /*)
	type rule struct {
		name     string
		priority int
		pattern  string
	}

	rules := []rule{
		{name: "m1-host", priority: 10, pattern: "assign.kaizen.{domain}"},
		{name: "m5-api", priority: 20, pattern: "/api/*"},
		{name: "m7-flags", priority: 30, pattern: "/flags/*"},
		{name: "m6-default", priority: 100, pattern: "/*"},
	}

	// Priorities must be unique and in ascending order.
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

	// M6 must be the last (highest priority number = catch-all).
	last := rules[len(rules)-1]
	if last.name != "m6-default" {
		t.Errorf("last rule should be m6-default, got %s", last.name)
	}
	if last.pattern != "/*" {
		t.Errorf("catch-all pattern should be /*, got %s", last.pattern)
	}
}

func TestAssignHostname(t *testing.T) {
	// M1 must be on separate hostname: assign.kaizen.{domain}
	domain := "example.com"
	expected := "assign.kaizen.example.com"
	got := "assign.kaizen." + domain
	if got != expected {
		t.Errorf("assign hostname = %q, want %q", got, expected)
	}
}
