// Package streaming provides cloud-agnostic streaming-stage modules.
//
// The package exists alongside infra/pkg/aws/streaming/ (which owns AWS MSK)
// and is the home for streaming providers whose substrate is independent of
// the tenant's chosen cloud — primarily Redpanda Cloud (BYOC).
//
// Deploy() dispatches on cfg.StreamingProvider, not cfg.CloudProvider, so an
// AWS tenant may opt into Redpanda without changing cloud, and a GCP tenant
// can use Redpanda from day one. Both providers populate the shared
// types.StreamingOutputs contract.
//
// The Redpanda implementation here uses the Redpanda Terraform provider via
// Pulumi's TF bridge mechanism: each resource is registered with a TF-bridge
// type token (e.g. "redpanda:index/cluster:Cluster") that matches what a
// generated pulumi-redpanda SDK would emit. Pulumi loads the upstream
// terraform-provider-redpanda plugin at runtime.
package streaming

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/pulumi/pulumi-kafka/sdk/v3/go/kafka"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
	pulumiconfig "github.com/pulumi/pulumi/sdk/v3/go/pulumi/config"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// RedpandaInputs configures Redpanda Cloud cluster provisioning. Fields are
// populated from the "redpanda:" Pulumi config namespace by NewRedpanda(),
// or supplied directly by tests.
type RedpandaInputs struct {
	// ClusterName is the human-readable cluster identifier (e.g. "kaizen-dev").
	ClusterName string
	// Environment is one of "dev", "staging", "prod" — drives sizing defaults.
	Environment string
	// CloudProvider is where Redpanda Cloud spins up the BYOC cluster
	// underneath ("aws" or "gcp"). Independent of the Kaizen tenant's cloud.
	CloudProvider string
	// Region is the cloud region the cluster runs in (e.g. "us-east-1", "us-central1").
	Region string
	// Zones is the list of availability zones the cluster spans.
	Zones []string
	// ThroughputTier selects the SLA tier (e.g. "tier-1-aws-v2-x86" for dev,
	// "tier-3-aws-v3-x86" for prod).
	ThroughputTier string
	// ClusterType is "dedicated" (default) or "byoc".
	ClusterType string
	// ConnectionType is "public" or "private" — Kaizen uses "private"
	// so the cluster lives on the tenant VPC.
	ConnectionType string
	// TenantVpcID is the tenant-side VPC ID (NetworkOutputs.VpcId) that the
	// Redpanda private network peers with. The TF provider does not yet read
	// this field directly, so it is persisted as a tag on the resource group
	// for operator-visible cross-reference between the two VPCs.
	TenantVpcID pulumi.StringInput
	// KafkaUsername is the SASL/SCRAM username the topics provider authenticates with.
	KafkaUsername pulumi.StringInput
	// KafkaPassword is the SASL/SCRAM password matching KafkaUsername.
	KafkaPassword pulumi.StringInput
	// Tags applied to all created resources.
	Tags pulumi.StringMap
}

// NewRedpanda is the entry point Deploy() calls when StreamingProvider == "redpanda".
//
// It reads Redpanda-specific configuration from the "redpanda:" Pulumi config
// namespace, then provisions the resource group, cluster, an admin user, an
// ACL, and the eight Kafka topics matching the MSK inventory. The returned
// types.StreamingOutputs has BootstrapBrokers, SchemaRegistryUrl, and
// ClusterName populated; ClusterArn is intentionally empty (see types.StreamingOutputs).
//
// netOut is consumed for VPC-aware private connectivity: the tenant VPC ID is
// propagated to the Redpanda resource group as a tag so operators can
// cross-reference the two VPCs in Redpanda's private connection type. The
// upstream OAuth client credentials (redpanda:clientId / redpanda:clientSecret)
// are read directly from Pulumi config by the underlying terraform-provider-
// redpanda plugin, so they are not threaded through Go code here.
func NewRedpanda(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.StreamingOutputs, error) {
	rp := pulumiconfig.New(ctx, "redpanda")

	cloudProvider := "aws"
	if v, err := rp.Try("cloudProvider"); err == nil && v != "" {
		cloudProvider = v
	}

	clusterType := "dedicated"
	if v, err := rp.Try("clusterType"); err == nil && v != "" {
		clusterType = v
	}

	connectionType := "private"
	if v, err := rp.Try("connectionType"); err == nil && v != "" {
		connectionType = v
	}

	zonesCSV := rp.Require("zones")
	zones := splitAndTrim(zonesCSV)

	args := &RedpandaInputs{
		ClusterName:    fmt.Sprintf("kaizen-%s", cfg.Environment),
		Environment:    cfg.Environment,
		CloudProvider:  cloudProvider,
		Region:         rp.Require("region"),
		Zones:          zones,
		ThroughputTier: rp.Require("throughputTier"),
		ClusterType:    clusterType,
		ConnectionType: connectionType,
		// netOut.VpcId is an IDOutput; convert to StringInput for tag propagation.
		TenantVpcID:    netOut.VpcId.ToStringOutput(),
		KafkaUsername:  pulumi.String(rp.Require("kafkaUsername")),
		KafkaPassword:  rp.RequireSecret("kafkaPassword"),
		Tags:           kconfig.DefaultTags(cfg.Environment),
	}

	out, err := newRedpanda(ctx, args)
	if err != nil {
		return types.StreamingOutputs{}, err
	}

	ctx.Export("redpandaBootstrapBrokers", out.BootstrapBrokers)
	ctx.Export("redpandaSchemaRegistryUrl", out.SchemaRegistryUrl)
	ctx.Export("redpandaClusterName", out.ClusterName)

	return out, nil
}

// NewRedpandaForTest is the test entry point that bypasses Pulumi config
// loading. Production callers must use NewRedpanda; tests can supply a
// fully-formed RedpandaInputs and run inside pulumi.RunErr.
func NewRedpandaForTest(ctx *pulumi.Context, args *RedpandaInputs) (types.StreamingOutputs, error) {
	return newRedpanda(ctx, args)
}

// newRedpanda is the inputs-driven core, exported within the package for tests
// and callable directly by parameterized integration tests that need to bypass
// Pulumi config loading.
func newRedpanda(ctx *pulumi.Context, args *RedpandaInputs) (types.StreamingOutputs, error) {
	if err := args.validate(); err != nil {
		return types.StreamingOutputs{}, err
	}

	// --- Resource Group: logical container in the Redpanda Cloud account ---
	// Tenant VPC ID (from the upstream NetworkOutputs) is folded into the
	// resource-group tag set so the two VPCs at each side of the private
	// connection peering can be correlated by operators.
	rgTags := args.Tags
	if args.TenantVpcID != nil {
		rgTagsCopy := pulumi.StringMap{}
		for k, v := range rgTags {
			rgTagsCopy[k] = v
		}
		rgTagsCopy["kaizenTenantVpcId"] = args.TenantVpcID
		rgTags = rgTagsCopy
	}
	rg, err := newRedpandaResourceGroup(ctx, args.ClusterName+"-rg", pulumi.Map{
		"name": pulumi.String(args.ClusterName + "-rg"),
		"tags": rgTags,
	})
	if err != nil {
		return types.StreamingOutputs{}, fmt.Errorf("creating Redpanda resource group: %w", err)
	}

	// --- Network: defines the cluster's network in the tenant's chosen cloud ---
	zoneInputs := make(pulumi.StringArray, 0, len(args.Zones))
	for _, z := range args.Zones {
		zoneInputs = append(zoneInputs, pulumi.String(z))
	}
	network, err := newRedpandaNetwork(ctx, args.ClusterName+"-network", pulumi.Map{
		"name":            pulumi.String(args.ClusterName + "-network"),
		"resourceGroupId": rg.ID(),
		"cloudProvider":   pulumi.String(args.CloudProvider),
		"region":          pulumi.String(args.Region),
		"clusterType":     pulumi.String(args.ClusterType),
		"cidrBlock":       pulumi.String(redpandaNetworkCIDR(args.Environment)),
		"tags":            args.Tags,
	})
	if err != nil {
		return types.StreamingOutputs{}, fmt.Errorf("creating Redpanda network: %w", err)
	}

	// --- Cluster: the Kafka-protocol broker pool + built-in Schema Registry ---
	cluster, err := newRedpandaCluster(ctx, args.ClusterName, pulumi.Map{
		"name":            pulumi.String(args.ClusterName),
		"resourceGroupId": rg.ID(),
		"networkId":       network.ID(),
		"cloudProvider":   pulumi.String(args.CloudProvider),
		"region":          pulumi.String(args.Region),
		"zones":           zoneInputs,
		"throughputTier":  pulumi.String(args.ThroughputTier),
		"clusterType":     pulumi.String(args.ClusterType),
		"connectionType":  pulumi.String(args.ConnectionType),
		"tags":            args.Tags,
	})
	if err != nil {
		return types.StreamingOutputs{}, fmt.Errorf("creating Redpanda cluster: %w", err)
	}

	// --- Admin user (SCRAM-SHA-512) ---
	user, err := newRedpandaUser(ctx, args.ClusterName+"-admin", pulumi.Map{
		"name":           args.KafkaUsername,
		"password":       args.KafkaPassword,
		"mechanism":      pulumi.String("SCRAM-SHA-512"),
		"clusterApiUrl":  cluster.ClusterAPIURL,
	}, pulumi.DependsOn([]pulumi.Resource{cluster}))
	if err != nil {
		return types.StreamingOutputs{}, fmt.Errorf("creating Redpanda user: %w", err)
	}

	// --- ACL: grants the admin user full topic+group rights on this cluster ---
	if _, err := newRedpandaACL(ctx, args.ClusterName+"-admin-acl", pulumi.Map{
		"resourceType":        pulumi.String("CLUSTER"),
		"resourceName":        pulumi.String("kafka-cluster"),
		"resourcePatternType": pulumi.String("LITERAL"),
		"principal":           pulumi.Sprintf("User:%s", args.KafkaUsername),
		"host":                pulumi.String("*"),
		"operation":           pulumi.String("ALL"),
		"permissionType":      pulumi.String("ALLOW"),
		"clusterApiUrl":       cluster.ClusterAPIURL,
	}, pulumi.DependsOn([]pulumi.Resource{user})); err != nil {
		return types.StreamingOutputs{}, fmt.Errorf("creating Redpanda ACL: %w", err)
	}

	// --- Topics via the Kafka provider, authenticated with the admin user ---
	bootstrapBrokers := cluster.BootstrapBrokers
	if err := provisionKafkaTopics(ctx, args.ClusterName+"-topics", &kafkaTopicsArgs{
		BootstrapBrokers: bootstrapBrokers,
		SaslUsername:     args.KafkaUsername,
		SaslPassword:     args.KafkaPassword,
		DependsOn:        []pulumi.Resource{user},
	}); err != nil {
		return types.StreamingOutputs{}, fmt.Errorf("creating Redpanda topics: %w", err)
	}

	return types.StreamingOutputs{
		BootstrapBrokers: bootstrapBrokers,
		// Redpanda has no separate unauthenticated listener; reuse the
		// SASL brokers so the field is never a zero-valued Output when
		// compute plumbs it into KAFKA_BROKERS.
		BootstrapBrokersPlaintext: bootstrapBrokers,
		SchemaRegistryUrl:         cluster.SchemaRegistryURL,
		ClusterArn:                pulumi.String("").ToStringOutput(), // Empty for Redpanda; documented in types.StreamingOutputs.
		ClusterName:               cluster.Name,
	}, nil
}

func (a *RedpandaInputs) validate() error {
	if a.ClusterName == "" {
		return fmt.Errorf("RedpandaInputs.ClusterName is required")
	}
	if a.Environment == "" {
		return fmt.Errorf("RedpandaInputs.Environment is required")
	}
	if a.Region == "" {
		return fmt.Errorf("RedpandaInputs.Region is required")
	}
	if len(a.Zones) == 0 {
		return fmt.Errorf("RedpandaInputs.Zones must contain at least one zone")
	}
	if a.ThroughputTier == "" {
		return fmt.Errorf("RedpandaInputs.ThroughputTier is required")
	}
	if a.ClusterType == "" {
		a.ClusterType = "dedicated"
	}
	if a.ConnectionType == "" {
		a.ConnectionType = "private"
	}
	return nil
}

// redpandaNetworkCIDR returns a non-overlapping CIDR for each environment
// so dev/staging/prod Redpanda networks can co-exist in the same Redpanda
// Cloud account without peering conflicts.
func redpandaNetworkCIDR(env string) string {
	switch env {
	case "prod":
		return "10.20.0.0/20"
	case "staging":
		return "10.21.0.0/20"
	default:
		return "10.22.0.0/20"
	}
}

func splitAndTrim(csv string) []string {
	parts := strings.Split(csv, ",")
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if t := strings.TrimSpace(p); t != "" {
			out = append(out, t)
		}
	}
	return out
}

// ─── TF-bridge resource wrappers ────────────────────────────────────────────
//
// Each Redpanda Terraform resource is exposed via a small Go type that
// embeds pulumi.CustomResourceState. The type token matches what a
// generated pulumi-redpanda SDK would emit (the bridge convention is
// "<provider>:index/<lowercase>:<TitleCase>"). Pulumi loads the upstream
// terraform-provider-redpanda plugin at runtime to satisfy these types.

type redpandaResourceGroup struct {
	pulumi.CustomResourceState

	Name pulumi.StringOutput `pulumi:"name"`
}

func newRedpandaResourceGroup(ctx *pulumi.Context, name string, args pulumi.Map, opts ...pulumi.ResourceOption) (*redpandaResourceGroup, error) {
	var r redpandaResourceGroup
	if err := ctx.RegisterResource("redpanda:index/resourceGroup:ResourceGroup", name, args, &r, opts...); err != nil {
		return nil, err
	}
	return &r, nil
}

type redpandaNetwork struct {
	pulumi.CustomResourceState

	Name      pulumi.StringOutput `pulumi:"name"`
	CidrBlock pulumi.StringOutput `pulumi:"cidrBlock"`
}

func newRedpandaNetwork(ctx *pulumi.Context, name string, args pulumi.Map, opts ...pulumi.ResourceOption) (*redpandaNetwork, error) {
	var r redpandaNetwork
	if err := ctx.RegisterResource("redpanda:index/network:Network", name, args, &r, opts...); err != nil {
		return nil, err
	}
	return &r, nil
}

type redpandaCluster struct {
	pulumi.CustomResourceState

	Name              pulumi.StringOutput `pulumi:"name"`
	BootstrapBrokers  pulumi.StringOutput `pulumi:"bootstrapBrokers"`
	SchemaRegistryURL pulumi.StringOutput `pulumi:"schemaRegistryUrl"`
	ClusterAPIURL     pulumi.StringOutput `pulumi:"clusterApiUrl"`
}

func newRedpandaCluster(ctx *pulumi.Context, name string, args pulumi.Map, opts ...pulumi.ResourceOption) (*redpandaCluster, error) {
	var r redpandaCluster
	if err := ctx.RegisterResource("redpanda:index/cluster:Cluster", name, args, &r, opts...); err != nil {
		return nil, err
	}
	return &r, nil
}

type redpandaUser struct {
	pulumi.CustomResourceState

	Name pulumi.StringOutput `pulumi:"name"`
}

func newRedpandaUser(ctx *pulumi.Context, name string, args pulumi.Map, opts ...pulumi.ResourceOption) (*redpandaUser, error) {
	var r redpandaUser
	if err := ctx.RegisterResource("redpanda:index/user:User", name, args, &r, opts...); err != nil {
		return nil, err
	}
	return &r, nil
}

type redpandaACL struct {
	pulumi.CustomResourceState

	Principal pulumi.StringOutput `pulumi:"principal"`
}

func newRedpandaACL(ctx *pulumi.Context, name string, args pulumi.Map, opts ...pulumi.ResourceOption) (*redpandaACL, error) {
	var r redpandaACL
	if err := ctx.RegisterResource("redpanda:index/acl:Acl", name, args, &r, opts...); err != nil {
		return nil, err
	}
	return &r, nil
}

// ─── Topics ─────────────────────────────────────────────────────────────────

// kafkaTopicsArgs lets both AWS MSK and Redpanda Cloud paths reuse a single
// Kafka-provider topic provisioning routine. The MSK path currently uses its
// own copy in infra/pkg/aws/streaming/topics.go; the spec list here is
// intentionally identical and is enforced by topology test parity.
type kafkaTopicsArgs struct {
	BootstrapBrokers pulumi.StringInput
	SaslUsername     pulumi.StringInput
	SaslPassword     pulumi.StringInput
	DependsOn        []pulumi.Resource
}

// provisionKafkaTopics creates the eight Kafka topics matching the MSK
// inventory. Topic specs MUST match infra/pkg/aws/streaming/topics.go
// byte-for-byte; topology test enforces this invariant.
func provisionKafkaTopics(ctx *pulumi.Context, providerName string, args *kafkaTopicsArgs) error {
	provider, err := kafka.NewProvider(ctx, providerName, &kafka.ProviderArgs{
		BootstrapServers: args.BootstrapBrokers.ToStringOutput().ApplyT(func(brokers string) []string {
			return splitAndTrim(brokers)
		}).(pulumi.StringArrayOutput),
		TlsEnabled:    pulumi.BoolPtr(true),
		SaslMechanism: pulumi.StringPtr("scram-sha512"),
		SaslUsername:  args.SaslUsername,
		SaslPassword:  args.SaslPassword,
	})
	if err != nil {
		return fmt.Errorf("creating Kafka provider for Redpanda: %w", err)
	}

	for _, spec := range RedpandaTopicSpecs() {
		opts := []pulumi.ResourceOption{pulumi.Provider(provider)}
		if len(args.DependsOn) > 0 {
			opts = append(opts, pulumi.DependsOn(args.DependsOn))
		}
		if _, err := kafka.NewTopic(ctx, spec.Name, &kafka.TopicArgs{
			Name:              pulumi.String(spec.Name),
			Partitions:        pulumi.Int(spec.Partitions),
			ReplicationFactor: pulumi.Int(redpandaReplicationFactor),
			Config: pulumi.StringMap{
				"retention.ms":        pulumi.String(strconv.FormatInt(spec.RetentionMs, 10)),
				"cleanup.policy":      pulumi.String(redpandaCleanupPolicy),
				"compression.type":    pulumi.String(redpandaCompressionType),
				"segment.bytes":       pulumi.String(strconv.Itoa(spec.SegmentBytes)),
				"max.message.bytes":   pulumi.String(strconv.Itoa(spec.MaxMessageBytes)),
				"min.insync.replicas": pulumi.String(strconv.Itoa(redpandaMinInsyncReplicas)),
			},
		}, opts...); err != nil {
			return fmt.Errorf("creating topic %q: %w", spec.Name, err)
		}
	}
	return nil
}

// TopicSpec is the cross-provider topic specification. Both Redpanda Cloud
// and AWS MSK provisioning paths consume this exact list.
type TopicSpec struct {
	Name            string
	Partitions      int
	RetentionMs     int64
	SegmentBytes    int
	MaxMessageBytes int
}

// Topic-config invariants shared across providers (replication, cleanup,
// compression, ISR). These mirror infra/pkg/aws/streaming/topics.go;
// changes must land in lockstep.
const (
	redpandaReplicationFactor = 3
	redpandaMinInsyncReplicas = 2
	redpandaCleanupPolicy     = "delete"
	redpandaCompressionType   = "lz4"
)

// Retention constants in milliseconds. Mirrors infra/pkg/aws/streaming/topics.go.
const (
	redpandaRetention30d  int64 = 2_592_000_000
	redpandaRetention90d  int64 = 7_776_000_000
	redpandaRetention180d int64 = 15_552_000_000
)

// Segment size constants in bytes. Mirrors infra/pkg/aws/streaming/topics.go.
const (
	redpandaSegment1GB   = 1_073_741_824
	redpandaSegment512MB = 536_870_912
	redpandaSegment256MB = 268_435_456
)

const redpandaMaxMessageBytes = 1_048_576

// RedpandaTopicSpecs returns the canonical eight-topic inventory matching
// kafka/topic_configs.sh and infra/pkg/aws/streaming/topics.go. Exposed as a
// function (not a package var) to prevent caller mutation.
func RedpandaTopicSpecs() []TopicSpec {
	return []TopicSpec{
		// High-volume event topics (M2 → M3/M4a)
		{Name: "exposures", Partitions: 64, RetentionMs: redpandaRetention90d, SegmentBytes: redpandaSegment1GB, MaxMessageBytes: redpandaMaxMessageBytes},
		{Name: "metric_events", Partitions: 128, RetentionMs: redpandaRetention90d, SegmentBytes: redpandaSegment1GB, MaxMessageBytes: redpandaMaxMessageBytes},
		{Name: "reward_events", Partitions: 32, RetentionMs: redpandaRetention180d, SegmentBytes: redpandaSegment512MB, MaxMessageBytes: redpandaMaxMessageBytes},
		{Name: "qoe_events", Partitions: 64, RetentionMs: redpandaRetention90d, SegmentBytes: redpandaSegment1GB, MaxMessageBytes: redpandaMaxMessageBytes},

		// Alert topics (low-volume, low-latency)
		{Name: "guardrail_alerts", Partitions: 8, RetentionMs: redpandaRetention30d, SegmentBytes: redpandaSegment256MB, MaxMessageBytes: redpandaMaxMessageBytes},
		{Name: "sequential_boundary_alerts", Partitions: 8, RetentionMs: redpandaRetention30d, SegmentBytes: redpandaSegment256MB, MaxMessageBytes: redpandaMaxMessageBytes},

		// Operational topics
		{Name: "model_retraining_events", Partitions: 8, RetentionMs: redpandaRetention180d, SegmentBytes: redpandaSegment256MB, MaxMessageBytes: redpandaMaxMessageBytes},
		{Name: "surrogate_recalibration_requests", Partitions: 4, RetentionMs: redpandaRetention30d, SegmentBytes: redpandaSegment256MB, MaxMessageBytes: redpandaMaxMessageBytes},
	}
}
