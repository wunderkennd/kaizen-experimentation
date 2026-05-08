// Package cicd provides GCP CI/CD infrastructure resources — primarily
// Artifact Registry repositories that mirror the AWS ECR set provisioned in
// pkg/aws/cicd. Both modules contribute to types.CICDOutputs so that the
// cicdOut.RepositoryURLs map consumed by the compute layer is provider-agnostic.
//
// The CI image pipeline builds once (multi-arch) and pushes the resulting tag
// and digest to BOTH registries in parallel — see .github/workflows/ci.yml.
// Pulumi here only owns the registry surface; the build pipeline is owned by
// CI and credential rotation is documented in
// docs/runbooks/artifact-registry-credentials.md.
package cicd

import (
	"fmt"
	"strings"
	"time"
	"unicode"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/artifactregistry"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ArtifactRegistryOutputs is the GCP analogue of pkg/aws/cicd.ECROutputs.
// RepositoryURLs values are pull/push-compatible Docker image refs of the
// form `<location>-docker.pkg.dev/<project>/<repo>` — append `:<tag>` to land
// at a specific image. Returning the same map shape lets the GCP facade
// populate types.CICDOutputs.RepositoryURLs without any compute-layer changes.
type ArtifactRegistryOutputs struct {
	// RepositoryURLs maps service name → fully-qualified Docker repository URL.
	RepositoryURLs map[string]pulumi.StringOutput
	// RepositoryIds maps service name → bare repository ID (for IAM and
	// gcloud tooling that operate on the repo resource directly).
	RepositoryIds map[string]pulumi.StringOutput
}

// ServiceNames lists all 9 Kaizen services that need an Artifact Registry
// repository. Kept identical to pkg/aws/cicd.ServiceNames to enforce parity —
// the unit test cross-checks this against the AWS list at compile time, and
// the CI dual-push job iterates this list when pushing.
var ServiceNames = []string{
	"assignment",
	"pipeline",
	"orchestration",
	"metrics",
	"analysis",
	"policy",
	"management",
	"ui",
	"flags",
}

// UtilityImageNames lists infrastructure utility images that need a
// repository alongside the application services. Kept in sync with the AWS
// list (currently just "healthgate", a startup-ordering init container).
//
// M4b ships in the same container image as `policy` — the spec calls it out
// explicitly in the Container Image Strategy section, so no separate repo is
// allocated for it. If M4b ever forks its image, add "policy-m4b" here AND in
// pkg/aws/cicd.UtilityImageNames so dual-push stays symmetric.
var UtilityImageNames = []string{
	"healthgate",
}

// Config controls registry creation. Kept tight on purpose — anything that
// varies per-tenant lives in Pulumi stack config, anything universal lives
// in defaults below.
type Config struct {
	// Environment is the deployment environment ("dev", "staging", "prod"). Used
	// for resource labelling so AR repos can be tracked per stack.
	Environment string

	// Location is the AR multi-region or region. Defaults to "us" (multi-region)
	// because Cloud Run can pull from any multi-region without egress charges.
	// Override to a specific region (e.g. "us-central1") for data-residency
	// constraints or to colocate with Cloud Run for marginally faster pulls.
	Location string

	// Project is the GCP project ID hosting the registry. Required. The
	// caller (pkg/gcp/gcp.go facade) reads it from stack config.
	Project string

	// PushPrincipal is the IAM principal (e.g. "serviceAccount:ci@...gserviceaccount.com"
	// or "principalSet://iam.googleapis.com/projects/.../locations/global/workloadIdentityPools/.../*")
	// that the CI pipeline impersonates when pushing images. Granted
	// roles/artifactregistry.writer per repo.
	//
	// May be empty when bootstrapping (project owner manually pushes the first
	// image) — IAM bindings only emit when this is set.
	PushPrincipal string

	// PullPrincipals is the list of GCP service account principals that need to
	// pull images. Granted roles/artifactregistry.reader per repo. The Cloud
	// Run runtime service account belongs here. Order is not significant.
	PullPrincipals []string
}

// defaultLocation is the multi-region used when Config.Location is empty.
// "us" is the cheapest cross-region option for Cloud Run in any US region.
const defaultLocation = "us"

// untaggedExpiryDays mirrors pkg/aws/cicd.lifecycleRule rule 1 (expire
// untagged after 7 days). Expressed as a duration string per the AR API.
const untaggedExpiry = 7 * 24 * time.Hour

// keepTaggedCount mirrors pkg/aws/cicd.lifecycleRule rule 2 (keep last 10
// tagged images). AR's `KEEP` action retains; absent versions get swept by
// the `DELETE` action below.
const keepTaggedCount = 10

// taggedTagPrefixes mirrors the AWS lifecycle policy's tagPrefixList — only
// images tagged with these prefixes are eligible for the keep-N rule. Both
// providers use the same prefix set so a CI tag bucket can't be unlucky on
// one cloud and not the other.
var taggedTagPrefixes = []string{"v", "sha-", "latest"}

// NewArtifactRegistryRepositories provisions one Docker-format Artifact
// Registry repository per service in ServiceNames + UtilityImageNames.
//
// Each repository:
//   - Is Docker-format (mode=STANDARD_REPOSITORY).
//   - Carries cleanup policies in dry-run=false enforcement mode that match
//     the ECR lifecycle policy semantically:
//   - Rule 1: DELETE untagged versions older than 7 days.
//   - Rule 2: KEEP the most recent 10 tagged versions (prefixed v|sha-|latest).
//   - Has Project/Service/Env/ManagedBy labels for cost attribution and
//     filtering in `gcloud artifacts` listings.
//   - Optionally binds the CI push principal to roles/artifactregistry.writer
//     and any Cloud Run runtime SAs to roles/artifactregistry.reader.
//
// The function never deletes existing repositories — it only registers them
// with Pulumi. If callers rename a service, Pulumi will create the new repo
// and leave the old one for manual cleanup (intentional: image registries
// are append-only by convention).
func NewArtifactRegistryRepositories(ctx *pulumi.Context, cfg Config) (*ArtifactRegistryOutputs, error) {
	if cfg.Project == "" {
		return nil, fmt.Errorf("artifactregistry: Config.Project is required")
	}
	if err := validateLabelValue(cfg.Environment); err != nil {
		return nil, fmt.Errorf("artifactregistry: invalid environment label %q: %w", cfg.Environment, err)
	}

	location := cfg.Location
	if location == "" {
		location = defaultLocation
	}

	allImages := make([]string, 0, len(ServiceNames)+len(UtilityImageNames))
	allImages = append(allImages, ServiceNames...)
	allImages = append(allImages, UtilityImageNames...)

	urls := make(map[string]pulumi.StringOutput, len(allImages))
	ids := make(map[string]pulumi.StringOutput, len(allImages))

	for _, svc := range allImages {
		repoID := fmt.Sprintf("kaizen-%s", svc)
		resourceName := fmt.Sprintf("ar-%s", svc)

		repo, err := artifactregistry.NewRepository(ctx, resourceName, &artifactregistry.RepositoryArgs{
			Project:             pulumi.String(cfg.Project),
			Location:            pulumi.String(location),
			RepositoryId:        pulumi.String(repoID),
			Format:              pulumi.String("DOCKER"),
			Description:         pulumi.String(fmt.Sprintf("Kaizen %s service container images", svc)),
			Mode:                pulumi.String("STANDARD_REPOSITORY"),
			CleanupPolicyDryRun: pulumi.Bool(false),
			CleanupPolicies:     buildCleanupPolicies(),
			Labels: pulumi.StringMap{
				"project":    pulumi.String("kaizen"),
				"service":    pulumi.String(svc),
				"env":        pulumi.String(cfg.Environment),
				"managed-by": pulumi.String("pulumi"),
			},
		})
		if err != nil {
			return nil, fmt.Errorf("creating Artifact Registry repo %s: %w", repoID, err)
		}

		// Construct the canonical pull/push URL. AR exposes Location, Project,
		// and RepositoryId as outputs — we materialise the standard URL form
		// `<location>-docker.pkg.dev/<project>/<repo>` so downstream consumers
		// (compute, CI) get a string identical in shape to ECR's RepositoryUrl.
		urls[svc] = pulumi.All(repo.Location, repo.Project, repo.RepositoryId).
			ApplyT(func(args []interface{}) string {
				loc := args[0].(string)
				proj := args[1].(string)
				rid := args[2].(string)
				return fmt.Sprintf("%s-docker.pkg.dev/%s/%s", loc, proj, rid)
			}).(pulumi.StringOutput)
		ids[svc] = repo.RepositoryId

		// Wire IAM only when principals are provided. Each binding is
		// separate so revoking write access does not require touching read
		// bindings (this matches the rotation runbook's revoke procedure).
		if cfg.PushPrincipal != "" {
			_, err := artifactregistry.NewRepositoryIamMember(ctx, fmt.Sprintf("ar-%s-push", svc), &artifactregistry.RepositoryIamMemberArgs{
				Project:    repo.Project,
				Location:   repo.Location,
				Repository: repo.Name,
				Role:       pulumi.String("roles/artifactregistry.writer"),
				Member:     pulumi.String(cfg.PushPrincipal),
			})
			if err != nil {
				return nil, fmt.Errorf("granting writer to %s on %s: %w", cfg.PushPrincipal, repoID, err)
			}
		}
		for i, pull := range cfg.PullPrincipals {
			if pull == "" {
				continue
			}
			_, err := artifactregistry.NewRepositoryIamMember(ctx, fmt.Sprintf("ar-%s-pull-%d", svc, i), &artifactregistry.RepositoryIamMemberArgs{
				Project:    repo.Project,
				Location:   repo.Location,
				Repository: repo.Name,
				Role:       pulumi.String("roles/artifactregistry.reader"),
				Member:     pulumi.String(pull),
			})
			if err != nil {
				return nil, fmt.Errorf("granting reader to %s on %s: %w", pull, repoID, err)
			}
		}
	}

	return &ArtifactRegistryOutputs{
		RepositoryURLs: urls,
		RepositoryIds:  ids,
	}, nil
}

// buildCleanupPolicies returns the two-rule cleanup policy that mirrors the
// ECR lifecycle policy in pkg/aws/cicd. Order does not matter for AR — the
// API resolves them per version.
func buildCleanupPolicies() artifactregistry.RepositoryCleanupPolicyArray {
	return artifactregistry.RepositoryCleanupPolicyArray{
		// Rule 1: DELETE untagged versions older than 7 days. Mirrors the
		// AWS rule "expire untagged images after 7 days".
		&artifactregistry.RepositoryCleanupPolicyArgs{
			Id:     pulumi.String("delete-untagged-after-7d"),
			Action: pulumi.String("DELETE"),
			Condition: &artifactregistry.RepositoryCleanupPolicyConditionArgs{
				TagState:  pulumi.String("UNTAGGED"),
				OlderThan: pulumi.String(formatDuration(untaggedExpiry)),
			},
		},
		// Rule 2: KEEP the last 10 tagged versions (with the same tag-prefix
		// allowlist as ECR). KEEP rules win over DELETE when both apply, so
		// this rule guarantees we never strand the latest releases even if
		// some other DELETE rule were ever introduced.
		&artifactregistry.RepositoryCleanupPolicyArgs{
			Id:     pulumi.String("keep-last-10-tagged"),
			Action: pulumi.String("KEEP"),
			MostRecentVersions: &artifactregistry.RepositoryCleanupPolicyMostRecentVersionsArgs{
				KeepCount: pulumi.Int(keepTaggedCount),
			},
			Condition: &artifactregistry.RepositoryCleanupPolicyConditionArgs{
				TagState:    pulumi.String("TAGGED"),
				TagPrefixes: pulumi.ToStringArray(taggedTagPrefixes),
			},
		},
	}
}

// formatDuration renders a Go time.Duration as the seconds-suffixed string
// expected by the AR API ("604800s" for 7 days). The AR docs explicitly call
// out the `s` suffix as required.
func formatDuration(d time.Duration) string {
	return fmt.Sprintf("%ds", int64(d.Seconds()))
}

// validateLabelValue rejects values that GCP would reject at apply time —
// we want the failure surface to be `pulumi preview` and not a 400 from the
// real API after a 90-second deploy. GCP labels: lowercase letters, numerics,
// underscores, dashes; ≤ 63 chars; non-empty.
func validateLabelValue(v string) error {
	if v == "" {
		return fmt.Errorf("must not be empty")
	}
	if len(v) > 63 {
		return fmt.Errorf("must be ≤ 63 chars (got %d)", len(v))
	}
	for _, r := range v {
		if r == '-' || r == '_' {
			continue
		}
		if unicode.IsDigit(r) {
			continue
		}
		if unicode.IsLower(r) && unicode.IsLetter(r) {
			continue
		}
		return fmt.Errorf("character %q not allowed (lowercase letters, digits, _, - only)", r)
	}
	return nil
}

// PolicySummary returns a human-readable summary of the cleanup policy for
// inclusion in `pulumi preview` output and runbooks. It MUST stay in sync
// with buildCleanupPolicies; the unit test asserts both views describe the
// same policy.
func PolicySummary() string {
	return strings.Join([]string{
		fmt.Sprintf("DELETE untagged versions older than %s", untaggedExpiry),
		fmt.Sprintf("KEEP the most recent %d tagged versions (prefixes: %s)",
			keepTaggedCount, strings.Join(taggedTagPrefixes, ", ")),
	}, "; ")
}
