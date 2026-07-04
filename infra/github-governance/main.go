// Package main is the ecosystem GitHub-governance Pulumi program (proposal H6).
//
// It stamps the PR-lifecycle branch protection — the same intent as
// .github/rulesets/main.json — across every repo in the fleet, from stack
// config rather than per-repo clicking. Two layers:
//
//   - Per-repo RULESETS (repo mode, works under the wunderkennd user account
//     today): universal governance checks + that repo's own CI contexts.
//   - One ORGANIZATION ruleset (org mode, once repos transfer to
//     wunderkind-ventures and the org plan supports org rulesets): carries the
//     universal rules for every matching repo; per-repo rulesets then only add
//     repo-specific CI contexts. Rulesets aggregate, so the layering is safe.
//
// Repos are keyed by owner so a repo migrates user → org by editing stack
// config, not code. Auth: a fine-grained PAT with "Administration: write" on
// the target repos via GITHUB_TOKEN (fine-grained PATs are per-owner — see
// docs/runbooks/ecosystem-governance.md for the migration-window story).
package main

import (
	"fmt"

	"github.com/pulumi/pulumi-github/sdk/v6/go/github"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// RepoSpec is one governed repository, as written in stack config.
type RepoSpec struct {
	Owner string `json:"owner"`
	Name  string `json:"name"`
	// Enforcement: "active", "evaluate", or "disabled". "evaluate" (dry-run
	// + Rule Insights) is Enterprise-only for BOTH repo- and org-level
	// rulesets: github/docs gates the entire Evaluate status behind the
	// repo-rules-enterprise flag (ghec/ghes versions only — no fpt entry),
	// verified 2026-07-04. Siblings default to "disabled" until their
	// caller workflows exist — a required check that never reports would
	// otherwise block every merge.
	Enforcement string `json:"enforcement"`
	// RequiredChecks: repo-specific CI contexts (e.g. kaizen-experimentation's
	// schema/rust/go/typescript/hash-parity). Universal governance checks are
	// appended from Spec.GovernanceChecks — don't repeat them here.
	RequiredChecks []string `json:"requiredChecks"`
}

// Spec is the whole program input.
type Spec struct {
	Repos []RepoSpec `json:"repos"`
	// GovernanceChecks are required on every governed repo (the reusable
	// PR-lifecycle checks). Reusable-workflow check runs are named
	// "<caller job name> / <callee job name>" — verify against a live PR
	// before editing.
	GovernanceChecks []string `json:"governanceChecks"`
	// OrgMode: emit one organization ruleset for OrgName carrying the
	// universal rules; per-repo rulesets then carry only repo CI contexts.
	OrgMode bool `json:"orgMode"`
	// OrgName is the destination organization (wunderkind-ventures).
	OrgName string `json:"orgName"`
	// OrgRepoPatterns select which org repos the org ruleset targets,
	// fnmatch-style (e.g. "kaizen-*", "kensho-*").
	OrgRepoPatterns []string `json:"orgRepoPatterns"`
	// OrgEnforcement: enforcement for the org ruleset. Defaults to
	// "disabled" — NOT "evaluate": the evaluate (dry-run) status is an
	// Enterprise-plan feature (docs: ifversion repo-rules-enterprise); a
	// Team-plan org rejects it. Set it explicitly only on GHEC.
	OrgEnforcement string `json:"orgEnforcement"`
}

func loadSpec(ctx *pulumi.Context) (Spec, error) {
	cfg := config.New(ctx, "kaizen-github-governance")
	var spec Spec
	if err := cfg.TryObject("spec", &spec); err != nil {
		return Spec{}, fmt.Errorf("stack config kaizen-github-governance:spec is required: %w", err)
	}
	return spec, nil
}

// pullRequestRules is the universal PR-lifecycle rule block: no blanket
// approval count (graduated review — owner decision 2026-07-04, #681), but
// every review thread resolved before merge.
func pullRequestRules() *github.RepositoryRulesetRulesPullRequestArgs {
	return &github.RepositoryRulesetRulesPullRequestArgs{
		RequiredApprovingReviewCount:   pulumi.Int(0),
		RequiredReviewThreadResolution: pulumi.Bool(true),
		DismissStaleReviewsOnPush:      pulumi.Bool(false),
		RequireCodeOwnerReview:         pulumi.Bool(false),
		RequireLastPushApproval:        pulumi.Bool(false),
	}
}

func requiredChecks(contexts []string) github.RepositoryRulesetRulesRequiredStatusChecksRequiredCheckArray {
	arr := github.RepositoryRulesetRulesRequiredStatusChecksRequiredCheckArray{}
	for _, c := range contexts {
		arr = append(arr, &github.RepositoryRulesetRulesRequiredStatusChecksRequiredCheckArgs{
			Context: pulumi.String(c),
		})
	}
	return arr
}

// dedupAppend returns base ∪ extra, preserving order, first occurrence wins.
func dedupAppend(base, extra []string) []string {
	seen := map[string]bool{}
	var out []string
	for _, c := range append(append([]string{}, base...), extra...) {
		if c == "" || seen[c] {
			continue
		}
		seen[c] = true
		out = append(out, c)
	}
	return out
}

// Deploy provisions governance for the given spec. Split from main() so the
// mock suite can drive it with literal specs (house pattern: infra/ tests).
func Deploy(ctx *pulumi.Context, spec Spec) error {
	// One provider instance per owner: the github provider is owner-scoped,
	// and during the user→org migration the fleet spans both.
	providers := map[string]*github.Provider{}
	providerFor := func(owner string) (*github.Provider, error) {
		if p, ok := providers[owner]; ok {
			return p, nil
		}
		p, err := github.NewProvider(ctx, "github-"+owner, &github.ProviderArgs{
			Owner: pulumi.String(owner),
		})
		if err != nil {
			return nil, err
		}
		providers[owner] = p
		return p, nil
	}

	for _, r := range spec.Repos {
		if r.Owner == "" || r.Name == "" {
			return fmt.Errorf("repo entry missing owner or name: %+v", r)
		}
		enforcement := r.Enforcement
		if enforcement == "" {
			enforcement = "disabled"
		}

		// In org mode the universal rules ride the org ruleset; per-repo
		// rulesets carry only repo-specific CI contexts. In repo mode each
		// repo carries everything.
		contexts := r.RequiredChecks
		if !spec.OrgMode {
			contexts = dedupAppend(spec.GovernanceChecks, r.RequiredChecks)
		}

		prov, err := providerFor(r.Owner)
		if err != nil {
			return err
		}

		rules := &github.RepositoryRulesetRulesArgs{
			Deletion:              pulumi.Bool(true),
			NonFastForward:        pulumi.Bool(true),
			RequiredLinearHistory: pulumi.Bool(true),
		}
		if !spec.OrgMode {
			rules.PullRequest = pullRequestRules()
		}
		if len(contexts) > 0 {
			rules.RequiredStatusChecks = &github.RepositoryRulesetRulesRequiredStatusChecksArgs{
				RequiredChecks:                   requiredChecks(contexts),
				StrictRequiredStatusChecksPolicy: pulumi.Bool(false),
			}
		}

		_, err = github.NewRepositoryRuleset(ctx, fmt.Sprintf("%s-%s-main", r.Owner, r.Name), &github.RepositoryRulesetArgs{
			Name:        pulumi.String("main"),
			Repository:  pulumi.String(r.Name),
			Target:      pulumi.String("branch"),
			Enforcement: pulumi.String(enforcement),
			Conditions: &github.RepositoryRulesetConditionsArgs{
				RefName: &github.RepositoryRulesetConditionsRefNameArgs{
					Includes: pulumi.StringArray{pulumi.String("~DEFAULT_BRANCH")},
					Excludes: pulumi.StringArray{},
				},
			},
			Rules: rules,
		}, pulumi.Provider(prov))
		if err != nil {
			return err
		}
	}

	if spec.OrgMode {
		if spec.OrgName == "" {
			return fmt.Errorf("orgMode requires orgName")
		}
		orgEnforcement := spec.OrgEnforcement
		if orgEnforcement == "" {
			// "disabled", not "evaluate": evaluate is Enterprise-gated and a
			// Team-plan org would reject the ruleset outright.
			orgEnforcement = "disabled"
		}
		patterns := pulumi.StringArray{}
		for _, p := range spec.OrgRepoPatterns {
			patterns = append(patterns, pulumi.String(p))
		}
		prov, err := providerFor(spec.OrgName)
		if err != nil {
			return err
		}
		_, err = github.NewOrganizationRuleset(ctx, "org-governance", &github.OrganizationRulesetArgs{
			Name:        pulumi.String("kaizen-governance"),
			Target:      pulumi.String("branch"),
			Enforcement: pulumi.String(orgEnforcement),
			Conditions: &github.OrganizationRulesetConditionsArgs{
				RefName: &github.OrganizationRulesetConditionsRefNameArgs{
					Includes: pulumi.StringArray{pulumi.String("~DEFAULT_BRANCH")},
					Excludes: pulumi.StringArray{},
				},
				RepositoryName: &github.OrganizationRulesetConditionsRepositoryNameArgs{
					Includes: patterns,
					Excludes: pulumi.StringArray{},
				},
			},
			Rules: &github.OrganizationRulesetRulesArgs{
				Deletion:              pulumi.Bool(true),
				NonFastForward:        pulumi.Bool(true),
				RequiredLinearHistory: pulumi.Bool(true),
				PullRequest: &github.OrganizationRulesetRulesPullRequestArgs{
					RequiredApprovingReviewCount:   pulumi.Int(0),
					RequiredReviewThreadResolution: pulumi.Bool(true),
					DismissStaleReviewsOnPush:      pulumi.Bool(false),
					RequireCodeOwnerReview:         pulumi.Bool(false),
					RequireLastPushApproval:        pulumi.Bool(false),
				},
				RequiredStatusChecks: &github.OrganizationRulesetRulesRequiredStatusChecksArgs{
					RequiredChecks: func() github.OrganizationRulesetRulesRequiredStatusChecksRequiredCheckArray {
						arr := github.OrganizationRulesetRulesRequiredStatusChecksRequiredCheckArray{}
						for _, c := range spec.GovernanceChecks {
							arr = append(arr, &github.OrganizationRulesetRulesRequiredStatusChecksRequiredCheckArgs{
								Context: pulumi.String(c),
							})
						}
						return arr
					}(),
					StrictRequiredStatusChecksPolicy: pulumi.Bool(false),
				},
			},
		}, pulumi.Provider(prov))
		if err != nil {
			return err
		}
	}

	ctx.Export("governedRepos", pulumi.Int(len(spec.Repos)))
	ctx.Export("orgMode", pulumi.Bool(spec.OrgMode))
	return nil
}

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		spec, err := loadSpec(ctx)
		if err != nil {
			return err
		}
		return Deploy(ctx, spec)
	})
}
