package main

import (
	"fmt"

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
	"github.com/kaizen-experimentation/infra/pkg/waf"
)

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		cfg := kconfig.LoadConfig(ctx)
		env := cfg.Environment

		ctx.Export("environment", pulumi.String(env))

		// =====================================================================
		// Stage 1: Network Foundation (Infra-1)
		// No dependencies — everything else builds on this.
		// =====================================================================

		// ── 1. VPC ──────────────────────────────────────────────────────────
		vpcOutputs, err := network.NewVpc(ctx)
		if err != nil {
			return err
		}

		// ── 2. Security Groups ──────────────────────────────────────────────
		sgResult, err := network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
			VpcId: vpcOutputs.VpcId,
		})
		if err != nil {
			return err
		}

		// ── 3. Service Discovery (Cloud Map) ────────────────────────────────
		sdOutputs, err := network.NewServiceDiscovery(ctx, &network.ServiceDiscoveryArgs{
			VpcId: vpcOutputs.VpcId,
		})
		if err != nil {
			return err
		}
		ctx.Export("cloudMapNamespaceId", sdOutputs.NamespaceId)

		// ── 4. VPC Endpoints ────────────────────────────────────────────────
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

		// =====================================================================
		// Stage 2: Storage + IAM (Infra-2, partial)
		// Depends on: VPC endpoints (S3 gateway).
		// =====================================================================

		// ── 5. S3 Buckets ───────────────────────────────────────────────────
		storageOutputs, err := storage.NewStorage(ctx, env, &storage.StorageInputs{
			S3VpcEndpointId: vpceOutputs.S3EndpointId,
		})
		if err != nil {
			return err
		}
		ctx.Export("dataBucketName", storageOutputs.DataBucketName)
		ctx.Export("mlflowBucketName", storageOutputs.MlflowBucketName)
		ctx.Export("logsBucketName", storageOutputs.LogsBucketName)

		// ── 6. IAM Roles ────────────────────────────────────────────────────
		iamOutputs, err := network.NewIAMRoles(ctx, &network.IAMArgs{
			Environment:     env,
			DataBucketArn:   storageOutputs.DataBucketArn,
			MlflowBucketArn: storageOutputs.MlflowBucketArn,
		})
		if err != nil {
			return err
		}
		ctx.Export("taskExecutionRoleArn", iamOutputs.ExecRoleArn)

		// =====================================================================
		// Stage 3: Data Stores (Infra-2)
		// Depends on: VPC subnets, security groups.
		// =====================================================================

		// ── 7. ElastiCache Redis ────────────────────────────────────────────
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

		// ── 8. RDS PostgreSQL ───────────────────────────────────────────────
		dbOutputs, err := database.NewRds(ctx, cfg, &database.RdsInputs{
			SubnetIds:           vpcOutputs.PrivateSubnetIds,
			VpcSecurityGroupIds: pulumi.StringArray{sgResult.Groups["rds"].ToStringOutput()},
		})
		if err != nil {
			return err
		}
		ctx.Export("rdsEndpoint", dbOutputs.RdsEndpoint)

		// =====================================================================
		// Stage 4: Streaming (Infra-3)
		// Depends on: VPC subnets, security groups.
		// =====================================================================

		// ── 9. MSK Kafka Cluster ────────────────────────────────────────────
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
		ctx.Export("mskClusterArn", mskOutputs.MskClusterArn)

		// ── 10. Secrets Manager ─────────────────────────────────────────────
		// Depends on: RDS endpoint, MSK brokers, Redis endpoint.
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

		// ── 11. Kafka Topics ────────────────────────────────────────────────
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

		// ── 12. ECR Repositories ────────────────────────────────────────────
		ecrOutputs, err := cicd.NewECRRepositories(ctx, env)
		if err != nil {
			return err
		}
		if url, ok := ecrOutputs.RepositoryURLs["assignment"]; ok {
			ctx.Export("ecrAssignmentUrl", url)
		}

		// =====================================================================
		// Stage 5: Compute (Infra-4)
		// Depends on: networking, data stores, secrets, ECR.
		// =====================================================================

		// ── 13. ECS Cluster + M4b EC2 ───────────────────────────────────────
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

		// ── 13.5. Database Migration (pre-deploy, blocks M5 service) ────────
		migrationOutputs, err := compute.NewMigration(ctx, &compute.MigrationArgs{
			Environment:       env,
			ClusterArn:        clusterOutputs.ClusterArn,
			PrivateSubnetIds:  vpcOutputs.PrivateSubnetIds,
			SecurityGroupId:   sgResult.Groups["ecs"],
			ECRRepositoryURL:  ecrOutputs.RepositoryURLs["management"],
			DatabaseSecretArn: secretsOutputs.DatabaseSecretArn,
			Region:            "us-east-1",
		})
		if err != nil {
			return err
		}

		// ── 14. ECS Fargate Services (tiered startup ordering) ──────────────
		//
		// Service dependency graph:
		//   Tier 0: M5 (foundation — owns PG schema via migration)
		//   Tier 1: M1, M2, M2-Orch (core — depend on M5 healthy)
		//           M4b is logically Tier 1 but EC2-based (section 15)
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
			PreDeployDeps:     []pulumi.Resource{migrationOutputs.RunCommand},
		})
		if err != nil {
			return err
		}

		// ── 15. M4b Operational Resources (Tier 1 — depends on M5) ──────────
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

		// ── 16. Schema Registry (ECS Fargate) ───────────────────────────────
		// Depends on: ECS cluster, MSK, secrets, Cloud Map.
		schemaRegOutputs, err := streaming.NewSchemaRegistry(ctx, &streaming.SchemaRegistryArgs{
			Environment:      env,
			Region:           "us-east-1",
			ClusterArn:       clusterOutputs.ClusterArn,
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
		ctx.Export("schemaRegistryUrl", schemaRegOutputs.SchemaRegistryUrl)

		// =====================================================================
		// Stage 6: Edge + Observability (Infra-5)
		// Depends on: networking, compute, storage.
		// =====================================================================

		// ── 17. DNS Zone + ACM Certificate ──────────────────────────────────
		// Created before ALB because the HTTPS listener needs the certificate.
		dnsOutputs, err := dns.NewDNS(ctx, &dns.Args{
			Config: kconfig.KaizenConfig{
				Domain:      cfg.Domain,
				ProjectName: cfg.ProjectName,
				Environment: env,
				Project:     cfg.Project,
				Env:         cfg.Env,
			},
		})
		if err != nil {
			return err
		}
		ctx.Export("certificateArn", dnsOutputs.CertificateArn)
		ctx.Export("hostedZoneId", dnsOutputs.HostedZoneID)

		// ── 18. ALB ─────────────────────────────────────────────────────────
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

		// ── 19. DNS Alias Records ───────────────────────────────────────────
		// Created after ALB to resolve the DNS<->ALB circular dependency.
		err = dns.NewDNSAliases(ctx, &dns.AliasArgs{
			ZoneId:     dnsOutputs.HostedZoneID,
			ZoneName:   fmt.Sprintf("kaizen.%s", cfg.Domain),
			ALBDnsName: albOutputs.AlbDnsName,
			ALBZoneId:  albOutputs.AlbZoneId,
		})
		if err != nil {
			return err
		}

		// ── 20. Target Groups + Listener Rules ──────────────────────────────
		tgOutputs, err := loadbalancer.NewTargetGroups(ctx, &loadbalancer.TargetGroupInputs{
			VpcId:            vpcOutputs.VpcId.ToStringOutput(),
			HttpsListenerArn: albOutputs.HttpsListenerArn,
			Domain:           cfg.Domain,
			Environment:      env,
		})
		if err != nil {
			return err
		}

		// ── 20b. WAF (conditional on kaizen:wafEnabled) ─────────────────────
		if cfg.WafEnabled {
			_, err = waf.New(ctx, &waf.Inputs{
				AlbArn:           albOutputs.AlbArn,
				Environment:      env,
				RateLimitPerIP:   cfg.WafRateLimitPerIP,
				BlockedCountries: cfg.WafBlockedCountries,
			})
			if err != nil {
				return err
			}
		}

		// ── 21. Autoscaling Policies ────────────────────────────────────────
		scalingArgs := compute.DefaultAutoscalingArgs(env)
		scalingArgs.ClusterName = clusterOutputs.ClusterName
		scalingArgs.ALBFullName = albOutputs.AlbArnSuffix
		scalingArgs.M1TargetGroupFullName = tgOutputs.M1AssignmentTgArnSuffix
		scalingArgs.M7TargetGroupFullName = tgOutputs.M7FlagsTgArnSuffix
		_, err = compute.NewAutoscaling(ctx, &scalingArgs)
		if err != nil {
			return err
		}

		// ── 22. CloudWatch Log Groups + Alarms ──────────────────────────────
		_, err = observability.NewCloudWatch(ctx, &observability.CloudWatchArgs{
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

		// ── 23. AMP/AMG Observability Workspaces ────────────────────────────
		_, err = observability.New(ctx, &observability.Args{
			Environment:    env,
			EcsClusterName: clusterOutputs.ClusterName,
			Tags:           kconfig.DefaultTags(env),
		})
		if err != nil {
			return err
		}

		// ── 24. Schema Registry Health Gate ─────────────────────────────────
		healthGateOutputs, err := streaming.NewHealthGate(ctx, &streaming.HealthGateArgs{
			Environment:               env,
			ClusterName:               clusterOutputs.ClusterName,
			SchemaRegistryServiceName: schemaRegOutputs.ServiceName,
			Tags:                      kconfig.DefaultTags(env),
		})
		if err != nil {
			return err
		}
		ctx.Export("schemaRegistryHealthAlarmArn", healthGateOutputs.HealthAlarmArn)

		// =====================================================================
		// Service URL Exports
		// =====================================================================

		for key, arn := range svcOutputs.ServiceArns {
			ctx.Export(fmt.Sprintf("serviceArn_%s", key), arn)
		}
		ctx.Export("taskRoleArn", svcOutputs.TaskRoleArn)
		ctx.Export("execRoleArn", svcOutputs.ExecRoleArn)

		return nil
	})
}
