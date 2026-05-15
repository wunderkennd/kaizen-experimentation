// Package cache provisions the Kaizen Memorystore Redis instance. The
// module mirrors the public surface of pkg/aws/cache (NewRedis) so the
// per-cloud facade (pkg/gcp/gcp.go NewCache) can dispatch on cloudProvider
// without per-provider input wiring.
//
// HA is enabled (Tier=STANDARD_HA) with one read replica, AUTH is enabled
// (GCP generates the auth token at create-time and surfaces it as the
// AuthString output), transit encryption is required, and reachability is
// private-only via PRIVATE_SERVICE_ACCESS connect mode — Cloud Run reaches
// the instance through the Serverless VPC Access connector wired in
// pkg/gcp/network/vpc_connector.go.
//
// The auth token is *generated* by Memorystore, not set by the caller —
// the secrets module (pkg/gcp/secrets) is responsible for writing the
// AuthString into Secret Manager so that downstream services consume it
// via SecretsOutputs.RedisSecretRef. The cache module exposes AuthString
// on its RedisOutputs so a follow-up wiring PR can flow it through.
package cache

import (
	"fmt"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/redis"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// RedisConfig holds configuration for the Memorystore Redis instance.
// Defaults mirror the AWS ElastiCache module: Redis 7, 1 primary + 1
// replica, 5 GB memory, HA enabled.
type RedisConfig struct {
	// Name is the bare instance name (GCP requires [a-z]([-a-z0-9]*[a-z0-9])?).
	// Defaults to "kaizen-redis".
	Name string

	// Region is the regional placement (e.g. "us-central1"). Required.
	Region pulumi.StringInput

	// AuthorizedNetwork is the VPC network self-link the instance peers with
	// for private connectivity. Required — passing an empty input forces
	// Memorystore to use the project's default network, which would defeat
	// the private-only acceptance criterion.
	AuthorizedNetwork pulumi.StringInput

	// MemorySizeGb is the Redis memory budget in GiB. Defaults to 5.
	MemorySizeGb int

	// ReplicaCount is the number of read replicas. Defaults to 1 (single
	// replica → automatic failover; total of 1 primary + 1 replica matches
	// the AWS module's NumCacheClusters=2 default).
	ReplicaCount int

	// RedisVersion is the Memorystore Redis version, e.g. "REDIS_7_2".
	// Defaults to "REDIS_7_2" to match the AWS module's "7.0"/"7.x" line.
	RedisVersion string

	// Labels are GCP resource labels (the GCP analogue of AWS tags). GCP
	// label values are restricted to [a-z0-9_-]; callers must pre-sanitize.
	Labels pulumi.StringMapInput
}

// RedisOutputs contains the internal outputs exported by the Memorystore
// module. The facade NewCache (pkg/gcp/gcp.go) narrows this to
// types.CacheOutputs (just Endpoint) for cross-provider parity. AuthString
// is intentionally surfaced here so a follow-up secrets wiring PR can pipe
// the GCP-generated AUTH token into Secret Manager.
type RedisOutputs struct {
	// Endpoint is the fully-qualified connection URL: redis://<host>:<port>.
	// Format chosen to match the issue spec; the AWS facade returns the bare
	// host because that's what AWS PrimaryEndpointAddress already provides.
	Endpoint pulumi.StringOutput

	// Host is the primary IP (private, by PRIVATE_SERVICE_ACCESS contract).
	Host pulumi.StringOutput

	// Port is the listener port (6379 in practice; surfaced as IntOutput
	// because Memorystore returns it as an output, not a known constant).
	Port pulumi.IntOutput

	// AuthString is the GCP-generated AUTH token. Sensitive. Consumed by
	// the secrets module so SecretsOutputs.RedisSecretRef carries the live
	// password.
	AuthString pulumi.StringOutput

	// InstanceId is the cloud-native instance identifier
	// (projects/<P>/locations/<R>/instances/<N>).
	InstanceId pulumi.StringOutput
}

// NewRedis creates the Memorystore Redis instance:
//   - Tier=STANDARD_HA: zonal failover with one+ replica
//   - AuthEnabled=true: GCP generates an AUTH token
//   - TransitEncryptionMode=SERVER_AUTHENTICATION: TLS to the primary
//   - ConnectMode=PRIVATE_SERVICE_ACCESS: private IP via VPC peering — the
//     instance has no public endpoint and is only reachable through the
//     Serverless VPC Access connector or in-VPC clients
func NewRedis(ctx *pulumi.Context, cfg *RedisConfig) (*RedisOutputs, error) {
	if cfg == nil {
		return nil, fmt.Errorf("gcp/cache: RedisConfig must not be nil")
	}
	if cfg.Region == nil {
		return nil, fmt.Errorf("gcp/cache: RedisConfig.Region is required")
	}
	if cfg.AuthorizedNetwork == nil {
		return nil, fmt.Errorf("gcp/cache: RedisConfig.AuthorizedNetwork is required (must be the VPC network self-link)")
	}

	name := cfg.Name
	if name == "" {
		name = "kaizen-redis"
	}
	memGb := cfg.MemorySizeGb
	if memGb == 0 {
		memGb = 5
	}
	replicas := cfg.ReplicaCount
	if replicas == 0 {
		replicas = 1
	}
	version := cfg.RedisVersion
	if version == "" {
		version = "REDIS_7_2"
	}

	inst, err := redis.NewInstance(ctx, name, &redis.InstanceArgs{
		Name:         pulumi.String(name),
		DisplayName:  pulumi.String("Kaizen experimentation Memorystore Redis"),
		Region:       cfg.Region,
		Tier:         pulumi.String("STANDARD_HA"),
		MemorySizeGb: pulumi.Int(memGb),
		RedisVersion: pulumi.String(version),

		// Private-IP only: VPC peering reachable through the Serverless
		// VPC Access connector. Public endpoints are not provisioned.
		AuthorizedNetwork: cfg.AuthorizedNetwork,
		ConnectMode:       pulumi.String("PRIVATE_SERVICE_ACCESS"),

		// AUTH + TLS. AuthString is GCP-generated and surfaced as an output.
		AuthEnabled:           pulumi.Bool(true),
		TransitEncryptionMode: pulumi.String("SERVER_AUTHENTICATION"),

		// HA: enable read replicas so the second node serves as a hot
		// standby for automatic failover.
		ReadReplicasMode: pulumi.String("READ_REPLICAS_ENABLED"),
		ReplicaCount:     pulumi.Int(replicas),

		Labels: cfg.Labels,
	})
	if err != nil {
		return nil, fmt.Errorf("create Memorystore Redis instance: %w", err)
	}

	endpoint := pulumi.All(inst.Host, inst.Port).ApplyT(func(args []interface{}) string {
		host, _ := args[0].(string)
		port, _ := args[1].(int)
		return fmt.Sprintf("redis://%s:%d", host, port)
	}).(pulumi.StringOutput)

	return &RedisOutputs{
		Endpoint:   endpoint,
		Host:       inst.Host,
		Port:       inst.Port,
		AuthString: inst.AuthString,
		InstanceId: inst.ID().ToStringOutput(),
	}, nil
}
