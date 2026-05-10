// Package observability provisions Amazon Managed Prometheus (AMP) and Amazon
// Managed Grafana (AMG) workspaces for the Kaizen experimentation platform.
//
// Owner: Infra-5 (task I.1.10)
//
// Resources created:
//   - AMP workspace with remote-write endpoint
//   - AMG workspace with AMP as a data source
//   - IAM role for ECS tasks to remote-write to AMP
//   - IAM execution role for AMG to query AMP
//   - Prometheus scrape configuration (SSM parameter) for ECS service discovery
//
// Outputs consumed by:
//   - pkg/compute (ECS task role gets remote-write permissions)
//   - main.go (Pulumi stack exports)
package observability

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/amp"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/grafana"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ssm"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// Args holds all inputs required by the observability module.
type Args struct {
	// Environment is the deployment environment (dev, staging, prod).
	Environment string
	// EcsClusterName is used to scope the Prometheus scrape configuration.
	EcsClusterName pulumi.StringOutput
	// Tags are the default resource tags.
	Tags pulumi.StringMap
}

// Outputs holds all resources exported by the observability module.
type Outputs struct {
	// AmpWorkspaceId is the AMP workspace ID.
	AmpWorkspaceId pulumi.IDOutput
	// AmpRemoteWriteEndpoint is the full remote-write URL for the AMP workspace.
	AmpRemoteWriteEndpoint pulumi.StringOutput
	// AmpQueryEndpoint is the query endpoint for the AMP workspace.
	AmpQueryEndpoint pulumi.StringOutput
	// AmgWorkspaceId is the AMG workspace ID.
	AmgWorkspaceId pulumi.IDOutput
	// AmgEndpoint is the AMG workspace URL.
	AmgEndpoint pulumi.StringOutput
	// EcsRemoteWriteRoleArn is the IAM role ARN for ECS tasks to remote-write to AMP.
	EcsRemoteWriteRoleArn pulumi.StringOutput
}

// New creates the AMP and AMG workspaces along with supporting IAM roles and
// Prometheus scrape configuration for ECS service discovery.
func New(ctx *pulumi.Context, args *Args) (*Outputs, error) {
	tags := args.Tags
	if tags == nil {
		tags = config.DefaultTags(args.Environment)
	}

	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	// ── 1. AMP Workspace ────────────────────────────────────────────────
	ampWorkspace, err := amp.NewWorkspace(ctx, "kaizen-amp", &amp.WorkspaceArgs{
		Alias: pulumi.Sprintf("%s-metrics", prefix),
		Tags: config.MergeTags(tags, pulumi.StringMap{
			"Component": pulumi.String("observability"),
			"Service":   pulumi.String("amp"),
		}),
	})
	if err != nil {
		return nil, fmt.Errorf("creating AMP workspace: %w", err)
	}

	// Pre-compute endpoint URLs once to avoid duplicate ApplyT graph nodes.
	remoteWriteEp := ampWorkspace.PrometheusEndpoint.ApplyT(func(ep string) string {
		return ep + "api/v1/remote_write"
	}).(pulumi.StringOutput)
	queryEp := ampWorkspace.PrometheusEndpoint.ApplyT(func(ep string) string {
		return ep + "api/v1/query"
	}).(pulumi.StringOutput)

	// ── 2. IAM Role: ECS tasks → AMP remote-write ──────────────────────
	remoteWriteRole, err := newRemoteWriteRole(ctx, prefix, ampWorkspace, tags)
	if err != nil {
		return nil, fmt.Errorf("remote-write role: %w", err)
	}

	// ── 3. AMG Workspace ────────────────────────────────────────────────
	amgOutputs, err := newAmgWorkspace(ctx, prefix, ampWorkspace, tags)
	if err != nil {
		return nil, fmt.Errorf("AMG workspace: %w", err)
	}

	// ── 4. Prometheus scrape configuration (SSM Parameter) ──────────────
	err = newPrometheusScrapeConfig(ctx, args.Environment, ampWorkspace, args.EcsClusterName, tags)
	if err != nil {
		return nil, fmt.Errorf("prometheus scrape config: %w", err)
	}

	// ── Exports ─────────────────────────────────────────────────────────
	ctx.Export("ampWorkspaceId", ampWorkspace.ID())
	ctx.Export("ampRemoteWriteEndpoint", remoteWriteEp)
	ctx.Export("amgWorkspaceId", amgOutputs.workspaceId)
	ctx.Export("amgEndpoint", amgOutputs.endpoint)

	return &Outputs{
		AmpWorkspaceId:         ampWorkspace.ID(),
		AmpRemoteWriteEndpoint: remoteWriteEp,
		AmpQueryEndpoint:       queryEp,
		AmgWorkspaceId:         amgOutputs.workspaceId,
		AmgEndpoint:            amgOutputs.endpoint,
		EcsRemoteWriteRoleArn:  remoteWriteRole.Arn,
	}, nil
}

// newRemoteWriteRole creates an IAM role that ECS tasks can assume to write
// metrics to the AMP workspace via Prometheus remote-write.
func newRemoteWriteRole(
	ctx *pulumi.Context,
	prefix string,
	ampWorkspace *amp.Workspace,
	tags pulumi.StringMap,
) (*iam.Role, error) {
	assumeRolePolicy := `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ecs-tasks.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

	role, err := iam.NewRole(ctx, "amp-remote-write-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-amp-remote-write", prefix),
		AssumeRolePolicy: pulumi.String(assumeRolePolicy),
		Tags:             tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating AMP remote-write role: %w", err)
	}

	// Inline policy: only aps:RemoteWrite, scoped to this AMP workspace.
	_, err = iam.NewRolePolicy(ctx, "amp-remote-write-policy", &iam.RolePolicyArgs{
		Role: role.Name,
		Policy: ampWorkspace.Arn.ApplyT(func(arn string) string {
			return fmt.Sprintf(`{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["aps:RemoteWrite"],
    "Resource": "%s"
  }]
}`, arn)
		}).(pulumi.StringOutput),
	})
	if err != nil {
		return nil, fmt.Errorf("creating AMP remote-write policy: %w", err)
	}

	return role, nil
}

// amgResult holds the AMG workspace outputs from newAmgWorkspace.
type amgResult struct {
	workspaceId pulumi.IDOutput
	endpoint    pulumi.StringOutput
}

// newAmgWorkspace creates an Amazon Managed Grafana workspace and configures
// AMP as its data source.
func newAmgWorkspace(
	ctx *pulumi.Context,
	prefix string,
	ampWorkspace *amp.Workspace,
	tags pulumi.StringMap,
) (*amgResult, error) {
	// IAM execution role for AMG to query AMP.
	amgAssumeRolePolicy := `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "grafana.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

	amgRole, err := iam.NewRole(ctx, "amg-execution-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-amg-execution", prefix),
		AssumeRolePolicy: pulumi.String(amgAssumeRolePolicy),
		Tags:             tags,
	})
	if err != nil {
		return nil, fmt.Errorf("creating AMG execution role: %w", err)
	}

	// Allow AMG to query AMP metrics. ListWorkspaces is account-wide and
	// requires Resource: "*"; workspace-scoped actions are restricted to the
	// specific AMP workspace ARN.
	_, err = iam.NewRolePolicy(ctx, "amg-amp-query-policy", &iam.RolePolicyArgs{
		Role: amgRole.Name,
		Policy: ampWorkspace.Arn.ApplyT(func(arn string) string {
			return fmt.Sprintf(`{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "aps:QueryMetrics",
        "aps:GetSeries",
        "aps:GetLabels",
        "aps:GetMetricMetadata",
        "aps:DescribeWorkspace"
      ],
      "Resource": "%s"
    },
    {
      "Effect": "Allow",
      "Action": ["aps:ListWorkspaces"],
      "Resource": "*"
    }
  ]
}`, arn)
		}).(pulumi.StringOutput),
	})
	if err != nil {
		return nil, fmt.Errorf("creating AMG AMP query policy: %w", err)
	}

	// AMG workspace.
	workspace, err := grafana.NewWorkspace(ctx, "kaizen-amg", &grafana.WorkspaceArgs{
		Name:                  pulumi.Sprintf("%s-grafana", prefix),
		AccountAccessType:     pulumi.String("CURRENT_ACCOUNT"),
		AuthenticationProviders: pulumi.StringArray{pulumi.String("AWS_SSO")},
		PermissionType:        pulumi.String("SERVICE_MANAGED"),
		RoleArn:               amgRole.Arn,
		DataSources:           pulumi.StringArray{pulumi.String("PROMETHEUS")},
		Tags: config.MergeTags(tags, pulumi.StringMap{
			"Component": pulumi.String("observability"),
			"Service":   pulumi.String("amg"),
		}),
	})
	if err != nil {
		return nil, fmt.Errorf("creating AMG workspace: %w", err)
	}

	return &amgResult{
		workspaceId: workspace.ID(),
		endpoint:    workspace.Endpoint,
	}, nil
}

// newPrometheusScrapeConfig stores a Prometheus scrape configuration as an SSM
// parameter. The ADOT collector sidecar in ECS tasks reads this configuration
// to discover and scrape Kaizen services via Cloud Map service discovery.
func newPrometheusScrapeConfig(
	ctx *pulumi.Context,
	environment string,
	ampWorkspace *amp.Workspace,
	ecsClusterName pulumi.StringOutput,
	tags pulumi.StringMap,
) error {
	// The scrape config uses ECS service discovery to find Kaizen services
	// in the Cloud Map namespace. Each service exposes /metrics on its
	// respective port (50051-50058 for gRPC services, 3000 for UI).
	//
	// Region is derived from the Pulumi stack config (aws:region) at deploy
	// time; this config is stored as an SSM parameter and read by the ADOT
	// collector sidecar, which inherits the task's region.
	scrapeConfig := ecsClusterName.ApplyT(func(cluster string) string {
		return fmt.Sprintf(`global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  # ── Kaizen gRPC services (M1-M5, M7) ──────────────────────────────
  - job_name: kaizen-grpc-services
    metrics_path: /metrics
    ec2_sd_configs:
      - port: 9090
        filters:
          - name: tag:Project
            values: [kaizen]
          - name: tag:AmazonECSManaged
            values: ["true"]
    relabel_configs:
      # Use the ECS cluster name to filter.
      - source_labels: [__meta_ec2_tag_aws_ecs_cluster_name]
        regex: "%s"
        action: keep
      # Set the instance label to the service name tag.
      - source_labels: [__meta_ec2_tag_Service]
        target_label: service
      # Set the environment label.
      - source_labels: [__meta_ec2_tag_Environment]
        target_label: environment

  # ── ECS task-level metrics via ADOT ────────────────────────────────
  - job_name: kaizen-ecs-tasks
    metrics_path: /metrics
    dns_sd_configs:
      - names:
          - assignment.kaizen.internal
          - pipeline.kaizen.internal
          - pipeline-orch.kaizen.internal
          - metrics.kaizen.internal
          - analysis.kaizen.internal
          - bandit.kaizen.internal
          - management.kaizen.internal
          - flags.kaizen.internal
        type: A
        port: 9090
        refresh_interval: 30s
    relabel_configs:
      - source_labels: [__address__]
        regex: "(.+)\\.kaizen\\.internal:\\d+"
        target_label: service
        replacement: "${1}"
`, cluster)
	}).(pulumi.StringOutput)

	_, err := ssm.NewParameter(ctx, "prometheus-scrape-config", &ssm.ParameterArgs{
		Name:  pulumi.Sprintf("/kaizen/%s/prometheus/scrape-config", environment),
		Type:  pulumi.String("String"),
		Value: scrapeConfig,
		Tier:  pulumi.String("Advanced"), // Advanced tier for configs > 4KB.
		Tags: config.MergeTags(tags, pulumi.StringMap{
			"Component": pulumi.String("observability"),
		}),
	})
	if err != nil {
		return fmt.Errorf("creating Prometheus scrape config SSM parameter: %w", err)
	}

	return nil
}
