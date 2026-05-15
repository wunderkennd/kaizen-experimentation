// Package compute — unit tests for the pure helpers in compute.go that
// don't need a Pulumi context. Topology behavior (resource registration,
// IAM bindings, Service Directory wiring) is tested via mocks in
// infra/test/gcp_compute_topology_test.go.
package compute

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// ---------------------------------------------------------------------------
// saAccountID — derives the GCP service-account local ID for a service.
// ---------------------------------------------------------------------------

func TestSaAccountIDHappyPath(t *testing.T) {
	cases := []struct {
		env, name, want string
	}{
		{"dev", "m1-assignment", "dev-m1-assignment-run"},
		{"dev", "m2-orchestration", "dev-m2-orchestration-run"},
		{"staging", "m7-flags", "staging-m7-flags-run"},
		{"prod", "m6-ui", "prod-m6-ui-run"},
	}
	for _, tc := range cases {
		t.Run(tc.env+"/"+tc.name, func(t *testing.T) {
			got, err := saAccountID(tc.env, tc.name)
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if got != tc.want {
				t.Errorf("got %q, want %q", got, tc.want)
			}
		})
	}
}

// TestSaAccountIDRejectsOverflow locks the GCP 30-char accountId ceiling.
// Without this, deploys would surface as opaque IAM 400s at apply time.
func TestSaAccountIDRejectsOverflow(t *testing.T) {
	// "staging-some-very-long-service-name-run" = 39 chars > 30
	_, err := saAccountID("staging", "some-very-long-service-name")
	if err == nil {
		t.Fatal("expected overflow error, got nil")
	}
	if !strings.Contains(err.Error(), "exceeds the 30-char GCP maximum") {
		t.Errorf("error %q does not mention the 30-char ceiling", err.Error())
	}
}

// ---------------------------------------------------------------------------
// isValidServiceName — DNS-label + length constraints
// ---------------------------------------------------------------------------

func TestIsValidServiceName(t *testing.T) {
	cases := []struct {
		name string
		want bool
	}{
		// Valid
		{"m1-assignment", true},
		{"m2-orchestration", true},
		{"a", true},
		{"preview-canary", true},
		{strings.Repeat("a", 49), true},

		// Invalid
		{"", false},
		{"M1-Assignment", false}, // uppercase
		{"-m1", false},           // leading hyphen
		{"1m-test", false},       // leading digit
		{"m1_test", false},       // underscore
		{"m1.test", false},       // dot
		{strings.Repeat("a", 50), false}, // overflow
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := isValidServiceName(tc.name); got != tc.want {
				t.Errorf("isValidServiceName(%q) = %v, want %v", tc.name, got, tc.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// slug — IAM role to Pulumi-resource-name suffix
// ---------------------------------------------------------------------------

func TestSlug(t *testing.T) {
	cases := []struct {
		in, want string
	}{
		{"roles/cloudsql.client", "cloudsql-client"},
		{"roles/secretmanager.secretAccessor", "secretmanager-secretaccessor"},
		{"roles/storage.objectAdmin", "storage-objectadmin"},
		{"roles/redis.editor", "redis-editor"},
		// Non-roles strings should still pass through cleanly.
		{"my_custom.role", "my-custom-role"},
	}
	for _, tc := range cases {
		t.Run(tc.in, func(t *testing.T) {
			if got := slug(tc.in); got != tc.want {
				t.Errorf("slug(%q) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// sortedUnique — deterministic role list to keep Pulumi diffs stable
// ---------------------------------------------------------------------------

func TestSortedUnique(t *testing.T) {
	got := sortedUnique([]string{
		"roles/redis.editor",
		"roles/cloudsql.client",
		"roles/cloudsql.client", // duplicate
		"roles/storage.objectAdmin",
	})
	want := []string{
		"roles/cloudsql.client",
		"roles/redis.editor",
		"roles/storage.objectAdmin",
	}
	if len(got) != len(want) {
		t.Fatalf("len mismatch: got %d, want %d", len(got), len(want))
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("index %d: got %q, want %q", i, got[i], want[i])
		}
	}
}

// ---------------------------------------------------------------------------
// stripScheme — Cloud Run URL → SD endpoint host
// ---------------------------------------------------------------------------

func TestStripScheme(t *testing.T) {
	cases := []struct {
		in, want string
	}{
		{"https://kaizen-dev-m1-mock-us-central1.a.run.app", "kaizen-dev-m1-mock-us-central1.a.run.app"},
		{"http://localhost:8080", "localhost:8080"},
		{"already-stripped.example.com", "already-stripped.example.com"},
		{"", ""},
	}
	for _, tc := range cases {
		t.Run(tc.in, func(t *testing.T) {
			if got := stripScheme(tc.in); got != tc.want {
				t.Errorf("stripScheme(%q) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// normalizeOptions — defaults applied
// ---------------------------------------------------------------------------

func TestNormalizeOptionsAppliesDefaults(t *testing.T) {
	got := normalizeOptions(Options{}, "preview-canary")
	if got.ContainerPort != 8080 {
		t.Errorf("default ContainerPort = %d, want 8080", got.ContainerPort)
	}
	if got.ServiceID != "preview-canary" {
		t.Errorf("default ServiceID = %q, want %q (= name)", got.ServiceID, "preview-canary")
	}
}

func TestNormalizeOptionsPreservesOverrides(t *testing.T) {
	got := normalizeOptions(Options{
		ContainerPort: 50051,
		ServiceID:     "custom-id",
		MinInstances:  3,
		MaxInstances:  10,
	}, "m1-assignment")
	if got.ContainerPort != 50051 {
		t.Errorf("ContainerPort = %d, want 50051", got.ContainerPort)
	}
	if got.ServiceID != "custom-id" {
		t.Errorf("ServiceID = %q, want %q", got.ServiceID, "custom-id")
	}
	if got.MinInstances != 3 {
		t.Errorf("MinInstances = %d, want 3", got.MinInstances)
	}
	if got.MaxInstances != 10 {
		t.Errorf("MaxInstances = %d, want 10", got.MaxInstances)
	}
}

// ---------------------------------------------------------------------------
// validateInputs — each rejection path
// ---------------------------------------------------------------------------

func TestValidateInputsRejections(t *testing.T) {
	// Build a baseline (valid) set of args; per-test mutates one field.
	baseCfg := func() *config.Config {
		return &config.Config{
			Project:      "kaizen",
			Environment:  "dev",
			GCPProjectID: "kaizen-experimentation-dev",
			GCPRegion:    "us-central1",
		}
	}
	baseInputs := func() *Inputs {
		return &Inputs{
			Project: "kaizen-experimentation-dev",
			Region:  "us-central1",
			VpcConnectorSelfLink: pulumi.String(
				"projects/p/locations/r/connectors/c").ToStringOutput(),
			ServiceDirectoryNamespaceID: pulumi.String(
				"projects/p/locations/r/namespaces/n").ToStringOutput(),
		}
	}
	baseOpts := func() *Options {
		return &Options{
			Image:         pulumi.String("img"),
			ContainerPort: 8080,
		}
	}

	cases := []struct {
		name    string
		mutate  func(cfg **config.Config, inputs **Inputs, opts **Options, name *string)
		wantErr string
	}{
		{
			name: "nil cfg",
			mutate: func(cfg **config.Config, _ **Inputs, _ **Options, _ *string) {
				*cfg = nil
			},
			wantErr: "cfg must not be nil",
		},
		{
			name: "empty GCPProjectID",
			mutate: func(cfg **config.Config, _ **Inputs, _ **Options, _ *string) {
				(*cfg).GCPProjectID = ""
			},
			wantErr: "cfg.GCPProjectID must not be empty",
		},
		{
			name: "empty Environment",
			mutate: func(cfg **config.Config, _ **Inputs, _ **Options, _ *string) {
				(*cfg).Environment = ""
			},
			wantErr: "cfg.Environment must not be empty",
		},
		{
			name: "empty GCPRegion",
			mutate: func(cfg **config.Config, _ **Inputs, _ **Options, _ *string) {
				(*cfg).GCPRegion = ""
			},
			wantErr: "cfg.GCPRegion must not be empty",
		},
		{
			name: "nil inputs",
			mutate: func(_ **config.Config, inputs **Inputs, _ **Options, _ *string) {
				*inputs = nil
			},
			wantErr: "inputs must not be nil",
		},
		{
			name: "empty name",
			mutate: func(_ **config.Config, _ **Inputs, _ **Options, name *string) {
				*name = ""
			},
			wantErr: "name must not be empty",
		},
		{
			name: "invalid name (uppercase)",
			mutate: func(_ **config.Config, _ **Inputs, _ **Options, name *string) {
				*name = "M1-Assignment"
			},
			wantErr: "must match [a-z][a-z0-9-]*",
		},
		{
			name: "nil opts",
			mutate: func(_ **config.Config, _ **Inputs, opts **Options, _ *string) {
				*opts = nil
			},
			wantErr: "opts must not be nil",
		},
		{
			name: "missing Image",
			mutate: func(_ **config.Config, _ **Inputs, opts **Options, _ *string) {
				(*opts).Image = nil
			},
			wantErr: "opts.Image is required",
		},
		{
			name: "negative MinInstances",
			mutate: func(_ **config.Config, _ **Inputs, opts **Options, _ *string) {
				(*opts).MinInstances = -1
			},
			wantErr: "MinInstances must be >= 0",
		},
		{
			name: "MaxInstances < MinInstances",
			mutate: func(_ **config.Config, _ **Inputs, opts **Options, _ *string) {
				(*opts).MinInstances = 5
				(*opts).MaxInstances = 2
			},
			wantErr: "MaxInstances (2) must be >= MinInstances (5)",
		},
		{
			name: "secret missing EnvName",
			mutate: func(_ **config.Config, _ **Inputs, opts **Options, _ *string) {
				(*opts).Secrets = []SecretEnv{
					{SecretID: pulumi.String("kaizen-dev-database")},
				}
			},
			wantErr: "EnvName + SecretID",
		},
		{
			name: "env var missing Value",
			mutate: func(_ **config.Config, _ **Inputs, opts **Options, _ *string) {
				(*opts).EnvVars = []EnvVar{{Name: "FOO"}}
			},
			wantErr: "Name + Value",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg := baseCfg()
			inputs := baseInputs()
			opts := baseOpts()
			name := "m1-assignment"
			tc.mutate(&cfg, &inputs, &opts, &name)

			err := validateInputs(cfg, inputs, name, opts)
			if err == nil {
				t.Fatalf("expected error containing %q, got nil", tc.wantErr)
			}
			if !strings.Contains(err.Error(), tc.wantErr) {
				t.Errorf("error %q does not contain %q", err.Error(), tc.wantErr)
			}
		})
	}
}

func TestValidateInputsHappyPath(t *testing.T) {
	cfg := &config.Config{
		Project:      "kaizen",
		Environment:  "dev",
		GCPProjectID: "kaizen-experimentation-dev",
		GCPRegion:    "us-central1",
	}
	inputs := &Inputs{
		Project: "kaizen-experimentation-dev",
		Region:  "us-central1",
		VpcConnectorSelfLink: pulumi.String(
			"projects/p/locations/r/connectors/c").ToStringOutput(),
		ServiceDirectoryNamespaceID: pulumi.String(
			"projects/p/locations/r/namespaces/n").ToStringOutput(),
	}
	opts := &Options{
		Image:         pulumi.String("img"),
		ContainerPort: 8080,
		EnvVars:       []EnvVar{{Name: "FOO", Value: pulumi.String("bar")}},
		Secrets: []SecretEnv{
			{EnvName: "DB", SecretID: pulumi.String("kaizen-dev-database")},
		},
	}
	if err := validateInputs(cfg, inputs, "m1-assignment", opts); err != nil {
		t.Errorf("happy-path validation failed: %v", err)
	}
}
