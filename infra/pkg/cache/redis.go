// Package cache provides Pulumi modules for managed cache resources.
package cache

import (
	"github.com/pulumi/pulumi-aws/sdk/v7/go/aws/elasticache"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// RedisConfig holds configuration for the ElastiCache Redis replication group.
type RedisConfig struct {
	// NodeType is the ElastiCache node instance type.
	// Default: "cache.r6g.large" (prod/staging); override to "cache.t4g.medium" for dev.
	NodeType string

	// NumCacheClusters is the total number of cache clusters (primary + replicas).
	// Default: 2 (1 primary + 1 replica).
	NumCacheClusters int

	// EngineVersion is the Redis engine version. Default: "7.0".
	EngineVersion string

	// SubnetIds for the Redis subnet group.
	// Placeholder — wired to VPC private subnet outputs in Sprint I.1.
	SubnetIds pulumi.StringArrayInput

	// SecurityGroupIds for the replication group.
	// Placeholder — wired to "redis" security group in Sprint I.1.
	SecurityGroupIds pulumi.StringArrayInput

	// Tags applied to all resources created by this module.
	Tags pulumi.StringMapInput
}

// RedisOutputs contains the outputs exported by the ElastiCache Redis module.
// These feed into DatabaseOutputs.RedisEndpoint and DatabaseOutputs.RedisPort
// when wired in main.go.
type RedisOutputs struct {
	RedisEndpoint      pulumi.StringOutput
	RedisPort          pulumi.IntOutput
	ReplicationGroupId pulumi.StringOutput
	SubnetGroupName    pulumi.StringOutput
}

// NewRedis creates an ElastiCache Redis 7 replication group with:
//   - 1 primary + 1 replica (configurable via NumCacheClusters)
//   - Encryption at rest and in transit
//   - Automatic failover and Multi-AZ
//   - Subnet group placeholder (wired in Sprint I.1)
//
// This is a code-only module for Sprint I.0. VPC wiring happens in Sprint I.1.
func NewRedis(ctx *pulumi.Context, name string, cfg *RedisConfig) (*RedisOutputs, error) {
	if cfg.NodeType == "" {
		cfg.NodeType = "cache.r6g.large"
	}
	if cfg.NumCacheClusters == 0 {
		cfg.NumCacheClusters = 2
	}
	if cfg.EngineVersion == "" {
		cfg.EngineVersion = "7.0"
	}

	// --- Subnet group (placeholder for Sprint I.1 VPC wiring) ---
	subnetGroup, err := elasticache.NewSubnetGroup(ctx, name+"-subnet-group", &elasticache.SubnetGroupArgs{
		Description: pulumi.Sprintf("Kaizen Redis subnet group — %s", name),
		SubnetIds:   cfg.SubnetIds,
		Tags:        cfg.Tags,
	})
	if err != nil {
		return nil, err
	}

	// --- Replication group: 1 primary + 1 replica, encryption at rest + in transit ---
	rg, err := elasticache.NewReplicationGroup(ctx, name, &elasticache.ReplicationGroupArgs{
		Description: pulumi.String("Kaizen experimentation platform — Redis cache"),

		// Engine
		Engine:        pulumi.String("redis"),
		EngineVersion: pulumi.String(cfg.EngineVersion),
		NodeType:      pulumi.String(cfg.NodeType),

		// Topology: 1 primary + N-1 replicas
		NumCacheClusters:         pulumi.Int(cfg.NumCacheClusters),
		AutomaticFailoverEnabled: pulumi.Bool(cfg.NumCacheClusters > 1),
		MultiAzEnabled:           pulumi.Bool(cfg.NumCacheClusters > 1),

		// Encryption
		AtRestEncryptionEnabled:  pulumi.Bool(true),
		TransitEncryptionEnabled: pulumi.Bool(true),

		// Networking (placeholders wired in Sprint I.1)
		SubnetGroupName:  subnetGroup.Name,
		SecurityGroupIds: cfg.SecurityGroupIds,
		Port:             pulumi.Int(6379),

		// Parameters
		ParameterGroupName: pulumi.String("default.redis7"),

		// Maintenance & snapshots
		MaintenanceWindow:      pulumi.String("sun:05:00-sun:07:00"),
		SnapshotRetentionLimit: pulumi.Int(7),
		SnapshotWindow:         pulumi.String("03:00-05:00"),
		ApplyImmediately:       pulumi.Bool(false),

		Tags: cfg.Tags,
	})
	if err != nil {
		return nil, err
	}

	return &RedisOutputs{
		RedisEndpoint:      rg.PrimaryEndpointAddress,
		RedisPort:          pulumi.Int(6379).ToIntOutput(),
		ReplicationGroupId: rg.ID().ToStringOutput(),
		SubnetGroupName:    subnetGroup.Name,
	}, nil
}
