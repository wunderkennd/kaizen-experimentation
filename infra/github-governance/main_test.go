package main

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ---------------------------------------------------------------------------
// Mock infrastructure (house pattern — mirrors infra/fullstack_test.go)
// ---------------------------------------------------------------------------

type govResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

type govMocks struct {
	mu        sync.Mutex
	resources []govResource
}

func (m *govMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, govResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}
	return args.Name + "_id", outputs, nil
}

func (m *govMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *govMocks) byType(token string) []govResource {
	var out []govResource
	for _, r := range m.resources {
		if r.TypeToken == token {
			out = append(out, r)
		}
	}
	return out
}

const (
	repoRulesetToken = "github:index/repositoryRuleset:RepositoryRuleset"
	orgRulesetToken  = "github:index/organizationRuleset:OrganizationRuleset"
)

func run(t *testing.T, spec Spec) *govMocks {
	t.Helper()
	mocks := &govMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		return Deploy(ctx, spec)
	}, pulumi.WithMocks("kaizen-github-governance", "governance", mocks))
	if err != nil {
		t.Fatalf("Deploy failed: %v", err)
	}
	return mocks
}

func fleet() Spec {
	return Spec{
		GovernanceChecks: []string{"PR title check / check", "Review gate / gate"},
		Repos: []RepoSpec{
			{Owner: "wunderkennd", Name: "kaizen-experimentation", Enforcement: "active",
				RequiredChecks: []string{"schema", "rust", "go", "typescript", "hash-parity"}},
			{Owner: "wunderkennd", Name: "kaizen-recsys"},
			{Owner: "wunderkennd", Name: "kaizen-pipelines"},
		},
		OrgName:         "wunderkind-ventures",
		OrgRepoPatterns: []string{"kaizen-*", "kensho-*"},
	}
}

// ---------------------------------------------------------------------------
// Repo mode
// ---------------------------------------------------------------------------

func TestRepoModeStampsOneRulesetPerRepo(t *testing.T) {
	mocks := run(t, fleet())
	rulesets := mocks.byType(repoRulesetToken)
	if len(rulesets) != 3 {
		t.Fatalf("want 3 repo rulesets, got %d", len(rulesets))
	}
	if orgs := mocks.byType(orgRulesetToken); len(orgs) != 0 {
		t.Fatalf("repo mode must not create org rulesets, got %d", len(orgs))
	}
}

func TestRepoModeMergesGovernanceAndRepoChecks(t *testing.T) {
	mocks := run(t, fleet())
	for _, rs := range mocks.byType(repoRulesetToken) {
		if !strings.Contains(rs.Name, "kaizen-experimentation") {
			continue
		}
		checks := rs.Inputs["rules"].ObjectValue()["requiredStatusChecks"].
			ObjectValue()["requiredChecks"].ArrayValue()
		var contexts []string
		for _, c := range checks {
			contexts = append(contexts, c.ObjectValue()["context"].StringValue())
		}
		want := []string{"PR title check / check", "Review gate / gate",
			"schema", "rust", "go", "typescript", "hash-parity"}
		if len(contexts) != len(want) {
			t.Fatalf("contexts = %v, want %v", contexts, want)
		}
		for i := range want {
			if contexts[i] != want[i] {
				t.Fatalf("contexts[%d] = %q, want %q (governance checks must come first)", i, contexts[i], want[i])
			}
		}
		return
	}
	t.Fatal("kaizen-experimentation ruleset not found")
}

func TestSiblingsDefaultToDisabledEnforcement(t *testing.T) {
	mocks := run(t, fleet())
	for _, rs := range mocks.byType(repoRulesetToken) {
		enforcement := rs.Inputs["enforcement"].StringValue()
		if strings.Contains(rs.Name, "kaizen-experimentation") {
			if enforcement != "active" {
				t.Fatalf("kaizen-experimentation enforcement = %q, want active", enforcement)
			}
		} else if enforcement != "disabled" {
			t.Fatalf("%s enforcement = %q, want disabled (caller workflows not onboarded yet)", rs.Name, enforcement)
		}
	}
}

func TestRepoModeGovernanceOnlySiblingStillGetsGovernanceChecks(t *testing.T) {
	mocks := run(t, fleet())
	for _, rs := range mocks.byType(repoRulesetToken) {
		if !strings.Contains(rs.Name, "kaizen-recsys") {
			continue
		}
		checks := rs.Inputs["rules"].ObjectValue()["requiredStatusChecks"].
			ObjectValue()["requiredChecks"].ArrayValue()
		if len(checks) != 2 {
			t.Fatalf("sibling with no repo checks should carry exactly the 2 governance checks, got %d", len(checks))
		}
		return
	}
	t.Fatal("kaizen-recsys ruleset not found")
}

// ---------------------------------------------------------------------------
// Org mode
// ---------------------------------------------------------------------------

func TestOrgModeEmitsOrgRulesetAndSlimsRepoRulesets(t *testing.T) {
	spec := fleet()
	spec.OrgMode = true
	mocks := run(t, spec)

	orgs := mocks.byType(orgRulesetToken)
	if len(orgs) != 1 {
		t.Fatalf("want exactly 1 org ruleset, got %d", len(orgs))
	}
	org := orgs[0]
	if got := org.Inputs["enforcement"].StringValue(); got != "evaluate" {
		t.Fatalf("org enforcement default = %q, want evaluate (dry-run while onboarding)", got)
	}
	patterns := org.Inputs["conditions"].ObjectValue()["repositoryName"].
		ObjectValue()["includes"].ArrayValue()
	if len(patterns) != 2 {
		t.Fatalf("org ruleset repo patterns = %v, want 2", patterns)
	}

	// Per-repo rulesets in org mode must NOT duplicate the universal rules:
	// governance contexts ride the org ruleset only.
	for _, rs := range mocks.byType(repoRulesetToken) {
		if !strings.Contains(rs.Name, "kaizen-experimentation") {
			continue
		}
		checks := rs.Inputs["rules"].ObjectValue()["requiredStatusChecks"].
			ObjectValue()["requiredChecks"].ArrayValue()
		for _, c := range checks {
			ctx := c.ObjectValue()["context"].StringValue()
			if strings.Contains(ctx, "Review gate") || strings.Contains(ctx, "PR title") {
				t.Fatalf("org mode: governance check %q must not be duplicated in the repo ruleset", ctx)
			}
		}
	}
}

func TestOrgModeRequiresOrgName(t *testing.T) {
	spec := fleet()
	spec.OrgMode = true
	spec.OrgName = ""
	mocks := &govMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		return Deploy(ctx, spec)
	}, pulumi.WithMocks("kaizen-github-governance", "governance", mocks))
	if err == nil {
		t.Fatal("orgMode without orgName must fail")
	}
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

func TestRepoEntryMissingNameFails(t *testing.T) {
	spec := Spec{Repos: []RepoSpec{{Owner: "wunderkennd"}}}
	mocks := &govMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		return Deploy(ctx, spec)
	}, pulumi.WithMocks("kaizen-github-governance", "governance", mocks))
	if err == nil {
		t.Fatal("repo entry without name must fail")
	}
}
