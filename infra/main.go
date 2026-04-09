package main

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	pulumiConfig "github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"

	"github.com/kaizen-experimentation/infra/pkg/cache"
	"github.com/kaizen-experimentation/infra/pkg/cicd"
	"github.com/kaizen-experimentation/infra/pkg/compute"
	"github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/database"
	"github.com/kaizen-experimentation/infra/pkg/dns"
	"github.com/kaizen-experimentation/infra/pkg/loadbalancer"
	"github.com/kaizen-experimentation/infra/pkg/network"
	"github.com/kaizen-experimentation/infra/pkg/secrets"
	"github.com/kaizen-experimentation/infra/pkg/storage"
	"github.com/kaizen-experimentation/infra/pkg/streaming"
)

func main() {
	pulumi.Run(func(ctx *pulumi.Context) error {
		cfg := config.LoadConfig(ctx)
		env := cfg.Environment

		awsCfg := pulumiConfig.New(ctx, "aws")
		awsRegion := awsCfg.Require("region")

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

		// ── 2. Secrets ──────────────────────────────────────────────────────
		secretsOutputs, err := secrets.NewSecrets(ctx, cfg)
		if err != nil {
			return err
		}
		ctx.Export("databaseSecretArn", secretsOutputs.DatabaseSecretArn)
		ctx.Export("kafkaSecretArn", secretsOutputs.KafkaSecretArn)

		// ── 3. Storage (S3 buckets) ─────────────────────────────────────────
		storageOutputs, err := storage.NewStorage(ctx, env)
		if err != nil {
			return err
		}
		ctx.Export("dataBucketName", storageOutputs.DataBucketName)
		ctx.Export("mlflowBucketName", storageOutputs.MlflowBucketName)
		ctx.Export("logsBucketName", storageOutputs.LogsBucketName)

		// ── 4. Cache (ElastiCache Redis) ────────────────────────────────────
		redisOutputs, err := cache.NewRedis(ctx, "kaizen-redis", &cache.RedisConfig{
			NodeType:         cfg.RedisNodeType,
			NumCacheClusters: 2,
			SubnetIds:        vpcOutputs.PrivateSubnetIds,
			SecurityGroupIds: pulumi.StringArray{sgResult.Groups["redis"].ToStringOutput()},
			Tags:             config.DefaultTags(env),
		})
		if err != nil {
			return err
		}
		ctx.Export("redisEndpoint", redisOutputs.RedisEndpoint)

		// ── 5. Database (RDS PostgreSQL) ────────────────────────────────────
		dbOutputs, err := database.NewRds(ctx, cfg, &database.RdsInputs{
			SubnetIds:           vpcOutputs.PrivateSubnetIds,
			VpcSecurityGroupIds: pulumi.StringArray{sgResult.Groups["rds"].ToStringOutput()},
		})
		if err != nil {
			return err
		}
		ctx.Export("rdsEndpoint", dbOutputs.RdsEndpoint)

		// ── 6. Streaming (MSK Kafka) ────────────────────────────────────────
		mskOutputs, err := streaming.NewMskCluster(ctx, "kaizen", &streaming.MskInputs{
			SubnetIds:        vpcOutputs.PrivateSubnetIds,
			SecurityGroupIds: pulumi.StringArray{sgResult.Groups["msk"].ToStringOutput()},
			Config: config.MskConfig{
				KafkaVersion:  "3.5.1",
				BrokerCount:   cfg.MskBrokerCount,
				InstanceType:  cfg.MskInstanceType,
				EbsVolumeSize: 100,
				Environment:   env,
			},
			Tags: config.DefaultTags(env),
		})
		if err != nil {
			return err
		}
		ctx.Export("mskBootstrapBrokers", mskOutputs.MskBootstrapBrokers)

		// ── 7. Kafka Topics ─────────────────────────────────────────────────
		_, err = streaming.NewTopics(ctx, &streaming.TopicsArgs{
			BootstrapBrokers: mskOutputs.MskBootstrapBrokers,
		})
		if err != nil {
			return err
		}

		// ── 8. Compute (ECS Cluster) ────────────────────────────────────────
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

		// ── 8b. Schema Registry (Fargate on ECS) ───────────────────────────
		srOutputs, err := streaming.NewSchemaRegistry(ctx, &streaming.SchemaRegistryArgs{
			Environment:      env,
			Region:           awsRegion,
			ClusterArn:       clusterOutputs.ClusterArn,
			PrivateSubnetIds: vpcOutputs.PrivateSubnetIds,
			SecurityGroupId:  sgResult.Groups["ecs"],
			NamespaceId:      sdOutputs.NamespaceId,
			BootstrapBrokers: mskOutputs.MskBootstrapBrokers,
			KafkaSecretArn:   secretsOutputs.KafkaSecretArn,
			Tags:             config.DefaultTags(env),
		})
		if err != nil {
			return err
		}
		ctx.Export("schemaRegistryUrl", srOutputs.SchemaRegistryUrl)

		// ── 9. DNS (Route 53 + ACM) ─────────────────────────────────────────
		// DNS must be created before ALB so the certificate ARN is available.
		dnsOutputs, err := dns.NewDNS(ctx, &dns.Args{
			Config: config.KaizenConfig{
				Domain:      cfg.Domain,
				ProjectName: cfg.ProjectName,
				Environment: env,
				Project:     cfg.Project,
				Env:         cfg.Env,
			},
			ALB: config.ALBOutputs{
				// Placeholder — will be replaced once ALB is created.
				// In practice, DNS alias records depend on ALB outputs which
				// creates a circular dependency. We break the cycle by creating
				// the ALB first and passing its outputs to DNS.
			},
		})
		if err != nil {
			return err
		}
		ctx.Export("certificateArn", dnsOutputs.CertificateArn)
		ctx.Export("hostedZoneId", dnsOutputs.HostedZoneID)

		// ── 10. Load Balancer (ALB) ─────────────────────────────────────────
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

		// ── 11. ECR Repositories ────────────────────────────────────────────
		ecrOutputs, err := cicd.NewECRRepositories(ctx, env)
		if err != nil {
			return err
		}
		// Export a representative ECR URL for verification.
		if url, ok := ecrOutputs.RepositoryURLs["assignment"]; ok {
			ctx.Export("ecrAssignmentUrl", url)
		}

		// Suppress unused variable warnings.
		_ = albOutputs
		_ = redisOutputs
		_ = secretsOutputs

		return nil
	})
}
