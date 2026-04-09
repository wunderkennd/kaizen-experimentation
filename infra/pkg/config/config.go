package config

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	pulumiConfig "github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"
)

// Environment represents the deployment environment.
type Environment string

const (
	EnvDev     Environment = "dev"
	EnvStaging Environment = "staging"
	EnvProd    Environment = "prod"
)

// Config holds all Pulumi stack configuration values.
type Config struct {
	// Core identification
	Project     string
	Environment string
	Env         Environment
	Domain      string
	ProjectName string

	// Network
	VpcCidr         string
	NatGatewayCount int

	// Database
	RdsInstanceClass string
	RdsMultiAz       bool

	// Streaming
	MskBrokerCount  int
	MskInstanceType string

	// Cache
	RedisNodeType string

	// Compute
	M4bInstanceType string
	FargateMinTasks int

	// Observability
	CloudwatchRetention int

	// Security
	WafEnabled bool
}

// KaizenConfig is an alias for Config. Some modules reference this type name.
type KaizenConfig = Config

// IsProd returns true when the environment is production.
func (c *Config) IsProd() bool {
	return c.Env == EnvProd
}

// IsStaging returns true when the environment is staging.
func (c *Config) IsStaging() bool {
	return c.Env == EnvStaging
}

// SecretPath returns a namespaced path for a Secrets Manager secret.
func (c *Config) SecretPath(name string) string {
	return fmt.Sprintf("kaizen/%s/%s", c.Env, name)
}

// ResourceName returns a consistent resource name incorporating the environment.
func (c *Config) ResourceName(name string) string {
	return fmt.Sprintf("kaizen-%s-%s", c.Env, name)
}

// LoadConfig reads Pulumi stack configuration into a Config struct.
func LoadConfig(ctx *pulumi.Context) *Config {
	cfg := pulumiConfig.New(ctx, "kaizen-experimentation")

	env := cfg.Require("environment")

	domain := ""
	if v, err := cfg.Try("domain"); err == nil {
		domain = v
	}

	projectName := "kaizen-experimentation"
	if v, err := cfg.Try("projectName"); err == nil {
		projectName = v
	}

	return &Config{
		Project:             "kaizen",
		Environment:         env,
		Env:                 Environment(env),
		Domain:              domain,
		ProjectName:         projectName,
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

// ---------------------------------------------------------------------------
// Tag helpers
// ---------------------------------------------------------------------------

// CommonTags returns the base tag set derived from Pulumi stack config.
func CommonTags(ctx *pulumi.Context) pulumi.StringMap {
	cfg := pulumiConfig.New(ctx, "kaizen-experimentation")
	env := cfg.Require("environment")
	return pulumi.StringMap{
		"Project":     pulumi.String("kaizen"),
		"Environment": pulumi.String(env),
		"ManagedBy":   pulumi.String("pulumi"),
	}
}

// DefaultTags returns the base tag set for a given environment string.
// This is used by modules that receive the environment as a plain string
// rather than reading it from Pulumi config.
func DefaultTags(env string) pulumi.StringMap {
	return pulumi.StringMap{
		"Project":     pulumi.String("kaizen"),
		"Environment": pulumi.String(env),
		"ManagedBy":   pulumi.String("pulumi"),
	}
}

// MergeTags combines a base tag map with extra overrides. Extra keys win.
func MergeTags(base, extra pulumi.StringMap) pulumi.StringMap {
	merged := pulumi.StringMap{}
	for k, v := range base {
		merged[k] = v
	}
	for k, v := range extra {
		merged[k] = v
	}
	return merged
}

// ---------------------------------------------------------------------------
// Cross-module output contracts
// ---------------------------------------------------------------------------

// NetworkOutputs is the contract exported by the network module.
type NetworkOutputs struct {
	VpcId             pulumi.IDOutput
	PrivateSubnetIds  pulumi.StringArrayOutput
	PublicSubnetIds   pulumi.StringArrayOutput
	SecurityGroups    map[string]pulumi.IDOutput // keys: "alb", "ecs", "rds", "msk", "redis", "m4b"
	CloudMapNamespace pulumi.IDOutput
}

// DatabaseOutputs is the contract exported by the database module.
type DatabaseOutputs struct {
	RdsEndpoint   pulumi.StringOutput
	RdsPort       pulumi.IntOutput
	RedisEndpoint pulumi.StringOutput
	RedisPort     pulumi.IntOutput
}

// StreamingOutputs is the contract exported by the streaming module.
type StreamingOutputs struct {
	MskClusterArn       pulumi.StringOutput
	MskClusterName      pulumi.StringOutput
	MskBootstrapBrokers pulumi.StringOutput
	SchemaRegistryUrl   pulumi.StringOutput
}

// SecretsOutputs is the contract exported by the secrets module.
type SecretsOutputs struct {
	DatabaseSecretArn pulumi.StringOutput
	KafkaSecretArn    pulumi.StringOutput
	RedisSecretArn    pulumi.StringOutput
	AuthSecretArn     pulumi.StringOutput
}

// StorageOutputs is the contract exported by the storage module.
type StorageOutputs struct {
	DataBucketArn    pulumi.StringOutput
	DataBucketName   pulumi.StringOutput
	MlflowBucketArn  pulumi.StringOutput
	MlflowBucketName pulumi.StringOutput
	LogsBucketArn    pulumi.StringOutput
	LogsBucketName   pulumi.StringOutput
}

// ComputeOutputs is the contract exported by the compute module.
type ComputeOutputs struct {
	ClusterId   pulumi.IDOutput
	ServiceArns map[string]pulumi.StringOutput // keys: "m1", "m2", "m2-orch", "m3", "m4a", "m4b", "m5", "m6", "m7"
	TaskRoleArn pulumi.StringOutput
	ExecRoleArn pulumi.StringOutput
}

// ALBOutputs holds ALB outputs consumed by the DNS module.
type ALBOutputs struct {
	ALBDNSName      pulumi.StringOutput
	ALBHostedZoneID pulumi.StringOutput
}

// MskConfig holds environment-specific MSK cluster configuration.
type MskConfig struct {
	KafkaVersion       string
	BrokerCount        int
	InstanceType       string
	EbsVolumeSize      int
	Environment        string
	EnhancedMonitoring string
}
