// Package cicd provides CI/CD infrastructure resources (ECR repositories, build pipelines).
package cicd

import (
	"encoding/json"
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ecr"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ECROutputs contains the outputs from ECR repository creation.
type ECROutputs struct {
	// RepositoryURLs maps service name → ECR repository URL (for ECS task definitions).
	RepositoryURLs map[string]pulumi.StringOutput
	// RepositoryArns maps service name → ECR repository ARN (for IAM policies).
	RepositoryArns map[string]pulumi.StringOutput
}

// ServiceNames lists all 9 Kaizen services that need ECR repositories.
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

// UtilityImageNames lists infrastructure utility images that need ECR
// repositories. These are not application services — they support service
// orchestration (e.g., health-gate init containers for startup ordering).
var UtilityImageNames = []string{
	"healthgate",
}

// lifecyclePolicy defines the ECR lifecycle rules:
//   - Rule 1: expire untagged images after 7 days
//   - Rule 2: keep only the last 10 tagged images
type lifecyclePolicy struct {
	Rules []lifecycleRule `json:"rules"`
}

type lifecycleRule struct {
	RulePriority int             `json:"rulePriority"`
	Description  string          `json:"description"`
	Selection    lifecycleSelect `json:"selection"`
	Action       lifecycleAction `json:"action"`
}

type lifecycleSelect struct {
	TagStatus     string   `json:"tagStatus"`
	TagPrefixList []string `json:"tagPrefixList,omitempty"`
	CountType     string   `json:"countType"`
	CountUnit     string   `json:"countUnit,omitempty"`
	CountNumber   int      `json:"countNumber"`
}

type lifecycleAction struct {
	Type string `json:"type"`
}

// NewECRRepositories creates ECR repositories for all 9 Kaizen services.
// Each repository has:
//   - Image scanning on push enabled
//   - Lifecycle policy: expire untagged images after 7 days, keep last 10 tagged
//   - Consistent tagging with Project, Service, and ManagedBy tags
func NewECRRepositories(ctx *pulumi.Context, env string) (*ECROutputs, error) {
	policy, err := buildLifecyclePolicy()
	if err != nil {
		return nil, fmt.Errorf("building ECR lifecycle policy: %w", err)
	}

	allImages := append(ServiceNames, UtilityImageNames...)
	urls := make(map[string]pulumi.StringOutput, len(allImages))
	arns := make(map[string]pulumi.StringOutput, len(allImages))

	for _, svc := range allImages {
		repoName := fmt.Sprintf("kaizen-%s", svc)
		resourceName := fmt.Sprintf("ecr-%s", svc)

		repo, err := ecr.NewRepository(ctx, resourceName, &ecr.RepositoryArgs{
			Name:               pulumi.String(repoName),
			ImageTagMutability: pulumi.String("MUTABLE"),
			ImageScanningConfiguration: &ecr.RepositoryImageScanningConfigurationArgs{
				ScanOnPush: pulumi.Bool(true),
			},
			Tags: pulumi.StringMap{
				"Project":   pulumi.String("kaizen"),
				"Service":   pulumi.String(svc),
				"Env":       pulumi.String(env),
				"ManagedBy": pulumi.String("pulumi"),
			},
		})
		if err != nil {
			return nil, fmt.Errorf("creating ECR repo %s: %w", repoName, err)
		}

		_, err = ecr.NewLifecyclePolicy(ctx, fmt.Sprintf("ecr-lifecycle-%s", svc), &ecr.LifecyclePolicyArgs{
			Repository: repo.Name,
			Policy:     pulumi.String(policy),
		})
		if err != nil {
			return nil, fmt.Errorf("creating lifecycle policy for %s: %w", repoName, err)
		}

		urls[svc] = repo.RepositoryUrl
		arns[svc] = repo.Arn
	}

	return &ECROutputs{
		RepositoryURLs: urls,
		RepositoryArns: arns,
	}, nil
}

// buildLifecyclePolicy returns the JSON-encoded ECR lifecycle policy.
func buildLifecyclePolicy() (string, error) {
	p := lifecyclePolicy{
		Rules: []lifecycleRule{
			{
				RulePriority: 1,
				Description:  "Expire untagged images after 7 days",
				Selection: lifecycleSelect{
					TagStatus:   "untagged",
					CountType:   "sinceImagePushed",
					CountUnit:   "days",
					CountNumber: 7,
				},
				Action: lifecycleAction{Type: "expire"},
			},
			{
				RulePriority: 2,
				Description:  "Keep only the last 10 tagged images",
				Selection: lifecycleSelect{
					TagStatus:     "tagged",
					TagPrefixList: []string{"v", "sha-", "latest"},
					CountType:     "imageCountMoreThan",
					CountNumber:   10,
				},
				Action: lifecycleAction{Type: "expire"},
			},
		},
	}

	b, err := json.Marshal(p)
	if err != nil {
		return "", fmt.Errorf("marshaling lifecycle policy: %w", err)
	}
	return string(b), nil
}
