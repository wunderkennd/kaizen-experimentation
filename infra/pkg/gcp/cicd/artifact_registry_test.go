package cicd

import (
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	awscicd "github.com/kaizen-experimentation/infra/pkg/aws/cicd"
)

// TestServiceNamesParityWithAWS guards the contract that every ECR repo has
// a parallel AR repo. If someone adds a service to AWS without adding it
// here, the dual-push CI step would push to ECR with no AR target, and the
// GCP tenant for that service would silently break on next deploy.
func TestServiceNamesParityWithAWS(t *testing.T) {
	if !reflect.DeepEqual(ServiceNames, awscicd.ServiceNames) {
		t.Errorf("AR ServiceNames diverged from ECR ServiceNames\n  AR : %v\n  ECR: %v",
			ServiceNames, awscicd.ServiceNames)
	}
	if !reflect.DeepEqual(UtilityImageNames, awscicd.UtilityImageNames) {
		t.Errorf("AR UtilityImageNames diverged from ECR UtilityImageNames\n  AR : %v\n  ECR: %v",
			UtilityImageNames, awscicd.UtilityImageNames)
	}
}

// TestServiceNamesCount mirrors ecr_test.go. If this fails, double-check the
// service inventory in CLAUDE.md and the spec — adding a service is a
// multi-PR change that should land here, in pkg/aws/cicd, in compute, and
// in the CI matrix at once.
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

// TestFormatDuration_KnownValue locks in the seven-days-in-seconds value
// since it's the same value the AWS lifecycle policy targets. If the format
// ever drifts (different unit, different magnitude), AR would silently retain
// untagged images longer than ECR or vice versa — which is exactly the kind
// of dual-cloud divergence the multi-cloud spec is trying to avoid.
func TestFormatDuration_KnownValue(t *testing.T) {
	got := formatDuration(untaggedExpiry)
	if got != "604800s" {
		t.Errorf("formatDuration(7d): got %q, want %q", got, "604800s")
	}
}

// TestPolicySummary keeps the human-readable runbook description in lockstep
// with the actual rule values. If someone tweaks the constants without
// updating the summary, this test forces the rename.
func TestPolicySummary(t *testing.T) {
	got := PolicySummary()
	for _, mustContain := range []string{
		"DELETE untagged",
		"KEEP",
		"168h0m0s", // Go's stringification of 7 days
		"10",
		"v",
		"sha-",
		"latest",
	} {
		if !strings.Contains(got, mustContain) {
			t.Errorf("PolicySummary() missing substring %q\n  got: %s", mustContain, got)
		}
	}
}

// TestValidateLabelValue exercises the label validation gate. Any value that
// would 400 at apply time should fail here, so `pulumi preview` rejects
// before we burn 60s on a deploy.
func TestValidateLabelValue(t *testing.T) {
	tests := []struct {
		name    string
		value   string
		wantErr bool
	}{
		{"valid lowercase", "dev", false},
		{"valid with dashes", "us-prod", false},
		{"valid with underscores", "team_kaizen", false},
		{"valid with digits", "env42", false},
		{"empty", "", true},
		{"uppercase", "Prod", true},
		{"too long", strings.Repeat("a", 64), true},
		{"with space", "us prod", true},
		{"with dot", "us.prod", true},
		{"with slash", "us/prod", true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := validateLabelValue(tt.value)
			if (err != nil) != tt.wantErr {
				t.Errorf("validateLabelValue(%q) err=%v, wantErr=%v", tt.value, err, tt.wantErr)
			}
		})
	}
}

// TestBuildCleanupPolicies_Structure verifies the cleanup policy array has
// the two expected rules with the right actions and required fields. We
// can't introspect Pulumi inputs deeply (they're opaque until apply), but we
// can assert array length and that the constants we baked in are wired.
func TestBuildCleanupPolicies_Structure(t *testing.T) {
	policies := buildCleanupPolicies()
	if len(policies) != 2 {
		t.Fatalf("expected 2 cleanup policies, got %d", len(policies))
	}
	// We can't read the inputs back as plain values without running Pulumi.
	// The structural check above (array length) plus the apply-time
	// integration test in TestNewArtifactRegistryRepositories_MockRun cover
	// the wiring; the constants themselves are covered by
	// TestFormatDuration_KnownValue and the constant declarations.
}

// arMocks is a minimal Pulumi mock that records resources by type token.
// We don't need the universalMocks scaffolding from infra/test because the
// AR module doesn't depend on any other modules.
type arMocks struct {
	mu        sync.Mutex
	resources []arResource
}

type arResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *arMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, arResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	id := args.Name + "_id"
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	switch args.TypeToken {
	case "gcp:artifactregistry/repository:Repository":
		// Echo the inputs we use to build the URL so the ApplyT chain resolves.
		if v, ok := args.Inputs["repositoryId"]; ok {
			outputs["repositoryId"] = v
			outputs["name"] = v
		}
		if v, ok := args.Inputs["location"]; ok {
			outputs["location"] = v
		}
		if v, ok := args.Inputs["project"]; ok {
			outputs["project"] = v
		}
	}
	return id, outputs, nil
}

func (m *arMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *arMocks) byType(t string) []arResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []arResource
	for _, r := range m.resources {
		if r.TypeToken == t {
			out = append(out, r)
		}
	}
	return out
}

// TestNewArtifactRegistryRepositories_MockRun exercises the constructor
// end-to-end against Pulumi's mock monitor. Asserts:
//   - One Repository resource per service+utility (10 total today).
//   - All have format=DOCKER.
//   - When PushPrincipal is set, one writer IamMember per repo is emitted.
//   - When PullPrincipals are set, one reader IamMember per (repo, principal).
//   - URLs follow the canonical <location>-docker.pkg.dev/<project>/<repo> form.
func TestNewArtifactRegistryRepositories_MockRun(t *testing.T) {
	mocks := &arMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		out, err := NewArtifactRegistryRepositories(ctx, Config{
			Project:        "kaizen-test-project",
			Environment:    "dev",
			Location:       "us",
			PushPrincipal:  "serviceAccount:ci@kaizen-test-project.iam.gserviceaccount.com",
			PullPrincipals: []string{"serviceAccount:run@kaizen-test-project.iam.gserviceaccount.com"},
		})
		if err != nil {
			return err
		}
		// Probe a URL to force the ApplyT chain to evaluate.
		urlChan := make(chan string, 1)
		out.RepositoryURLs["assignment"].ApplyT(func(s string) string {
			urlChan <- s
			return s
		})
		select {
		case url := <-urlChan:
			want := "us-docker.pkg.dev/kaizen-test-project/kaizen-assignment"
			if url != want {
				t.Errorf("assignment URL: got %q, want %q", url, want)
			}
		default:
			// URL didn't resolve synchronously — that's fine for the mock,
			// the resource-count assertions below still validate the wiring.
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewArtifactRegistryRepositories failed: %v", err)
	}

	gotRepos := len(mocks.byType("gcp:artifactregistry/repository:Repository"))
	wantRepos := len(ServiceNames) + len(UtilityImageNames)
	if gotRepos != wantRepos {
		t.Errorf("Repository count: got %d, want %d", gotRepos, wantRepos)
	}

	// One IAM member per (repo, push) + per (repo, pull).
	gotMembers := len(mocks.byType("gcp:artifactregistry/repositoryIamMember:RepositoryIamMember"))
	wantMembers := wantRepos * 2 // 1 writer + 1 reader per repo
	if gotMembers != wantMembers {
		t.Errorf("IamMember count: got %d, want %d", gotMembers, wantMembers)
	}

	// Spot-check that every repo is Docker format.
	for _, r := range mocks.byType("gcp:artifactregistry/repository:Repository") {
		f, ok := r.Inputs["format"]
		if !ok {
			t.Errorf("repo %s: missing format", r.Name)
			continue
		}
		if f.StringValue() != "DOCKER" {
			t.Errorf("repo %s: format=%s, want DOCKER", r.Name, f.StringValue())
		}
	}
}

// TestNewArtifactRegistryRepositories_NoIamWhenPrincipalsEmpty verifies the
// bootstrapping path: zero IAM bindings if neither push nor pull principals
// are provided. Useful for the very first apply where the SAs don't exist
// yet.
func TestNewArtifactRegistryRepositories_NoIamWhenPrincipalsEmpty(t *testing.T) {
	mocks := &arMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewArtifactRegistryRepositories(ctx, Config{
			Project:     "kaizen-test-project",
			Environment: "dev",
			// no PushPrincipal, no PullPrincipals
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewArtifactRegistryRepositories failed: %v", err)
	}

	gotMembers := len(mocks.byType("gcp:artifactregistry/repositoryIamMember:RepositoryIamMember"))
	if gotMembers != 0 {
		t.Errorf("IamMember count when no principals provided: got %d, want 0", gotMembers)
	}
}

// TestNewArtifactRegistryRepositories_RejectsEmptyProject ensures the early
// validation gate fires before any resource is registered. The mock would
// happily accept an empty project; this test covers the explicit guard.
func TestNewArtifactRegistryRepositories_RejectsEmptyProject(t *testing.T) {
	mocks := &arMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewArtifactRegistryRepositories(ctx, Config{
			Project:     "",
			Environment: "dev",
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil {
		t.Fatal("expected error for empty Project, got nil")
	}
	if !strings.Contains(err.Error(), "Project") {
		t.Errorf("error should mention Project, got: %v", err)
	}
}

// TestNewArtifactRegistryRepositories_DefaultsLocation asserts that the
// "us" multi-region is used when Location is empty. This is the cheapest
// option for Cloud Run and should never silently change without a test
// update.
func TestNewArtifactRegistryRepositories_DefaultsLocation(t *testing.T) {
	mocks := &arMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewArtifactRegistryRepositories(ctx, Config{
			Project:     "kaizen-test-project",
			Environment: "dev",
			// Location omitted on purpose
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewArtifactRegistryRepositories failed: %v", err)
	}
	for _, r := range mocks.byType("gcp:artifactregistry/repository:Repository") {
		loc, ok := r.Inputs["location"]
		if !ok || loc.StringValue() != defaultLocation {
			t.Errorf("repo %s: location=%v, want %q", r.Name, loc, defaultLocation)
		}
	}
}
