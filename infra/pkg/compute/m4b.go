// Package compute — m4b.go provides the operational resources for the M4b
// Policy service: AWS Backup for EBS snapshots, CloudWatch status-check
// alarms, and Cloud Map service discovery.
//
// The core EC2 infrastructure (launch template, ASG, instance profile) lives
// in cluster.go.  This file layers on the resources that keep M4b observable,
// recoverable, and discoverable inside the VPC.
package compute

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/backup"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/cloudwatch"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/servicediscovery"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// M4bServiceArgs configures the operational resources for the M4b Policy service.
type M4bServiceArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string

	// CloudMapNamespaceId is the Cloud Map private DNS namespace ID
	// (kaizen.local) created by the network module.
	CloudMapNamespaceId pulumi.IDOutput

	// AsgName is the Auto Scaling Group name for M4b, used as the alarm
	// dimension. Passed from ClusterOutputs.
	AsgName pulumi.StringOutput
}

// M4bServiceOutputs holds the outputs from M4b operational resources.
type M4bServiceOutputs struct {
	// BackupVaultArn is the ARN of the AWS Backup vault for M4b EBS snapshots.
	BackupVaultArn pulumi.StringOutput
	// BackupPlanArn is the ARN of the backup plan.
	BackupPlanArn pulumi.StringOutput
	// CloudMapServiceArn is the ARN of the Cloud Map service registration.
	CloudMapServiceArn pulumi.StringOutput
	// StatusCheckAlarmArn is the ARN of the CloudWatch status-check alarm.
	StatusCheckAlarmArn pulumi.StringOutput
}

// NewM4bService creates the operational resources for the M4b Policy service:
//   - AWS Backup vault + plan + selection for daily EBS snapshots (7-day retention)
//   - CloudWatch alarm on StatusCheckFailed with auto-recover action
//   - Cloud Map service discovery (m4b-policy.kaizen.local:50054)
func NewM4bService(ctx *pulumi.Context, args *M4bServiceArgs) (*M4bServiceOutputs, error) {
	prefix := fmt.Sprintf("kaizen-%s", args.Environment)
	tags := pulumi.StringMap{
		"Environment": pulumi.String(args.Environment),
		"Project":     pulumi.String("kaizen"),
		"Service":     pulumi.String("m4b-policy"),
		"ManagedBy":   pulumi.String("pulumi"),
	}

	// ── AWS Backup: daily EBS snapshots, 7-day retention ──────────────────

	vaultArn, planArn, err := newM4bBackup(ctx, prefix, tags)
	if err != nil {
		return nil, err
	}

	// ── CloudWatch alarm: StatusCheckFailed → auto-recover ────────────────

	alarmArn, err := newM4bStatusCheckAlarm(ctx, prefix, args, tags)
	if err != nil {
		return nil, err
	}

	// ── Cloud Map: m4b-policy.kaizen.local:50054 ──────────────────────────

	serviceArn, err := newM4bCloudMapService(ctx, prefix, args, tags)
	if err != nil {
		return nil, err
	}

	return &M4bServiceOutputs{
		BackupVaultArn:      vaultArn,
		BackupPlanArn:       planArn,
		CloudMapServiceArn:  serviceArn,
		StatusCheckAlarmArn: alarmArn,
	}, nil
}

// newM4bBackup creates an AWS Backup vault, plan, and selection for M4b EBS
// volumes. Schedule: daily at 03:00 UTC.  Retention: 7 days.
func newM4bBackup(
	ctx *pulumi.Context,
	prefix string,
	tags pulumi.StringMap,
) (vaultArn pulumi.StringOutput, planArn pulumi.StringOutput, err error) {
	vault, err := backup.NewVault(ctx, "m4b-backup-vault", &backup.VaultArgs{
		Name: pulumi.Sprintf("%s-m4b-ebs", prefix),
		Tags: tags,
	})
	if err != nil {
		return pulumi.StringOutput{}, pulumi.StringOutput{}, fmt.Errorf("creating M4b backup vault: %w", err)
	}

	plan, err := backup.NewPlan(ctx, "m4b-backup-plan", &backup.PlanArgs{
		Name: pulumi.Sprintf("%s-m4b-daily", prefix),
		Rules: backup.PlanRuleArray{
			&backup.PlanRuleArgs{
				RuleName:        pulumi.String("m4b-ebs-daily"),
				TargetVaultName: vault.Name,
				Schedule:        pulumi.String("cron(0 3 * * ? *)"),
				StartWindow:     pulumi.Int(60),      // 1 hour start window
				CompletionWindow: pulumi.Int(180),     // 3 hour completion window
				Lifecycle: &backup.PlanRuleLifecycleArgs{
					DeleteAfter: pulumi.Int(7), // 7-day retention
				},
			},
		},
		Tags: tags,
	})
	if err != nil {
		return pulumi.StringOutput{}, pulumi.StringOutput{}, fmt.Errorf("creating M4b backup plan: %w", err)
	}

	// IAM role for AWS Backup to take EBS snapshots.
	backupRole, err := newM4bBackupRole(ctx, prefix)
	if err != nil {
		return pulumi.StringOutput{}, pulumi.StringOutput{}, err
	}

	// Select M4b EBS volumes by tag.
	_, err = backup.NewSelection(ctx, "m4b-backup-selection", &backup.SelectionArgs{
		Name:      pulumi.Sprintf("%s-m4b-ebs-selection", prefix),
		PlanId:    plan.ID(),
		IamRoleArn: backupRole.Arn,
		SelectionTags: backup.SelectionSelectionTagArray{
			&backup.SelectionSelectionTagArgs{
				Type:  pulumi.String("STRINGEQUALS"),
				Key:   pulumi.String("Service"),
				Value: pulumi.String("m4b-policy"),
			},
		},
	})
	if err != nil {
		return pulumi.StringOutput{}, pulumi.StringOutput{}, fmt.Errorf("creating M4b backup selection: %w", err)
	}

	return vault.Arn, plan.Arn, nil
}

// newM4bBackupRole creates an IAM role that AWS Backup assumes to take EBS
// snapshots of M4b volumes.
func newM4bBackupRole(ctx *pulumi.Context, prefix string) (*iam.Role, error) {
	assumeRolePolicy := `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "backup.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

	role, err := iam.NewRole(ctx, "m4b-backup-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-m4b-backup-role", prefix),
		AssumeRolePolicy: pulumi.String(assumeRolePolicy),
		Tags: pulumi.StringMap{
			"Project": pulumi.String("kaizen"),
			"Service": pulumi.String("m4b-policy"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b backup role: %w", err)
	}

	_, err = iam.NewRolePolicyAttachment(ctx, "m4b-backup-policy", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/service-role/AWSBackupServiceRolePolicyForBackup"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching backup policy: %w", err)
	}

	_, err = iam.NewRolePolicyAttachment(ctx, "m4b-backup-restore-policy", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/service-role/AWSBackupServiceRolePolicyForRestores"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching backup restore policy: %w", err)
	}

	return role, nil
}

// newM4bStatusCheckAlarm creates a CloudWatch alarm that fires when the M4b
// ASG has zero in-service instances — indicating the single M4b instance has
// failed its status checks.  The ASG (min=max=1) will automatically replace
// the instance; this alarm provides observability and triggers notifications.
//
// EC2 auto-recovery for system-level failures is enabled in the launch
// template via MaintenanceOptions (see cluster.go).
func newM4bStatusCheckAlarm(
	ctx *pulumi.Context,
	prefix string,
	args *M4bServiceArgs,
	tags pulumi.StringMap,
) (pulumi.StringOutput, error) {
	alarm, err := cloudwatch.NewMetricAlarm(ctx, "m4b-status-check-alarm", &cloudwatch.MetricAlarmArgs{
		Name:             pulumi.Sprintf("%s-m4b-status-check-failed", prefix),
		AlarmDescription: pulumi.String("M4b Policy service instance failed status checks. ASG will auto-replace; auto-recovery handles system failures."),
		ComparisonOperator: pulumi.String("LessThanThreshold"),
		EvaluationPeriods:  pulumi.Int(2),
		MetricName:         pulumi.String("GroupInServiceInstances"),
		Namespace:          pulumi.String("AWS/AutoScaling"),
		Period:             pulumi.Int(60),
		Statistic:          pulumi.String("Minimum"),
		Threshold:          pulumi.Float64(1),
		DatapointsToAlarm:  pulumi.Int(2),
		TreatMissingData:   pulumi.String("missing"),
		Dimensions: pulumi.StringMap{
			"AutoScalingGroupName": args.AsgName,
		},
		Tags: tags,
	})
	if err != nil {
		return pulumi.StringOutput{}, fmt.Errorf("creating M4b status check alarm: %w", err)
	}

	return alarm.Arn, nil
}

// newM4bCloudMapService registers the M4b Policy service in Cloud Map so
// other services can discover it via DNS: m4b-policy.kaizen.local:50054.
//
// The ECS task (configured in Sprint I.2) will register instance IPs against
// this service using the SRV + A record type.
func newM4bCloudMapService(
	ctx *pulumi.Context,
	prefix string,
	args *M4bServiceArgs,
	tags pulumi.StringMap,
) (pulumi.StringOutput, error) {
	svc, err := servicediscovery.NewService(ctx, "m4b-cloud-map-service", &servicediscovery.ServiceArgs{
		Name:        pulumi.String("m4b-policy"),
		Description: pulumi.String("M4b Policy service — LMAX bandit evaluation (port 50054)"),
		DnsConfig: &servicediscovery.ServiceDnsConfigArgs{
			NamespaceId:   args.CloudMapNamespaceId.ToStringOutput(),
			RoutingPolicy: pulumi.String("WEIGHTED"),
			DnsRecords: servicediscovery.ServiceDnsConfigDnsRecordArray{
				&servicediscovery.ServiceDnsConfigDnsRecordArgs{
					Type: pulumi.String("A"),
					Ttl:  pulumi.Int(10),
				},
				&servicediscovery.ServiceDnsConfigDnsRecordArgs{
					Type: pulumi.String("SRV"),
					Ttl:  pulumi.Int(10),
				},
			},
		},
		HealthCheckCustomConfig: &servicediscovery.ServiceHealthCheckCustomConfigArgs{
			FailureThreshold: pulumi.Int(1),
		},
		Tags: tags,
	})
	if err != nil {
		return pulumi.StringOutput{}, fmt.Errorf("creating M4b Cloud Map service: %w", err)
	}

	return svc.Arn, nil
}
