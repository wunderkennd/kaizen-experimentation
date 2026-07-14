package config

import (
	"fmt"
	"strings"

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
	// ManageKafkaTopics gates the declarative kafka:Topic resources. The
	// Kafka provider needs direct broker connectivity, which only exists
	// from inside the VPC — set false (dev) to skip topic resources and
	// enable MSK auto-create instead. Defaults true.
	ManageKafkaTopics bool
	// MskAllowPlaintext opens MSK's unauthenticated PLAINTEXT listener
	// (9092) alongside SASL/SCRAM. Dev-only: app services were built
	// against plaintext Kafka and carry no SASL/TLS client wiring yet.
	// Defaults false.
	MskAllowPlaintext bool

	// Cache
	RedisNodeType string

	// Compute
	M4bInstanceType string
	FargateMinTasks int

	// Observability
	CloudwatchRetention int

	// Security
	WafEnabled bool
	// GrafanaEnabled gates the Amazon Managed Grafana workspace, which
	// hard-requires IAM Identity Center (SSO). Optional; defaults true.
	GrafanaEnabled      bool
	WafBlockedCountries []string
	WafRateLimitPerIP   int

	// Provider routing — read by Deploy()'s switch dispatch.
	// Defaults preserve current AWS-only behavior when stack config omits them.
	CloudProvider     string // "aws" (default) or "gcp"
	StreamingProvider string // "msk" (default) or "redpanda"

	// GCP-specific fields. Required when CloudProvider="gcp", ignored otherwise.
	// Reading these via Try() (not Require()) keeps existing AWS stacks
	// byte-for-byte identical — they don't have to declare these.
	GCPProjectID  string // e.g. "kaizen-experimentation-dev"
	GCPRegion     string // e.g. "us-central1" — used for regional resources later
	GCPARLocation string // Artifact Registry location, e.g. "us" (multi-region) or "us-central1"
	// GCPCIPushPrincipal is the IAM principal CI uses to push images, e.g.
	// "serviceAccount:kaizen-ci-push@<project>.iam.gserviceaccount.com" or a
	// Workload Identity principalSet. Empty means "skip the IAM binding" —
	// useful during bootstrap before the SA exists.
	GCPCIPushPrincipal string
	// GCPRunPullPrincipals lists Cloud Run runtime SAs that need to pull from
	// AR. Comma-separated in stack config. Empty means "skip".
	GCPRunPullPrincipals []string
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

	// WAF optional settings.
	var blockedCountries []string
	if v, err := cfg.Try("wafBlockedCountries"); err == nil && v != "" {
		for _, c := range strings.Split(v, ",") {
			c = strings.TrimSpace(c)
			if c != "" {
				blockedCountries = append(blockedCountries, c)
			}
		}
	}

	wafRateLimit := 1000 // default: 1000 requests per 5-minute window
	if v, err := cfg.TryInt("wafRateLimitPerIP"); err == nil {
		wafRateLimit = v
	}

	// Provider routing — default to AWS / MSK to preserve existing behavior.
	cloudProvider := "aws"
	if v, err := cfg.Try("cloudProvider"); err == nil && v != "" {
		cloudProvider = v
	}
	streamingProvider := "msk"
	if v, err := cfg.Try("streamingProvider"); err == nil && v != "" {
		streamingProvider = v
	}

	// GCP fields — all optional at the config layer; the GCP facade enforces
	// presence when cloudProvider="gcp".
	gcpProjectID, _ := cfg.Try("gcpProjectId")
	gcpRegion, _ := cfg.Try("gcpRegion")
	gcpARLocation, _ := cfg.Try("gcpArLocation")
	gcpCIPush, _ := cfg.Try("gcpCiPushPrincipal")

	var gcpPullPrincipals []string
	if v, err := cfg.Try("gcpRunPullPrincipals"); err == nil && v != "" {
		for _, p := range strings.Split(v, ",") {
			p = strings.TrimSpace(p)
			if p != "" {
				gcpPullPrincipals = append(gcpPullPrincipals, p)
			}
		}
	}

	out := &Config{
		Project:              "kaizen",
		Environment:          env,
		Env:                  Environment(env),
		Domain:               domain,
		ProjectName:          projectName,
		WafBlockedCountries:  blockedCountries,
		WafRateLimitPerIP:    wafRateLimit,
		CloudProvider:        cloudProvider,
		StreamingProvider:    streamingProvider,
		GCPProjectID:         gcpProjectID,
		GCPRegion:            gcpRegion,
		GCPARLocation:        gcpARLocation,
		GCPCIPushPrincipal:   gcpCIPush,
		GCPRunPullPrincipals: gcpPullPrincipals,
	}

	// AWS-specific stack config. Required when targeting AWS so existing
	// stacks behave identically; ignored under cloudProvider=gcp where these
	// fields have no meaning. The AWS facade is what reads them, and it only
	// runs from Deploy() when cfg.CloudProvider=="aws".
	if cloudProvider == "aws" {
		out.VpcCidr = cfg.Require("vpcCidr")
		out.RdsInstanceClass = cfg.Require("rdsInstanceClass")
		out.RdsMultiAz = cfg.RequireBool("rdsMultiAz")
		out.MskBrokerCount = cfg.RequireInt("mskBrokerCount")
		out.MskInstanceType = cfg.Require("mskInstanceType")
		out.RedisNodeType = cfg.Require("redisNodeType")
		out.M4bInstanceType = cfg.Require("m4bInstanceType")
		out.NatGatewayCount = cfg.RequireInt("natGatewayCount")
		out.WafEnabled = cfg.RequireBool("wafEnabled")
		out.GrafanaEnabled = true
		if v, err := cfg.TryBool("grafanaEnabled"); err == nil {
			out.GrafanaEnabled = v
		}
		out.ManageKafkaTopics = true
		if v, err := cfg.TryBool("manageKafkaTopics"); err == nil {
			out.ManageKafkaTopics = v
		}
		out.MskAllowPlaintext = false
		if v, err := cfg.TryBool("mskAllowPlaintext"); err == nil {
			out.MskAllowPlaintext = v
		}
		if out.MskAllowPlaintext && out.IsProd() {
			// Same failure mode as a missing Require key: refuse to build
			// a prod plan with the dev-only plaintext listener enabled.
			panic("mskAllowPlaintext=true is dev-only: prod keeps TLS+SASL — remove the config key from the prod stack")
		}
		out.FargateMinTasks = cfg.RequireInt("fargateMinTasks")
		out.CloudwatchRetention = cfg.RequireInt("cloudwatchRetentionDays")
	} else {
		// Soft reads for non-AWS providers — keep zero-values when missing
		// so a misconfigured stack fails in the AWS facade (with a clear
		// "Require..." panic) instead of silently here.
		if v, err := cfg.Try("vpcCidr"); err == nil {
			out.VpcCidr = v
		}
		// wafEnabled gates Cloud Armor on GCP exactly as it gates WAF v2 on
		// AWS (charter parity: the kaizen:enableWaf toggle spans providers).
		if v, err := cfg.TryBool("wafEnabled"); err == nil {
			out.WafEnabled = v
		}
		if v, err := cfg.TryInt("natGatewayCount"); err == nil {
			out.NatGatewayCount = v
		}
		if v, err := cfg.TryInt("fargateMinTasks"); err == nil {
			out.FargateMinTasks = v
		}
		if v, err := cfg.TryInt("cloudwatchRetentionDays"); err == nil {
			out.CloudwatchRetention = v
		}
	}

	return out
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
	// MskBootstrapBrokersPlaintext is the unauthenticated 9092 listener
	// list — populated only when MskConfig.AllowPlaintext is set.
	MskBootstrapBrokersPlaintext pulumi.StringOutput
	SchemaRegistryUrl            pulumi.StringOutput
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
	// AutoCreateTopics sets auto.create.topics.enable on the cluster.
	// True only when declarative topic management is disabled (dev).
	AutoCreateTopics bool
	// AllowPlaintext opens the unauthenticated PLAINTEXT listener (9092)
	// alongside SASL/SCRAM by switching client-broker encryption to
	// TLS_PLAINTEXT. Dev-only.
	AllowPlaintext bool
}
