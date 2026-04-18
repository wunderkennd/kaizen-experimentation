package config

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ---------------------------------------------------------------------------
// SecretPath
// ---------------------------------------------------------------------------

func TestSecretPath(t *testing.T) {
	tests := []struct {
		name string
		env  Environment
		sec  string
		want string
	}{
		{"dev database", EnvDev, "database", "kaizen/dev/database"},
		{"staging kafka", EnvStaging, "kafka", "kaizen/staging/kafka"},
		{"prod auth", EnvProd, "auth", "kaizen/prod/auth"},
		{"dev nested path", EnvDev, "rds/password", "kaizen/dev/rds/password"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := &Config{Env: tt.env}
			got := cfg.SecretPath(tt.sec)
			if got != tt.want {
				t.Errorf("SecretPath(%q) = %q, want %q", tt.sec, got, tt.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// ResourceName
// ---------------------------------------------------------------------------

func TestResourceName(t *testing.T) {
	tests := []struct {
		name string
		env  Environment
		res  string
		want string
	}{
		{"dev alb", EnvDev, "alb", "kaizen-dev-alb"},
		{"staging cluster", EnvStaging, "cluster", "kaizen-staging-cluster"},
		{"prod rds", EnvProd, "rds", "kaizen-prod-rds"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := &Config{Env: tt.env}
			got := cfg.ResourceName(tt.res)
			if got != tt.want {
				t.Errorf("ResourceName(%q) = %q, want %q", tt.res, got, tt.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// IsProd
// ---------------------------------------------------------------------------

func TestIsProd(t *testing.T) {
	tests := []struct {
		name string
		env  Environment
		want bool
	}{
		{"prod is prod", EnvProd, true},
		{"dev is not prod", EnvDev, false},
		{"staging is not prod", EnvStaging, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := &Config{Env: tt.env}
			if got := cfg.IsProd(); got != tt.want {
				t.Errorf("IsProd() = %v, want %v", got, tt.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// IsStaging
// ---------------------------------------------------------------------------

func TestIsStaging(t *testing.T) {
	tests := []struct {
		name string
		env  Environment
		want bool
	}{
		{"staging is staging", EnvStaging, true},
		{"dev is not staging", EnvDev, false},
		{"prod is not staging", EnvProd, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := &Config{Env: tt.env}
			if got := cfg.IsStaging(); got != tt.want {
				t.Errorf("IsStaging() = %v, want %v", got, tt.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// DefaultTags
// ---------------------------------------------------------------------------

func TestDefaultTags(t *testing.T) {
	tests := []struct {
		name string
		env  string
	}{
		{"dev tags", "dev"},
		{"staging tags", "staging"},
		{"prod tags", "prod"},
	}

	requiredKeys := []string{"Project", "Environment", "ManagedBy"}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tags := DefaultTags(tt.env)

			for _, key := range requiredKeys {
				if _, ok := tags[key]; !ok {
					t.Errorf("DefaultTags(%q) missing key %q", tt.env, key)
				}
			}

			// Verify values via type assertion (pulumi.String is type string).
			assertTagValue(t, tags, "Project", "kaizen")
			assertTagValue(t, tags, "Environment", tt.env)
			assertTagValue(t, tags, "ManagedBy", "pulumi")
		})
	}
}

// ---------------------------------------------------------------------------
// MergeTags
// ---------------------------------------------------------------------------

func TestMergeTags(t *testing.T) {
	t.Run("extra overrides base on conflict", func(t *testing.T) {
		base := pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String("dev"),
		}
		extra := pulumi.StringMap{
			"Environment": pulumi.String("prod"),
			"Team":        pulumi.String("platform"),
		}

		merged := MergeTags(base, extra)

		// "Environment" should be overridden to "prod".
		assertTagValue(t, merged, "Environment", "prod")
		// "Project" inherited from base.
		assertTagValue(t, merged, "Project", "kaizen")
		// "Team" added from extra.
		assertTagValue(t, merged, "Team", "platform")
	})

	t.Run("empty extra returns base copy", func(t *testing.T) {
		base := pulumi.StringMap{
			"Project": pulumi.String("kaizen"),
		}
		merged := MergeTags(base, pulumi.StringMap{})

		assertTagValue(t, merged, "Project", "kaizen")
		if len(merged) != 1 {
			t.Errorf("expected 1 tag, got %d", len(merged))
		}
	})

	t.Run("empty base returns extra copy", func(t *testing.T) {
		extra := pulumi.StringMap{
			"Team": pulumi.String("platform"),
		}
		merged := MergeTags(pulumi.StringMap{}, extra)

		assertTagValue(t, merged, "Team", "platform")
		if len(merged) != 1 {
			t.Errorf("expected 1 tag, got %d", len(merged))
		}
	})
}

// assertTagValue checks that a pulumi.StringMap key holds the expected string.
func assertTagValue(t *testing.T, tags pulumi.StringMap, key, want string) {
	t.Helper()

	v, ok := tags[key]
	if !ok {
		t.Errorf("tag %q not found", key)
		return
	}

	// pulumi.String is type string, so type-assert the StringInput.
	got, ok := v.(pulumi.String)
	if !ok {
		t.Errorf("tag %q: expected pulumi.String, got %T", key, v)
		return
	}

	if string(got) != want {
		t.Errorf("tag %q = %q, want %q", key, string(got), want)
	}
}
