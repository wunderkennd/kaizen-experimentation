// Package compute provisions the ECS cluster and capacity providers
// for the Kaizen experimentation platform.
//
// Sprint I.0 scope: cluster + Fargate + EC2 capacity providers (code only).
// Sprint I.1 adds service definitions, M4b wiring, and autoscaling.
package compute

import (
	"encoding/base64"
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/autoscaling"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ec2"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ecs"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/iam"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/ssm"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// ClusterArgs configures the ECS cluster and its capacity providers.
type ClusterArgs struct {
	// Environment name: "dev", "staging", or "prod".
	Environment string

	// M4bInstanceType is the EC2 instance type for the M4b bandit service.
	// Recommended: "t3.large" (dev), "c6i.xlarge" (staging/prod).
	M4bInstanceType string

	// M4bEbsSizeGb is the gp3 EBS volume size for M4b RocksDB data.
	// Default: 20.
	M4bEbsSizeGb int

	// PrivateSubnetIds for M4b ASG placement (wired in Sprint I.1).
	PrivateSubnetIds pulumi.StringArrayOutput

	// M4bSecurityGroupId for the M4b EC2 instance (wired in Sprint I.1).
	M4bSecurityGroupId pulumi.IDOutput
}

// ClusterOutputs holds the ECS cluster outputs consumed by downstream modules.
type ClusterOutputs struct {
	// ClusterId is the ECS cluster resource ID.
	ClusterId pulumi.IDOutput
	// ClusterArn is the full ARN of the ECS cluster.
	ClusterArn pulumi.StringOutput
	// ClusterName is the ECS cluster name.
	ClusterName pulumi.StringOutput
	// M4bCapacityProvider is the name of the EC2 capacity provider for M4b.
	M4bCapacityProvider pulumi.StringOutput
	// M4bAsgName is the ASG name, consumed by M4b operational resources (alarms).
	M4bAsgName pulumi.StringOutput
}

// NewCluster creates an ECS cluster with Container Insights, Fargate capacity
// providers (default), and an EC2 capacity provider backed by an ASG for the
// M4b bandit policy service.
func NewCluster(ctx *pulumi.Context, args *ClusterArgs) (*ClusterOutputs, error) {
	if args.M4bEbsSizeGb == 0 {
		args.M4bEbsSizeGb = 20
	}
	if args.M4bInstanceType == "" {
		if args.Environment == "dev" {
			args.M4bInstanceType = "t3.large"
		} else {
			args.M4bInstanceType = "c6i.xlarge"
		}
	}

	prefix := fmt.Sprintf("kaizen-%s", args.Environment)

	// --- ECS Cluster ---

	cluster, err := ecs.NewCluster(ctx, "kaizen-cluster", &ecs.ClusterArgs{
		Name: pulumi.Sprintf("%s-cluster", prefix),
		Settings: ecs.ClusterSettingArray{
			&ecs.ClusterSettingArgs{
				Name:  pulumi.String("containerInsights"),
				Value: pulumi.String("enabled"),
			},
		},
		Tags: pulumi.StringMap{
			"Environment": pulumi.String(args.Environment),
			"Project":     pulumi.String("kaizen"),
			"ManagedBy":   pulumi.String("pulumi"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating ECS cluster: %w", err)
	}

	// --- M4b EC2 Capacity Provider ---

	instanceProfile, err := newM4bInstanceProfile(ctx, prefix)
	if err != nil {
		return nil, err
	}

	lt, err := newM4bLaunchTemplate(ctx, prefix, args, cluster, instanceProfile)
	if err != nil {
		return nil, err
	}

	asg, err := newM4bASG(ctx, prefix, args, lt)
	if err != nil {
		return nil, err
	}

	m4bProvider, err := ecs.NewCapacityProvider(ctx, "m4b-capacity-provider", &ecs.CapacityProviderArgs{
		Name: pulumi.Sprintf("%s-m4b-ec2", prefix),
		AutoScalingGroupProvider: &ecs.CapacityProviderAutoScalingGroupProviderArgs{
			AutoScalingGroupArn: asg.Arn,
			ManagedScaling: &ecs.CapacityProviderAutoScalingGroupProviderManagedScalingArgs{
				Status:                pulumi.String("ENABLED"),
				TargetCapacity:        pulumi.Int(100),
				MinimumScalingStepSize: pulumi.Int(1),
				MaximumScalingStepSize: pulumi.Int(1),
			},
			ManagedTerminationProtection: pulumi.String("ENABLED"),
		},
		Tags: pulumi.StringMap{
			"Environment": pulumi.String(args.Environment),
			"Project":     pulumi.String("kaizen"),
			"Service":     pulumi.String("m4b-policy"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b capacity provider: %w", err)
	}

	// --- Associate Capacity Providers with Cluster ---

	_, err = ecs.NewClusterCapacityProviders(ctx, "kaizen-capacity-providers", &ecs.ClusterCapacityProvidersArgs{
		ClusterName: cluster.Name,
		CapacityProviders: pulumi.StringArray{
			pulumi.String("FARGATE"),
			pulumi.String("FARGATE_SPOT"),
			m4bProvider.Name,
		},
		DefaultCapacityProviderStrategies: ecs.ClusterCapacityProvidersDefaultCapacityProviderStrategyArray{
			&ecs.ClusterCapacityProvidersDefaultCapacityProviderStrategyArgs{
				CapacityProvider: pulumi.String("FARGATE"),
				Weight:           pulumi.Int(1),
				Base:             pulumi.Int(1),
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("associating capacity providers: %w", err)
	}

	// --- Exports ---

	ctx.Export("clusterId", cluster.ID())
	ctx.Export("clusterArn", cluster.Arn)
	ctx.Export("clusterName", cluster.Name)

	return &ClusterOutputs{
		ClusterId:           cluster.ID(),
		ClusterArn:          cluster.Arn,
		ClusterName:         cluster.Name,
		M4bCapacityProvider: m4bProvider.Name,
		M4bAsgName:          asg.Name,
	}, nil
}

// newM4bInstanceProfile creates the IAM instance profile that allows EC2
// instances to register with ECS and pull images from ECR.
func newM4bInstanceProfile(ctx *pulumi.Context, prefix string) (*iam.InstanceProfile, error) {
	assumeRolePolicy := `{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": {"Service": "ec2.amazonaws.com"},
    "Action": "sts:AssumeRole"
  }]
}`

	role, err := iam.NewRole(ctx, "m4b-ec2-role", &iam.RoleArgs{
		Name:             pulumi.Sprintf("%s-m4b-ec2-role", prefix),
		AssumeRolePolicy: pulumi.String(assumeRolePolicy),
		Tags: pulumi.StringMap{
			"Project": pulumi.String("kaizen"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b EC2 role: %w", err)
	}

	// AmazonEC2ContainerServiceforEC2Role grants ECS agent permissions.
	_, err = iam.NewRolePolicyAttachment(ctx, "m4b-ecs-policy", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/service-role/AmazonEC2ContainerServiceforEC2Role"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching ECS policy: %w", err)
	}

	// SSM managed instance policy for Session Manager access (ops debugging).
	_, err = iam.NewRolePolicyAttachment(ctx, "m4b-ssm-policy", &iam.RolePolicyAttachmentArgs{
		Role:      role.Name,
		PolicyArn: pulumi.String("arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"),
	})
	if err != nil {
		return nil, fmt.Errorf("attaching SSM policy: %w", err)
	}

	profile, err := iam.NewInstanceProfile(ctx, "m4b-instance-profile", &iam.InstanceProfileArgs{
		Name: pulumi.Sprintf("%s-m4b-profile", prefix),
		Role: role.Name,
	})
	if err != nil {
		return nil, fmt.Errorf("creating instance profile: %w", err)
	}

	return profile, nil
}

// newM4bLaunchTemplate creates the EC2 launch template for M4b instances.
// Uses ECS-optimized AL2023 AMI with user data that configures the ECS agent
// and mounts an EBS gp3 volume at /data/rocksdb.
func newM4bLaunchTemplate(
	ctx *pulumi.Context,
	prefix string,
	args *ClusterArgs,
	cluster *ecs.Cluster,
	instanceProfile *iam.InstanceProfile,
) (*ec2.LaunchTemplate, error) {
	// Resolve latest ECS-optimized AMI from SSM parameter store.
	ecsAmi, err := ssm.LookupParameter(ctx, &ssm.LookupParameterArgs{
		Name: "/aws/service/ecs/optimized-ami/amazon-linux-2023/recommended/image_id",
	})
	if err != nil {
		return nil, fmt.Errorf("looking up ECS AMI: %w", err)
	}

	// Build user data from cluster name output.
	userData := cluster.Name.ApplyT(func(clusterName string) string {
		script := fmt.Sprintf(`#!/bin/bash
set -euo pipefail

# Configure ECS agent to join the cluster.
cat <<ECSCONFIG >> /etc/ecs/ecs.config
ECS_CLUSTER=%s
ECS_ENABLE_CONTAINER_METADATA=true
ECS_CONTAINER_INSTANCE_PROPAGATE_TAGS_FROM=ec2_instance
ECSCONFIG

# Format and mount the EBS data volume for RocksDB.
# Wait for the device to become available.
while [ ! -b /dev/xvdf ]; do sleep 1; done
mkfs.xfs /dev/xvdf
mkdir -p /data/rocksdb
mount /dev/xvdf /data/rocksdb
echo "/dev/xvdf /data/rocksdb xfs defaults,nofail 0 2" >> /etc/fstab

# Set ownership for the ECS task user.
chown -R 1000:1000 /data/rocksdb
`, clusterName)
		return base64.StdEncoding.EncodeToString([]byte(script))
	}).(pulumi.StringOutput)

	lt, err := ec2.NewLaunchTemplate(ctx, "m4b-launch-template", &ec2.LaunchTemplateArgs{
		Name:        pulumi.Sprintf("%s-m4b-lt", prefix),
		ImageId:     pulumi.String(ecsAmi.Value),
		InstanceType: pulumi.String(args.M4bInstanceType),
		UserData:    userData,

		IamInstanceProfile: &ec2.LaunchTemplateIamInstanceProfileArgs{
			Arn: instanceProfile.Arn,
		},

		// EBS data volume for RocksDB (attached as /dev/xvdf).
		// gp3 baseline is 3000 IOPS / 125 MB/s — explicit for auditability.
		BlockDeviceMappings: ec2.LaunchTemplateBlockDeviceMappingArray{
			&ec2.LaunchTemplateBlockDeviceMappingArgs{
				DeviceName: pulumi.String("/dev/xvdf"),
				Ebs: &ec2.LaunchTemplateBlockDeviceMappingEbsArgs{
					VolumeSize:          pulumi.Int(args.M4bEbsSizeGb),
					VolumeType:          pulumi.String("gp3"),
					Iops:                pulumi.Int(3000),
					Throughput:          pulumi.Int(125),
					Encrypted:           pulumi.String("true"),
					DeleteOnTermination: pulumi.String("true"),
				},
			},
		},

		// EC2 auto-recovery: migrate to new hardware on system failures,
		// preserving EBS volumes, private IP, and instance ID.
		MaintenanceOptions: &ec2.LaunchTemplateMaintenanceOptionsArgs{
			AutoRecovery: pulumi.String("default"),
		},

		// Metadata options: IMDSv2 required for security.
		MetadataOptions: &ec2.LaunchTemplateMetadataOptionsArgs{
			HttpEndpoint:            pulumi.String("enabled"),
			HttpTokens:              pulumi.String("required"),
			HttpPutResponseHopLimit: pulumi.Int(2),
		},

		Monitoring: &ec2.LaunchTemplateMonitoringArgs{
			Enabled: pulumi.Bool(true),
		},

		TagSpecifications: ec2.LaunchTemplateTagSpecificationArray{
			&ec2.LaunchTemplateTagSpecificationArgs{
				ResourceType: pulumi.String("instance"),
				Tags: pulumi.StringMap{
					"Name":        pulumi.Sprintf("%s-m4b-policy", prefix),
					"Environment": pulumi.String(args.Environment),
					"Project":     pulumi.String("kaizen"),
					"Service":     pulumi.String("m4b-policy"),
				},
			},
			&ec2.LaunchTemplateTagSpecificationArgs{
				ResourceType: pulumi.String("volume"),
				Tags: pulumi.StringMap{
					"Name":        pulumi.Sprintf("%s-m4b-data", prefix),
					"Environment": pulumi.String(args.Environment),
					"Project":     pulumi.String("kaizen"),
				},
			},
		},

		// Security group wired via ClusterArgs.M4bSecurityGroupId (Sprint I.1).
		VpcSecurityGroupIds: pulumi.StringArray{
			args.M4bSecurityGroupId.ToStringOutput(),
		},

		Tags: pulumi.StringMap{
			"Environment": pulumi.String(args.Environment),
			"Project":     pulumi.String("kaizen"),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b launch template: %w", err)
	}

	return lt, nil
}

// newM4bASG creates the Auto Scaling Group for M4b EC2 instances.
// Configured for a single instance (min=max=desired=1) since M4b runs a
// single-threaded LMAX core that does not benefit from horizontal scaling.
func newM4bASG(
	ctx *pulumi.Context,
	prefix string,
	args *ClusterArgs,
	lt *ec2.LaunchTemplate,
) (*autoscaling.Group, error) {
	asg, err := autoscaling.NewGroup(ctx, "m4b-asg", &autoscaling.GroupArgs{
		Name:            pulumi.Sprintf("%s-m4b-asg", prefix),
		MaxSize:         pulumi.Int(1),
		MinSize:         pulumi.Int(1),
		DesiredCapacity: pulumi.Int(1),

		LaunchTemplate: &autoscaling.GroupLaunchTemplateArgs{
			Id:      lt.ID(),
			Version: pulumi.String("$Latest"),
		},

		VpcZoneIdentifiers: args.PrivateSubnetIds,

		// Protect instances from scale-in; ECS managed termination handles this.
		ProtectFromScaleIn: pulumi.Bool(true),

		// Health check uses EC2 status checks (ECS agent health is separate).
		HealthCheckType:        pulumi.String("EC2"),
		HealthCheckGracePeriod: pulumi.Int(300),

		Tags: autoscaling.GroupTagArray{
			&autoscaling.GroupTagArgs{
				Key:               pulumi.String("Name"),
				Value:             pulumi.Sprintf("%s-m4b-policy", prefix),
				PropagateAtLaunch: pulumi.Bool(true),
			},
			&autoscaling.GroupTagArgs{
				Key:               pulumi.String("Environment"),
				Value:             pulumi.String(args.Environment),
				PropagateAtLaunch: pulumi.Bool(true),
			},
			&autoscaling.GroupTagArgs{
				Key:               pulumi.String("Project"),
				Value:             pulumi.String("kaizen"),
				PropagateAtLaunch: pulumi.Bool(true),
			},
			&autoscaling.GroupTagArgs{
				Key:               pulumi.String("AmazonECSManaged"),
				Value:             pulumi.String("true"),
				PropagateAtLaunch: pulumi.Bool(true),
			},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("creating M4b ASG: %w", err)
	}

	return asg, nil
}
