package main

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"

	"github.com/kaizen-experimentation/infra/pkg/cache"
	"github.com/kaizen-experimentation/infra/pkg/cicd"
	"github.com/kaizen-experimentation/infra/pkg/compute"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/database"
	"github.com/kaizen-experimentation/infra/pkg/dns"
	"github.com/kaizen-experimentation/infra/pkg/loadbalancer"
	"github.com/kaizen-experimentation/infra/pkg/network"
	"github.com/kaizen-experimentation/infra/pkg/observability"
	"github.com/kaizen-experimentation/infra/pkg/secrets"
	"github.com/kaizen-experimentation/infra/pkg/storage"
	"github.com/kaizen-experimentation/infra/pkg/streaming"
)

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		cfg := kconfig.LoadConfig(ctx)
		env := cfg.Environment

		ctx.Export("environment", pulumi.String(env))

		// ── 1. Network foundation ───────────────────────────────────────────
		vpcOutputs, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}

		sgResult, err := network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOutputs.VpcId,
		})
		if err != nil {
			return err
		}

		sdOutputs, err := network.NewServiceDiscovery(ctx, &network.ServiceDiscoveryArgs{
			VpcId: vpcOutputs.VpcId,
		})
		if err != nil {
			return err
		}
		ctx.Export("cloudMapNamespaceId", sdOutputs.NamespaceId)

		// VPC endpoints for private AWS service access (S3 gateway, ECR/Logs/SM interface).
		vpceOutputs, err := network.NewVpcEndpoints(ctx, &network.VpcEndpointArgs{
			VpcId:                vpcOutputs.VpcId,
			PrivateSubnetIds:     vpcOutputs.PrivateSubnetIds,
			PrivateRouteTableIds: vpcOutputs.PrivateRouteTableIds,
			EcsSecurityGroupId:   sgResult.Groups["ecs"],
			M4bSecurityGroupId:   sgResult.Groups["m4b"],
		})
		if err != nil {
			return err
		}

		// ── 2. Storage (S3 buckets) ─────────────────────────────────────────
		storageOutputs, err := storage.NewStorage(ctx, env, &storage.StorageInputs{
			S3VpcEndpointId: vpceOutputs.S3EndpointId,
		})
		if err != nil {
			return err
		}
		ctx.Export("dataBucketName", storageOutputs.DataBucketName)
		ctx.Export("mlflowBucketName", storageOutputs.MlflowBucketName)
		ctx.Export("logsBucketName", storageOutputs.LogsBucketName)

		// IAM roles for ECS tasks and CI/CD (depends on S3 bucket ARNs).
		iamOutputs, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     env,
			DataBucketArn:   storageOutputs.DataBucketArn,
			MlflowBucketArn: storageOutputs.MlflowBucketArn,
		})
		if err != nil {
			return err
		}

		// ── 3. Cache (ElastiCache Redis) ────────────────────────────────────
		redisOutputs, err := cache.NewRedis(ctx, "kaizen-redis", &cache.RedisConfig{
			NodeType:         cfg.RedisNodeType,
			NumCacheClusters: 2,
			SubnetIds:        vpcOutputs.PrivateSubnetIds,
			SecurityGroupIds: pulumi.StringArray{sgResult.Groups["redis"].ToStringOutput()},
			Tags:             kconfig.DefaultTags(env),
		})
		if err != nil {
			return err
		}
		ctx.Export("redisEndpoint", redisOutputs.RedisEndpoint)

		// ── 4. Database (RDS PostgreSQL) ────────────────────────────────────
		dbOutputs, err := database.NewRds(ctx, cfg, &database.RdsInputs{
			SubnetIds:           vpcOutputs.PrivateSubnetIds,
			VpcSecurityGroupIds: pulumi.StringArray{sgResult.Groups["rds"].ToStringOutput()},
		})
		if err != nil {
			return err
		}
		ctx.Export("rdsEndpoint", dbOutputs.RdsEndpoint)

		// ── 5. Streaming (MSK Kafka) ────────────────────────────────────────
		mskOutputs, err := streaming.NewMskCluster(ctx, "kaizen", &streaming.MskInputs{
			SubnetIds:        vpcOutputs.PrivateSubnetIds,
			SecurityGroupIds: pulumi.StringArray{sgResult.Groups["msk"].ToStringOutput()},
			KafkaSecretArn:   nil, // SCRAM association wired after secrets are created below.
			Config: kconfig.MskConfig{
				KafkaVersion:  "3.5.1",
				BrokerCount:   cfg.MskBrokerCount,
				InstanceType:  cfg.MskInstanceType,
				EbsVolumeSize: 100,
				Environment:   env,
			},
			Tags: kconfig.DefaultTags(env),
		})
		if err != nil {
			return err
		}
		ctx.Export("mskBootstrapBrokers", mskOutputs.MskBootstrapBrokers)

		// ── 6. Secrets (depends on RDS, MSK, Redis endpoints) ───────────────
		secretsOutputs, err := secrets.NewSecrets(ctx, cfg, &secrets.SecretsInputs{
			RdsEndpoint:         dbOutputs.RdsEndpoint,
			MskBootstrapBrokers: mskOutputs.MskBootstrapBrokers,
			RedisEndpoint:       redisOutputs.RedisEndpoint,
		})
		if err != nil {
			return err
		}
		ctx.Export("databaseSecretArn", secretsOutputs.DatabaseSecretArn)
		ctx.Export("kafkaSecretArn", secretsOutputs.KafkaSecretArn)

		// ── 7. Kafka Topics (SASL_SSL auth via stack config) ────────────────
		kafkaCfg := config.New(ctx, "kafka")
		_, err = streaming.NewTopics(ctx, &streaming.TopicsArgs{
			BootstrapBrokers: mskOutputs.MskBootstrapBrokers,
			SaslUsername:     pulumi.String(kafkaCfg.Require("saslUsername")),
			SaslPassword:     pulumi.String(kafkaCfg.Require("saslPassword")),
			KafkaVersion:     "3.5.1",
		})
		if err != nil {
			return err
		}

		// ── 8. Schema Registry (ECS Fargate service) ────────────────────────
		schemaRegOutputs, err := streaming.NewSchemaRegistry(ctx, &streaming.SchemaRegistryArgs{
			Environment:      env,
			Region:           "us-east-1",
			ClusterArn:       pulumi.StringOutput{}, // Placeholder — resolved after cluster creation.
			PrivateSubnetIds: vpcOutputs.PrivateSubnetIds,
			SecurityGroupId:  sgResult.Groups["ecs"],
			NamespaceId:      sdOutputs.NamespaceId,
			BootstrapBrokers: mskOutputs.MskBootstrapBrokers,
			KafkaSecretArn:   secretsOutputs.KafkaSecretArn,
			Tags:             kconfig.DefaultTags(env),
		})
		if err != nil {
			return err
		}
		_ = schemaRegOutputs

		// ── 9. ECR Repositories ─────────────────────────────────────────────
		ecrOutputs, err := cicd.NewECRRepositories(ctx, env)
		if err != nil {
			return err
		}
		if url, ok := ecrOutputs.RepositoryURLs["assignment"]; ok {
			ctx.Export("ecrAssignmentUrl", url)
		}

		// ── 10. Compute (ECS Cluster + M4b EC2) ────────────────────────────
		clusterOutputs, err := compute.NewCluster(ctx, &compute.ClusterArgs{
			Environment:        env,
			M4bInstanceType:    cfg.M4bInstanceType,
			PrivateSubnetIds:   vpcOutputs.PrivateSubnetIds,
			M4bSecurityGroupId: sgResult.Groups["m4b"],
		})
		if err != nil {
			return err
		}
		ctx.Export("ecsClusterId", clusterOutputs.ClusterId)
		ctx.Export("ecsClusterArn", clusterOutputs.ClusterArn)

		// ── 11. ECS Fargate Services (tiered startup ordering) ──────────────
		//
		// Service dependency graph:
		//   Tier 0: M5 (foundation — owns PG schema via migration)
		//   Tier 1: M1, M2, M2-Orch (core — depend on M5 healthy)
		//           M4b is logically Tier 1 but EC2-based (section 12)
		//   Tier 2: M3, M4a, M6, M7 (dependent — after Tier 1 + M4b)
		//
		// Pulumi DependsOn + WaitForSteadyState enforce deployment ordering.
		// Health-gate init containers enforce runtime ordering via polling.
		svcOutputs, err := compute.NewServices(ctx, &compute.ServicesArgs{
			Environment:       env,
			ClusterArn:        clusterOutputs.ClusterArn,
			PrivateSubnetIds:  vpcOutputs.PrivateSubnetIds,
			SecurityGroupId:   sgResult.Groups["ecs"],
			NamespaceId:       sdOutputs.NamespaceId,
			ECRRepositoryURLs: ecrOutputs.RepositoryURLs,
			DatabaseSecretArn: secretsOutputs.DatabaseSecretArn,
			KafkaSecretArn:    secretsOutputs.KafkaSecretArn,
			RedisSecretArn:    secretsOutputs.RedisSecretArn,
			AuthSecretArn:     secretsOutputs.AuthSecretArn,
			DesiredCount:      cfg.FargateMinTasks,
		})
		if err != nil {
			return err
		}

		// ── 12. M4b Operational Resources (Tier 1 — depends on M5) ──────────
		// M4b is logically Tier 1: it starts after M5 is healthy.
		// Pulumi DependsOn ensures M4b ops are created only after M5's ECS
		// service reaches steady state.
		_, err = compute.NewM4bService(ctx, &compute.M4bServiceArgs{
			Environment:         env,
			CloudMapNamespaceId: sdOutputs.NamespaceId,
			AsgName:             clusterOutputs.M4bAsgName,
			DependsOnResources:  []pulumi.Resource{svcOutputs.M5ServiceResource},
		})
		if err != nil {
			return err
		}

		// ── 13. DNS (Route 53 + ACM) ────────────────────────────────────────
		dnsOutputs, err := dns.NewDNS(ctx, &dns.Args{
			Config: kconfig.KaizenConfig{
				Domain:      cfg.Domain,
				ProjectName: cfg.ProjectName,
				Environment: env,
				Project:     cfg.Project,
				Env:         cfg.Env,
			},
			ALB: kconfig.ALBOutputs{},
		})
		if err != nil {
			return err
		}
		ctx.Export("certificateArn", dnsOutputs.CertificateArn)
		ctx.Export("hostedZoneId", dnsOutputs.HostedZoneID)

		// ── 14. Load Balancer (ALB) ─────────────────────────────────────────
		albOutputs, err := loadbalancer.NewALB(ctx, &loadbalancer.ALBInputs{
			PublicSubnetIds: vpcOutputs.PublicSubnetIds,
			SecurityGroupId: sgResult.Groups["alb"].ToStringOutput(),
			CertificateArn:  dnsOutputs.CertificateArn,
			LogsBucketName:  storageOutputs.LogsBucketName,
			Environment:     env,
		})
		if err != nil {
			return err
		}
		ctx.Export("albDnsName", albOutputs.AlbDnsName)
		ctx.Export("albArn", albOutputs.AlbArn)

		// ── 15. Target Groups + Listener Rules ──────────────────────────────
		_, err = loadbalancer.NewTargetGroups(ctx, &loadbalancer.TargetGroupInputs{
			VpcId:            vpcOutputs.VpcId.ToStringOutput(),
			HttpsListenerArn: albOutputs.HttpsListenerArn,
			Domain:           cfg.Domain,
			Environment:      env,
		})
		if err != nil {
			return err
		}

		// ── 16. Autoscaling Policies ────────────────────────────────────────
		scalingArgs := compute.DefaultAutoscalingArgs(env)
		scalingArgs.ClusterName = clusterOutputs.ClusterName
		// ALB full name and target group full names would be wired from the ALB/TG
		// outputs once those export FullName fields. For now, autoscaling is created
		// without ALB-based request count metrics (CPU-based scaling still works).
		_, err = compute.NewAutoscaling(ctx, &scalingArgs)
		if err != nil {
			return err
		}

		// ── 17. Observability: CloudWatch Log Groups + Alarms ───────────────
		cwOutputs, err := observability.NewCloudWatch(ctx, &observability.CloudWatchArgs{
			Environment:             env,
			CloudwatchRetention:     cfg.CloudwatchRetention,
			RdsInstanceId:           dbOutputs.RdsInstanceId,
			MskClusterName:          mskOutputs.MskClusterName,
			M4bAutoScalingGroupName: clusterOutputs.M4bAsgName,
			Tags:                    kconfig.DefaultTags(env),
		})
		if err != nil {
			return err
		}

		// ── 18. Observability: AMP/AMG Workspaces ───────────────────────────
		ampOutputs, err := observability.New(ctx, &observability.Args{
			Environment:    env,
			EcsClusterName: clusterOutputs.ClusterName,
			Tags:           kconfig.DefaultTags(env),
		})
		if err != nil {
			return err
		}

		// Suppress unused variable warnings.
		_ = iamOutputs
		_ = svcOutputs
		_ = cwOutputs
		_ = ampOutputs

		return nil
	})
}
