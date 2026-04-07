package config

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	pulumiConfig "github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// Config holds all Pulumi stack configuration values.
type Config struct {
	Environment          string
	VpcCidr              string
	RdsInstanceClass     string
	RdsMultiAz           bool
	MskBrokerCount       int
	MskInstanceType      string
	RedisNodeType        string
	M4bInstanceType      string
	NatGatewayCount      int
	WafEnabled           bool
	FargateMinTasks      int
	CloudwatchRetention  int
}

// LoadConfig reads Pulumi stack configuration into a Config struct.
func LoadConfig(ctx *pulumi.Context) *Config {
	cfg := pulumiConfig.New(ctx, "kaizen-experimentation")

	return &Config{
		Environment:         cfg.Require("environment"),
		VpcCidr:             cfg.Require("vpcCidr"),
		RdsInstanceClass:    cfg.Require("rdsInstanceClass"),
		RdsMultiAz:          cfg.RequireBool("rdsMultiAz"),
		MskBrokerCount:      cfg.RequireInt("mskBrokerCount"),
		MskInstanceType:     cfg.Require("mskInstanceType"),
		RedisNodeType:       cfg.Require("redisNodeType"),
		M4bInstanceType:     cfg.Require("m4bInstanceType"),
		NatGatewayCount:     cfg.RequireInt("natGatewayCount"),
		WafEnabled:          cfg.RequireBool("wafEnabled"),
		FargateMinTasks:     cfg.RequireInt("fargateMinTasks"),
		CloudwatchRetention: cfg.RequireInt("cloudwatchRetentionDays"),
	}
}

// NetworkOutputs is the contract exported by the network module (Infra-1).
type NetworkOutputs struct {
	VpcId             pulumi.IDOutput
	PrivateSubnetIds  pulumi.StringArrayOutput
	PublicSubnetIds   pulumi.StringArrayOutput
	SecurityGroups    map[string]pulumi.IDOutput // keys: "alb", "ecs", "rds", "msk", "redis", "m4b"
	CloudMapNamespace pulumi.IDOutput
}

// DatabaseOutputs is the contract exported by the database module (Infra-2).
type DatabaseOutputs struct {
	RdsEndpoint   pulumi.StringOutput
	RdsPort       pulumi.IntOutput
	RedisEndpoint pulumi.StringOutput
	RedisPort     pulumi.IntOutput
}

// StreamingOutputs is the contract exported by the streaming module (Infra-3).
type StreamingOutputs struct {
	MskBootstrapBrokers pulumi.StringOutput
	SchemaRegistryUrl   pulumi.StringOutput
}

// SecretsOutputs is the contract exported by the secrets module (Infra-2).
type SecretsOutputs struct {
	DatabaseSecretArn pulumi.StringOutput
	KafkaSecretArn    pulumi.StringOutput
	RedisSecretArn    pulumi.StringOutput
	AuthSecretArn     pulumi.StringOutput
}

// StorageOutputs is the contract exported by the storage module (Infra-2).
type StorageOutputs struct {
	DataBucketArn    pulumi.StringOutput
	DataBucketName   pulumi.StringOutput
	MlflowBucketArn  pulumi.StringOutput
	MlflowBucketName pulumi.StringOutput
	LogsBucketArn    pulumi.StringOutput
	LogsBucketName   pulumi.StringOutput
}

// ComputeOutputs is the contract exported by the compute module (Infra-4).
type ComputeOutputs struct {
	ClusterId   pulumi.IDOutput
	ServiceArns map[string]pulumi.StringOutput // keys: "m1", "m2", "m2-orch", "m3", "m4a", "m4b", "m5", "m6", "m7"
	TaskRoleArn pulumi.StringOutput
	ExecRoleArn pulumi.StringOutput
}
