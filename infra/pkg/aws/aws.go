// Package aws is the AWS-side facade for Deploy(). Each function here
// composes one or more module-internal constructors (in pkg/aws/<module>/)
// and returns one of the shared output structs from pkg/types/.
//
// This is the layer that satisfies Phase 0 of ADR-mc: Deploy() switches on
// cloud provider and only the shared types.* shapes cross the boundary
// between Deploy() and provider implementations. Internally, modules retain
// their existing concrete output structs — the aggregators copy the relevant
// fields into the shared types.
package aws

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	pulumiconfig "github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"

	"github.com/kaizen-experimentation/infra/pkg/aws/cache"
	"github.com/kaizen-experimentation/infra/pkg/aws/cicd"
	"github.com/kaizen-experimentation/infra/pkg/aws/compute"
	"github.com/kaizen-experimentation/infra/pkg/aws/database"
	"github.com/kaizen-experimentation/infra/pkg/aws/dns"
	"github.com/kaizen-experimentation/infra/pkg/aws/loadbalancer"
	"github.com/kaizen-experimentation/infra/pkg/aws/network"
	"github.com/kaizen-experimentation/infra/pkg/aws/observability"
	"github.com/kaizen-experimentation/infra/pkg/aws/secrets"
	"github.com/kaizen-experimentation/infra/pkg/aws/storage"
	"github.com/kaizen-experimentation/infra/pkg/aws/streaming"
	"github.com/kaizen-experimentation/infra/pkg/aws/waf"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// ─── Stage 1: Network ───────────────────────────────────────────────────────

// NewNetwork creates the VPC foundation: VPC, subnets, security groups,
// service discovery namespace, and VPC endpoints. Resource ordering matches
// the original main.go exactly — required for the zero-diff gate.
func NewNetwork(ctx *pulumi.Context, cfg *kconfig.Config) (types.NetworkOutputs, error) {
	vpcOut, err := network.NewVpc(ctx)
	if err != nil {
		return types.NetworkOutputs{}, err
	}

	sgRes, err := network.NewSecurityGroups(ctx, "kaizen", &network.SecurityGroupsArgs{
		VpcId: vpcOut.VpcId,
	})
	if err != nil {
		return types.NetworkOutputs{}, err
	}

	sdOut, err := network.NewServiceDiscovery(ctx, &network.ServiceDiscoveryArgs{
		VpcId: vpcOut.VpcId,
	})
	if err != nil {
		return types.NetworkOutputs{}, err
	}
	ctx.Export("cloudMapNamespaceId", sdOut.NamespaceId)

	vpceOut, err := network.NewVpcEndpoints(ctx, &network.VpcEndpointArgs{
		VpcId:                vpcOut.VpcId,
		PrivateSubnetIds:     vpcOut.PrivateSubnetIds,
		PrivateRouteTableIds: vpcOut.PrivateRouteTableIds,
		EcsSecurityGroupId:   sgRes.Groups["ecs"],
		M4bSecurityGroupId:   sgRes.Groups["m4b"],
	})
	if err != nil {
		return types.NetworkOutputs{}, err
	}

	return types.NetworkOutputs{
		VpcId:                vpcOut.VpcId,
		PublicSubnetIds:      vpcOut.PublicSubnetIds,
		PrivateSubnetIds:     vpcOut.PrivateSubnetIds,
		SecurityGroupIds:     sgRes.Groups,
		ServiceDiscoveryId:   sdOut.NamespaceId,
		PrivateRouteTableIds: vpcOut.PrivateRouteTableIds,
		S3VpcEndpointId:      vpceOut.S3EndpointId,
	}, nil
}

// ─── Stage 2: Storage + IAM ─────────────────────────────────────────────────

// NewStorage creates the S3 buckets (data, mlflow, logs).
func NewStorage(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.StorageOutputs, error) {
	out, err := storage.NewStorage(ctx, cfg.Environment, &storage.StorageInputs{
		S3VpcEndpointId: netOut.S3VpcEndpointId,
	})
	if err != nil {
		return types.StorageOutputs{}, err
	}
	ctx.Export("dataBucketName", out.DataBucketName)
	ctx.Export("mlflowBucketName", out.MlflowBucketName)
	ctx.Export("logsBucketName", out.LogsBucketName)
	return types.StorageOutputs{
		DataBucketName:   out.DataBucketName,
		DataBucketRef:    out.DataBucketArn,
		MlflowBucketName: out.MlflowBucketName,
		MlflowBucketRef:  out.MlflowBucketArn,
		LogsBucketName:   out.LogsBucketName,
		LogsBucketRef:    out.LogsBucketArn,
	}, nil
}

// NewIAM creates the ECS task execution role and task roles.
func NewIAM(ctx *pulumi.Context, cfg *kconfig.Config, storageOut types.StorageOutputs) (types.IAMOutputs, error) {
	out, err := network.NewIAMRoles(ctx, &network.IAMArgs{
		Environment:     cfg.Environment,
		DataBucketArn:   storageOut.DataBucketRef,
		MlflowBucketArn: storageOut.MlflowBucketRef,
	})
	if err != nil {
		return types.IAMOutputs{}, err
	}
	ctx.Export("taskExecutionRoleArn", out.ExecRoleArn)
	return types.IAMOutputs{
		ExecRoleRef: out.ExecRoleArn,
		TaskRoleRef: out.TaskRoleArn,
	}, nil
}

// ─── Stage 3: Data Stores ───────────────────────────────────────────────────

// NewCache creates the ElastiCache Redis replication group.
func NewCache(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.CacheOutputs, error) {
	redisSgArr := pulumi.StringArray{netOut.SecurityGroupIds["redis"].ToStringOutput()}
	out, err := cache.NewRedis(ctx, "kaizen-redis", &cache.RedisConfig{
		NodeType:         cfg.RedisNodeType,
		NumCacheClusters: 2,
		SubnetIds:        netOut.PrivateSubnetIds,
		SecurityGroupIds: redisSgArr,
		Tags:             kconfig.DefaultTags(cfg.Environment),
	})
	if err != nil {
		return types.CacheOutputs{}, err
	}
	ctx.Export("redisEndpoint", out.RedisEndpoint)
	return types.CacheOutputs{
		Endpoint: out.RedisEndpoint,
	}, nil
}

// NewDatabase creates the RDS PostgreSQL instance.
func NewDatabase(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.DatabaseOutputs, error) {
	rdsSgArr := pulumi.StringArray{netOut.SecurityGroupIds["rds"].ToStringOutput()}
	out, err := database.NewRds(ctx, cfg, &database.RdsInputs{
		SubnetIds:           netOut.PrivateSubnetIds,
		VpcSecurityGroupIds: rdsSgArr,
	})
	if err != nil {
		return types.DatabaseOutputs{}, err
	}
	ctx.Export("rdsEndpoint", out.RdsEndpoint)
	return types.DatabaseOutputs{
		Endpoint:       out.RdsEndpoint,
		Port:           out.RdsPort,
		InstanceId:     out.RdsInstanceId,
		MasterPassword: out.RdsMasterPassword,
	}, nil
}

// ─── Stage 4: Streaming + Secrets + CICD ────────────────────────────────────

// NewKafkaCluster creates the AWS MSK cluster. SchemaRegistryUrl is filled
// later by NewSchemaRegistry once compute is up.
func NewKafkaCluster(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.StreamingOutputs, error) {
	mskSgArr := pulumi.StringArray{netOut.SecurityGroupIds["msk"].ToStringOutput()}
	kafkaCfg := pulumiconfig.New(ctx, "kafka")
	out, err := streaming.NewMskCluster(ctx, "kaizen", &streaming.MskInputs{
		SubnetIds:        netOut.PrivateSubnetIds,
		SecurityGroupIds: mskSgArr,
		// The streaming module owns the AmazonMSK_* SCRAM secret +
		// association (MSK requires a customer-KMS secret with that name
		// prefix, which the app-facing kafka secret is not).
		SaslUsername: kafkaCfg.Require("saslUsername"),
		SaslPassword: kafkaCfg.RequireSecret("saslPassword"),
		Config: kconfig.MskConfig{
			KafkaVersion:  "3.6.0",
			BrokerCount:   cfg.MskBrokerCount,
			InstanceType:  cfg.MskInstanceType,
			EbsVolumeSize: 100,
			Environment:   cfg.Environment,
			// Brokers must auto-create topics when the declarative
			// kafka:Topic resources are gated off (no broker
			// reachability from outside the VPC).
			AutoCreateTopics: !cfg.ManageKafkaTopics,
			AllowPlaintext:   cfg.MskAllowPlaintext,
		},
		Tags: kconfig.DefaultTags(cfg.Environment),
	})
	if err != nil {
		return types.StreamingOutputs{}, err
	}
	ctx.Export("mskBootstrapBrokers", out.MskBootstrapBrokers)
	ctx.Export("mskClusterArn", out.MskClusterArn)
	return types.StreamingOutputs{
		BootstrapBrokers:          out.MskBootstrapBrokers,
		BootstrapBrokersPlaintext: out.MskBootstrapBrokersPlaintext,
		ClusterArn:                out.MskClusterArn,
		ClusterName:               out.MskClusterName,
	}, nil
}

// NewSecrets creates the Secrets Manager entries for DB, Kafka, Redis, auth.
// The Kafka secret's SASL username is sourced from the streaming provider's
// own config namespace so service clients can authenticate against whichever
// cluster (MSK or Redpanda) the stack is wired to.
func NewSecrets(ctx *pulumi.Context, cfg *kconfig.Config, dbOut types.DatabaseOutputs, streamOut types.StreamingOutputs, cacheOut types.CacheOutputs) (types.SecretsOutputs, error) {
	var kafkaSaslUsername string
	var kafkaSaslPassword pulumi.StringOutput
	switch cfg.StreamingProvider {
	case "redpanda":
		kafkaSaslUsername = pulumiconfig.New(ctx, "redpanda").Require("kafkaUsername")
		kafkaSaslPassword = pulumiconfig.New(ctx, "redpanda").RequireSecret("kafkaPassword")
	default:
		// MSK (and the historical default) reads the credentials from the
		// kafka:saslUsername/saslPassword keys — the same values the SCRAM
		// secret association registers with the cluster.
		kafkaSaslUsername = pulumiconfig.New(ctx, "kafka").Require("saslUsername")
		kafkaSaslPassword = pulumiconfig.New(ctx, "kafka").RequireSecret("saslPassword")
	}
	out, err := secrets.NewSecrets(ctx, cfg, &secrets.SecretsInputs{
		RdsEndpoint:         dbOut.Endpoint,
		RdsMasterPassword:   dbOut.MasterPassword,
		MskBootstrapBrokers: streamOut.BootstrapBrokers,
		RedisEndpoint:       cacheOut.Endpoint,
		KafkaSaslUsername:   kafkaSaslUsername,
		KafkaSaslPassword:   kafkaSaslPassword,
	})
	if err != nil {
		return types.SecretsOutputs{}, err
	}
	ctx.Export("databaseSecretArn", out.DatabaseSecretArn)
	ctx.Export("kafkaSecretArn", out.KafkaSecretArn)
	return types.SecretsOutputs{
		DatabaseSecretRef: out.DatabaseSecretArn,
		KafkaSecretRef:    out.KafkaSecretArn,
		RedisSecretRef:    out.RedisSecretArn,
		AuthSecretRef:     out.AuthSecretArn,
	}, nil
}

// NewKafkaTopics creates the Kafka topics on the MSK cluster.
func NewKafkaTopics(ctx *pulumi.Context, streamOut types.StreamingOutputs) error {
	kafkaCfg := pulumiconfig.New(ctx, "kafka")
	_, err := streaming.NewTopics(ctx, &streaming.TopicsArgs{
		BootstrapBrokers: streamOut.BootstrapBrokers,
		SaslUsername:     pulumi.String(kafkaCfg.Require("saslUsername")),
		SaslPassword:     pulumi.String(kafkaCfg.Require("saslPassword")),
		KafkaVersion:     "3.6.0",
	})
	return err
}

// NewCICD creates ECR repositories for all services.
func NewCICD(ctx *pulumi.Context, cfg *kconfig.Config) (types.CICDOutputs, error) {
	out, err := cicd.NewECRRepositories(ctx, cfg.Environment)
	if err != nil {
		return types.CICDOutputs{}, err
	}
	if url, ok := out.RepositoryURLs["assignment"]; ok {
		ctx.Export("ecrAssignmentUrl", url)
	}
	return types.CICDOutputs{
		RepositoryURLs: out.RepositoryURLs,
	}, nil
}

// ─── Stage 5: Compute ───────────────────────────────────────────────────────

// NewCompute creates the ECS cluster, runs the DB migration, deploys
// 8 Fargate services + the M4b EC2-backed service. Returns an aggregate
// types.ComputeOutputs and the M5 service resource for downstream
// dependency wiring (used by Schema Registry health gate, etc.).
func NewCompute(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	netOut types.NetworkOutputs,
	cicdOut types.CICDOutputs,
	secretsOut types.SecretsOutputs,
	streamOut types.StreamingOutputs,
	tgOut *loadbalancer.TargetGroupOutputs,
) (types.ComputeOutputs, *compute.ServicesOutputs, error) {
	clusterOut, err := compute.NewCluster(ctx, &compute.ClusterArgs{
		Environment:        cfg.Environment,
		M4bInstanceType:    cfg.M4bInstanceType,
		PrivateSubnetIds:   netOut.PrivateSubnetIds,
		M4bSecurityGroupId: netOut.SecurityGroupIds["m4b"],
	})
	if err != nil {
		return types.ComputeOutputs{}, nil, err
	}
	ctx.Export("ecsClusterId", clusterOut.ClusterId)
	ctx.Export("ecsClusterArn", clusterOut.ClusterArn)

	migOut, err := compute.NewMigration(ctx, &compute.MigrationArgs{
		Environment:       cfg.Environment,
		ClusterArn:        clusterOut.ClusterArn,
		PrivateSubnetIds:  netOut.PrivateSubnetIds,
		SecurityGroupId:   netOut.SecurityGroupIds["ecs"],
		ECRRepositoryURL:  cicdOut.RepositoryURLs["management"],
		DatabaseSecretArn: secretsOut.DatabaseSecretRef,
		Region:            "us-east-1",
	})
	if err != nil {
		return types.ComputeOutputs{}, nil, err
	}

	svcOut, err := compute.NewServices(ctx, &compute.ServicesArgs{
		Environment:       cfg.Environment,
		ClusterArn:        clusterOut.ClusterArn,
		PrivateSubnetIds:  netOut.PrivateSubnetIds,
		SecurityGroupId:   netOut.SecurityGroupIds["ecs"],
		NamespaceId:       netOut.ServiceDiscoveryId,
		ECRRepositoryURLs: cicdOut.RepositoryURLs,
		DatabaseSecretArn: secretsOut.DatabaseSecretRef,
		KafkaSecretArn:    secretsOut.KafkaSecretRef,
		RedisSecretArn:    secretsOut.RedisSecretRef,
		AuthSecretArn:     secretsOut.AuthSecretRef,
		KafkaBrokers:      streamOut.BootstrapBrokersPlaintext,
		TargetGroupArns: lbAttachableTargetGroups(cfg, tgOut),
		LbRuleDeps:    tgOut.Rules,
		DesiredCount:  cfg.FargateMinTasks,
		PreDeployDeps: []pulumi.Resource{migOut.RunCommand},
	})
	if err != nil {
		return types.ComputeOutputs{}, nil, err
	}

	_, err = compute.NewM4bService(ctx, &compute.M4bServiceArgs{
		Environment:         cfg.Environment,
		CloudMapNamespaceId: netOut.ServiceDiscoveryId,
		AsgName:             clusterOut.M4bAsgName,
		DependsOnResources:  []pulumi.Resource{svcOut.M5ServiceResource},
	})
	if err != nil {
		return types.ComputeOutputs{}, nil, err
	}

	for key, arn := range svcOut.ServiceArns {
		ctx.Export(fmt.Sprintf("serviceArn_%s", key), arn)
	}
	ctx.Export("taskRoleArn", svcOut.TaskRoleArn)
	ctx.Export("execRoleArn", svcOut.ExecRoleArn)

	return types.ComputeOutputs{
		ClusterId:   clusterOut.ClusterId.ToStringOutput(),
		ClusterName: clusterOut.ClusterName,
		ClusterArn:  clusterOut.ClusterArn,
		ServiceArns: svcOut.ServiceArns,
		M4bAsgName:  clusterOut.M4bAsgName,
	}, svcOut, nil
}

// NewSchemaRegistry deploys the Schema Registry service onto the existing
// ECS cluster and returns the URL so it can be merged into the streaming
// outputs. Returns the service name as well, used by NewKafkaHealthGate.
func NewSchemaRegistry(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	netOut types.NetworkOutputs,
	computeOut types.ComputeOutputs,
	streamOut types.StreamingOutputs,
	secretsOut types.SecretsOutputs,
) (schemaUrl pulumi.StringOutput, schemaSvcName pulumi.StringOutput, err error) {
	out, err := streaming.NewSchemaRegistry(ctx, &streaming.SchemaRegistryArgs{
		Environment:      cfg.Environment,
		Region:           "us-east-1",
		ClusterArn:       computeOut.ClusterArn,
		PrivateSubnetIds: netOut.PrivateSubnetIds,
		SecurityGroupId:  netOut.SecurityGroupIds["ecs"],
		NamespaceId:      netOut.ServiceDiscoveryId,
		BootstrapBrokers: streamOut.BootstrapBrokers,
		KafkaSecretArn:   secretsOut.KafkaSecretRef,
		Tags:             kconfig.DefaultTags(cfg.Environment),
	})
	if err != nil {
		return pulumi.StringOutput{}, pulumi.StringOutput{}, err
	}
	ctx.Export("schemaRegistryUrl", out.SchemaRegistryUrl)
	return out.SchemaRegistryUrl, out.ServiceName, nil
}

// lbAttachableTargetGroups returns the target groups ECS services may attach
// to. In dev tlsEnabled=false mode the gRPC target groups (M1, M7) carry no
// listener rules — ALB forbids GRPC/HTTP2 protocol versions behind the
// plaintext :80 listener — and ECS rejects a LoadBalancers entry whose target
// group is not attached to a load balancer.
func lbAttachableTargetGroups(cfg *kconfig.Config, tgOut *loadbalancer.TargetGroupOutputs) map[string]pulumi.StringOutput {
	arns := map[string]pulumi.StringOutput{
		"m5": tgOut.M5ManagementTgArn,
		"m6": tgOut.M6UITgArn,
	}
	if cfg.TlsEnabled {
		arns["m1"] = tgOut.M1AssignmentTgArn
		arns["m7"] = tgOut.M7FlagsTgArn
	}
	return arns
}

// ─── Stage 6: Edge + Observability ──────────────────────────────────────────

// NewEdge creates DNS, ALB, DNS aliases, target groups, and (conditionally) WAF.
func NewEdge(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	netOut types.NetworkOutputs,
	storageOut types.StorageOutputs,
) (types.EdgeOutputs, *loadbalancer.TargetGroupOutputs, error) {
	dnsOut, err := dns.NewDNS(ctx, &dns.Args{
		Config: kconfig.KaizenConfig{
			Domain:      cfg.Domain,
			ProjectName: cfg.ProjectName,
			Environment: cfg.Environment,
			Project:     cfg.Project,
			Env:         cfg.Env,
		},
	})
	if err != nil {
		return types.EdgeOutputs{}, nil, err
	}
	ctx.Export("certificateArn", dnsOut.CertificateArn)
	ctx.Export("hostedZoneId", dnsOut.HostedZoneID)

	albOut, err := loadbalancer.NewALB(ctx, &loadbalancer.ALBInputs{
		PublicSubnetIds: netOut.PublicSubnetIds,
		SecurityGroupId: netOut.SecurityGroupIds["alb"].ToStringOutput(),
		CertificateArn:  dnsOut.CertificateArn,
		LogsBucketName:  storageOut.LogsBucketName,
		Environment:     cfg.Environment,
		TlsEnabled:      cfg.TlsEnabled,
	})
	if err != nil {
		return types.EdgeOutputs{}, nil, err
	}
	ctx.Export("albDnsName", albOut.AlbDnsName)
	ctx.Export("albArn", albOut.AlbArn)

	if err := dns.NewDNSAliases(ctx, &dns.AliasArgs{
		ZoneId:     dnsOut.HostedZoneID,
		ZoneName:   fmt.Sprintf("kaizen.%s", cfg.Domain),
		ALBDnsName: albOut.AlbDnsName,
		ALBZoneId:  albOut.AlbZoneId,
	}); err != nil {
		return types.EdgeOutputs{}, nil, err
	}

	tgOut, err := loadbalancer.NewTargetGroups(ctx, &loadbalancer.TargetGroupInputs{
		VpcId:            netOut.VpcId.ToStringOutput(),
		HttpsListenerArn: albOut.HttpsListenerArn,
		Domain:           cfg.Domain,
		Environment:      cfg.Environment,
		TlsEnabled:       cfg.TlsEnabled,
	})
	if err != nil {
		return types.EdgeOutputs{}, nil, err
	}

	if cfg.WafEnabled {
		_, err = waf.New(ctx, &waf.Inputs{
			AlbArn:           albOut.AlbArn,
			Environment:      cfg.Environment,
			RateLimitPerIP:   cfg.WafRateLimitPerIP,
			BlockedCountries: cfg.WafBlockedCountries,
		})
		if err != nil {
			return types.EdgeOutputs{}, nil, err
		}
	}

	return types.EdgeOutputs{
		LoadBalancerDns:       albOut.AlbDnsName,
		LoadBalancerArn:       albOut.AlbArn,
		LoadBalancerArnSuffix: albOut.AlbArnSuffix,
		CertificateRef:        dnsOut.CertificateArn,
		HostedZoneId:          dnsOut.HostedZoneID.ToStringOutput(),
	}, tgOut, nil
}

// NewAutoscaling wires ECS service auto-scaling policies to ALB metrics.
// svcOut supplies the ECS service resources as explicit DependsOn edges —
// scaling targets reference services by constructed name string, which
// otherwise races service creation.
func NewAutoscaling(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	computeOut types.ComputeOutputs,
	svcOut *compute.ServicesOutputs,
	albArnSuffix pulumi.StringOutput,
	tgOut *loadbalancer.TargetGroupOutputs,
) error {
	args := compute.DefaultAutoscalingArgs(cfg.Environment)
	args.ClusterName = computeOut.ClusterName
	args.ALBFullName = albArnSuffix
	args.M1TargetGroupFullName = tgOut.M1AssignmentTgArnSuffix
	args.M7TargetGroupFullName = tgOut.M7FlagsTgArnSuffix
	args.AlbAttached = cfg.TlsEnabled
	if svcOut != nil {
		args.DependsOn = svcOut.AllServiceResources
	}
	_, err := compute.NewAutoscaling(ctx, &args)
	return err
}

// NewObservability creates CloudWatch log groups, alarms, and the AMP/AMG
// observability workspaces.
func NewObservability(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	dbOut types.DatabaseOutputs,
	streamOut types.StreamingOutputs,
	computeOut types.ComputeOutputs,
) error {
	// Gate MSK-specific CloudWatch inputs on the streaming provider.
	// When streamingProvider=redpanda, there is no AWS/Kafka cluster, so
	// MskClusterName must not be forwarded — passing it would cause
	// NewCloudWatch to create a consumer-lag alarm against a non-existent
	// MSK cluster, which would alarm perpetually and incur cost.
	cwArgs := &observability.CloudWatchArgs{
		Environment:             cfg.Environment,
		CloudwatchRetention:     cfg.CloudwatchRetention,
		RdsInstanceId:           dbOut.InstanceId,
		M4bAutoScalingGroupName: computeOut.M4bAsgName,
		Tags:                    kconfig.DefaultTags(cfg.Environment),
	}
	if cfg.StreamingProvider == "msk" {
		cwArgs.MskAlarmEnabled = true
		cwArgs.MskClusterName = streamOut.ClusterName
	}
	if _, err := observability.NewCloudWatch(ctx, cwArgs); err != nil {
		return err
	}

	if _, err := observability.New(ctx, &observability.Args{
		Environment:    cfg.Environment,
		EcsClusterName: computeOut.ClusterName,
		Tags:           kconfig.DefaultTags(cfg.Environment),
		DisableGrafana: !cfg.GrafanaEnabled,
	}); err != nil {
		return err
	}
	return nil
}

// NewKafkaHealthGate creates the Schema Registry health alarm.
func NewKafkaHealthGate(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	computeOut types.ComputeOutputs,
	schemaRegSvcName pulumi.StringOutput,
) error {
	out, err := streaming.NewHealthGate(ctx, &streaming.HealthGateArgs{
		Environment:               cfg.Environment,
		ClusterName:               computeOut.ClusterName,
		SchemaRegistryServiceName: schemaRegSvcName,
		Tags:                      kconfig.DefaultTags(cfg.Environment),
	})
	if err != nil {
		return err
	}
	ctx.Export("schemaRegistryHealthAlarmArn", out.HealthAlarmArn)
	return nil
}
