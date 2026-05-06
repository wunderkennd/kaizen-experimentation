// Package network provisions IAM roles for ECS tasks and CI/CD deployment.
//
// Three roles are created:
//   - ECS task role: runtime permissions for running containers
//   - ECS task execution role: agent permissions for ECS infrastructure
//   - CI deploy role: GitHub Actions OIDC-based deployment
package network

import (
	"encoding/json"
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// IAMArgs holds the inputs for creating IAM roles.
type IAMArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string
	// DataBucketArn is the ARN of the Delta Lake S3 bucket.
	DataBucketArn pulumi.StringOutput
	// MlflowBucketArn is the ARN of the MLflow artifact S3 bucket.
	MlflowBucketArn pulumi.StringOutput
}

// IAMOutputs holds the IAM role ARNs consumed by downstream modules (compute, CI).
type IAMOutputs struct {
	TaskRoleArn     pulumi.StringOutput
	ExecRoleArn     pulumi.StringOutput
	CIDeployRoleArn pulumi.StringOutput
}

const ecsTrustPolicy = `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ecs-tasks.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

// NewIAMRoles creates three IAM roles for ECS task runtime, ECS agent
// infrastructure, and GitHub Actions CI/CD deployment.
func NewIAMRoles(ctx *pulumi.Context, args *IAMArgs) (*IAMOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	identity, err := aws.GetCallerIdentity(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("get caller identity: %w", err)
	}

	region, err := aws.GetRegion(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("get region: %w", err)
	}

	// ── ECS Task Role ───────────────────────────────────────────────────
	taskRole, err := newEcsTaskRole(ctx, prefix, args, identity, region)
	if err != nil {
		return nil, err
	}

	// ── ECS Task Execution Role ─────────────────────────────────────────
	execRole, err := newEcsExecRole(ctx, prefix, args, identity, region)
	if err != nil {
		return nil, err
	}

	// ── CI Deploy Role ──────────────────────────────────────────────────
	ciRole, err := newCIDeployRole(ctx, prefix, identity, taskRole, execRole)
	if err != nil {
		return nil, err
	}

	ctx.Export("ecsTaskRoleArn", taskRole.Arn)
	ctx.Export("ecsExecRoleArn", execRole.Arn)
	ctx.Export("ciDeployRoleArn", ciRole.Arn)

	return &IAMOutputs{
		TaskRoleArn:     taskRole.Arn,
		ExecRoleArn:     execRole.Arn,
		CIDeployRoleArn: ciRole.Arn,
	}, nil
}

// ---------------------------------------------------------------------------
// ECS Task Role — runtime permissions for running containers
// ---------------------------------------------------------------------------

// newEcsTaskRole creates the role assumed by running ECS task containers.
// Grants access to Secrets Manager (read secrets), S3 (data + mlflow buckets),
// and X-Ray (distributed tracing).
func newEcsTaskRole(
	ctx *pulumi.Context,
	prefix string,
	args *IAMArgs,
	identity *aws.GetCallerIdentityResult,
	region *aws.GetRegionResult,
) (*iam.Role, error) {
	role, err := iam.NewRole(ctx, "ecs-task-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-ecs-task-role", prefix),
		AssumeRolePolicy: pulumi.String(ecsTrustPolicy),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("ecs-task-role: %w", err)
	}

	// X-Ray distributed tracing (managed policy).
	if _, err = iam.NewRolePolicyAttachment(ctx, "task-xray-policy", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/AWSXRayDaemonWriteAccess"),
	}); err != nil {
		return nil, fmt.Errorf("task-xray-policy: %w", err)
	}

	// Secrets Manager — scoped to kaizen/{env}/* path.
	secretsArn := fmt.Sprintf(
		"arn:aws:secretsmanager:%s:%s:secret:kaizen/%s/*",
		region.Name, identity.AccountId, args.Environment,
	)
	if _, err = iam.NewRolePolicy(ctx, "task-secrets-policy", &iam.RolePolicyArgs{
		Role: role.Name,
		Policy: staticPolicyJSON([]policyStatement{{
			Effect:    "Allow",
			Actions:   []string{"secretsmanager:GetSecretValue"},
			Resources: []string{secretsArn},
		}}),
	}); err != nil {
		return nil, fmt.Errorf("task-secrets-policy: %w", err)
	}

	// S3 — read/write to data and mlflow buckets (dynamic ARNs).
	s3Policy := pulumi.All(args.DataBucketArn, args.MlflowBucketArn).ApplyT(
		func(arns []interface{}) (string, error) {
			dataArn := arns[0].(string)
			mlflowArn := arns[1].(string)
			return marshalPolicy([]policyStatement{
				{
					Effect:  "Allow",
					Actions: []string{"s3:GetObject", "s3:PutObject", "s3:DeleteObject"},
					Resources: []string{
						dataArn + "/*",
						mlflowArn + "/*",
					},
				},
				{
					Effect:    "Allow",
					Actions:   []string{"s3:ListBucket", "s3:GetBucketLocation"},
					Resources: []string{dataArn, mlflowArn},
				},
			})
		},
	).(pulumi.StringOutput)

	if _, err = iam.NewRolePolicy(ctx, "task-s3-policy", &iam.RolePolicyArgs{
		Role:   role.Name,
		Policy: s3Policy,
	}); err != nil {
		return nil, fmt.Errorf("task-s3-policy: %w", err)
	}

	return role, nil
}

// ---------------------------------------------------------------------------
// ECS Task Execution Role — ECS agent infrastructure permissions
// ---------------------------------------------------------------------------

// newEcsExecRole creates the execution role used by the ECS agent to pull
// images from ECR, write CloudWatch Logs, and inject secrets into containers.
func newEcsExecRole(
	ctx *pulumi.Context,
	prefix string,
	args *IAMArgs,
	identity *aws.GetCallerIdentityResult,
	region *aws.GetRegionResult,
) (*iam.Role, error) {
	role, err := iam.NewRole(ctx, "ecs-exec-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-ecs-exec-role", prefix),
		AssumeRolePolicy: pulumi.String(ecsTrustPolicy),
		Tags: pulumi.StringMap{
			"Project":     pulumi.String("kaizen"),
			"Environment": pulumi.String(args.Environment),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("ecs-exec-role: %w", err)
	}

	// Standard ECS execution role policy — ECR pull + CloudWatch Logs write.
	if _, err = iam.NewRolePolicyAttachment(ctx, "exec-ecs-policy", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"),
	}); err != nil {
		return nil, fmt.Errorf("exec-ecs-policy: %w", err)
	}

	// Secrets Manager — for injecting secrets as container environment variables.
	// The managed policy above covers ECR/CW Logs but not Secrets Manager.
	secretsArn := fmt.Sprintf(
		"arn:aws:secretsmanager:%s:%s:secret:kaizen/%s/*",
		region.Name, identity.AccountId, args.Environment,
	)
	if _, err = iam.NewRolePolicy(ctx, "exec-secrets-policy", &iam.RolePolicyArgs{
		Role: role.Name,
		Policy: staticPolicyJSON([]policyStatement{{
			Effect:    "Allow",
			Actions:   []string{"secretsmanager:GetSecretValue"},
			Resources: []string{secretsArn},
		}}),
	}); err != nil {
		return nil, fmt.Errorf("exec-secrets-policy: %w", err)
	}

	return role, nil
}

// ---------------------------------------------------------------------------
// CI Deploy Role — GitHub Actions OIDC-based deployment
// ---------------------------------------------------------------------------

// newCIDeployRole creates a role for GitHub Actions CI/CD deployments.
// Trusts the GitHub Actions OIDC provider scoped to kaizen-experimentation repos.
func newCIDeployRole(
	ctx *pulumi.Context,
	prefix string,
	identity *aws.GetCallerIdentityResult,
	taskRole *iam.Role,
	execRole *iam.Role,
) (*iam.Role, error) {
	// GitHub Actions OIDC provider — AWS validates the certificate chain
	// for well-known providers; the thumbprint is required by the API but
	// functionally a placeholder.
	oidcProvider, err := iam.NewOpenIdConnectProvider(ctx, "github-oidc", &iam.OpenIdConnectProviderArgs{
		Url:             pulumi.String("https://token.actions.githubusercontent.com"),
		ClientIdLists:   pulumi.StringArray{pulumi.String("sts.amazonaws.com")},
		ThumbprintLists: pulumi.StringArray{pulumi.String("6938fd4d98bab03faadb97b34396831e3780aea1")},
		Tags: pulumi.StringMap{
			"Project":   pulumi.String("kaizen"),
			"ManagedBy": pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("github-oidc: %w", err)
	}

	// Trust policy: allow GitHub Actions from kaizen-experimentation repos.
	trustPolicy := oidcProvider.Arn.ApplyT(func(arn string) (string, error) {
		doc := map[string]interface{}{
			"Version": "2012-10-17",
			"Statement": []map[string]interface{}{
				{
					"Effect":    "Allow",
					"Principal": map[string]string{"Federated": arn},
					"Action":    "sts:AssumeRoleWithWebIdentity",
					"Condition": map[string]interface{}{
						"StringEquals": map[string]string{
							"token.actions.githubusercontent.com:aud": "sts.amazonaws.com",
						},
						"StringLike": map[string]string{
							"token.actions.githubusercontent.com:sub": "repo:*kaizen-experimentation*:*",
						},
					},
				},
			},
		}
		b, err := json.Marshal(doc)
		return string(b), err
	}).(pulumi.StringOutput)

	role, err := iam.NewRole(ctx, "ci-deploy-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-ci-deploy-role", prefix),
		AssumeRolePolicy: trustPolicy,
		Tags: pulumi.StringMap{
			"Project":   pulumi.String("kaizen"),
			"ManagedBy": pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("ci-deploy-role: %w", err)
	}

	// ECS deployment permissions.
	if _, err = iam.NewRolePolicy(ctx, "ci-ecs-policy", &iam.RolePolicyArgs{
		Role: role.Name,
		Policy: staticPolicyJSON([]policyStatement{{
			Effect: "Allow",
			Actions: []string{
				"ecs:UpdateService",
				"ecs:DescribeServices",
				"ecs:ListServices",
				"ecs:DescribeClusters",
				"ecs:DescribeTaskDefinition",
				"ecs:RegisterTaskDefinition",
				"ecs:DeregisterTaskDefinition",
				"ecs:ListTasks",
				"ecs:DescribeTasks",
				"ecs:TagResource",
			},
			Resources: []string{"*"},
		}}),
	}); err != nil {
		return nil, fmt.Errorf("ci-ecs-policy: %w", err)
	}

	// ECR push permissions — scoped to kaizen-* repositories.
	ecrRepoArn := fmt.Sprintf("arn:aws:ecr:*:%s:repository/kaizen-*", identity.AccountId)
	if _, err = iam.NewRolePolicy(ctx, "ci-ecr-policy", &iam.RolePolicyArgs{
		Role: role.Name,
		Policy: staticPolicyJSON([]policyStatement{
			{
				Effect:    "Allow",
				Actions:   []string{"ecr:GetAuthorizationToken"},
				Resources: []string{"*"},
			},
			{
				Effect: "Allow",
				Actions: []string{
					"ecr:BatchCheckLayerAvailability",
					"ecr:CompleteLayerUpload",
					"ecr:UploadLayerPart",
					"ecr:InitiateLayerUpload",
					"ecr:PutImage",
					"ecr:BatchGetImage",
					"ecr:GetDownloadUrlForLayer",
				},
				Resources: []string{ecrRepoArn},
			},
		}),
	}); err != nil {
		return nil, fmt.Errorf("ci-ecr-policy: %w", err)
	}

	// PassRole — allow CI to pass task and execution roles to ECS.
	passRolePolicy := pulumi.All(taskRole.Arn, execRole.Arn).ApplyT(
		func(arns []interface{}) (string, error) {
			return marshalPolicy([]policyStatement{{
				Effect:    "Allow",
				Actions:   []string{"iam:PassRole"},
				Resources: []string{arns[0].(string), arns[1].(string)},
			}})
		},
	).(pulumi.StringOutput)

	if _, err = iam.NewRolePolicy(ctx, "ci-passrole-policy", &iam.RolePolicyArgs{
		Role:   role.Name,
		Policy: passRolePolicy,
	}); err != nil {
		return nil, fmt.Errorf("ci-passrole-policy: %w", err)
	}

	// CloudWatch Logs — create log groups for new service deployments.
	if _, err = iam.NewRolePolicy(ctx, "ci-logs-policy", &iam.RolePolicyArgs{
		Role: role.Name,
		Policy: staticPolicyJSON([]policyStatement{{
			Effect:    "Allow",
			Actions:   []string{"logs:CreateLogGroup", "logs:TagResource"},
			Resources: []string{"*"},
		}}),
	}); err != nil {
		return nil, fmt.Errorf("ci-logs-policy: %w", err)
	}

	return role, nil
}

// ---------------------------------------------------------------------------
// Policy helpers
// ---------------------------------------------------------------------------

type policyDocument struct {
	Version   string            `json:"Version"`
	Statement []policyStatement `json:"Statement"`
}

type policyStatement struct {
	Effect    string   `json:"Effect"`
	Actions   []string `json:"Action"`
	Resources []string `json:"Resource"`
}

// staticPolicyJSON serializes statements into a JSON IAM policy string.
// Safe for structs with only string/slice fields (json.Marshal cannot fail).
func staticPolicyJSON(stmts []policyStatement) pulumi.StringInput {
	doc := policyDocument{Version: "2012-10-17", Statement: stmts}
	b, _ := json.Marshal(doc) //nolint:errcheck // struct with basic types only
	return pulumi.String(string(b))
}

// marshalPolicy serializes statements into a JSON IAM policy string with error
// propagation, for use in ApplyT callbacks.
func marshalPolicy(stmts []policyStatement) (string, error) {
	doc := policyDocument{Version: "2012-10-17", Statement: stmts}
	b, err := json.Marshal(doc)
	return string(b), err
}
