// Package config provides shared configuration types and output structs
// for the Kaizen experimentation platform infrastructure.
//
// This is a minimal scaffold for Sprint I.0. Infra-2 (I.0.1) will expand
// this with full Pulumi config reading and stack-specific defaults.
package config

import "github.com/pulumi/pulumi/sdk/v3/go/pulumi"

// StreamingOutputs are exported by the streaming module for downstream consumers.
type StreamingOutputs struct {
	MskClusterArn       pulumi.StringOutput
	MskBootstrapBrokers pulumi.StringOutput
	SchemaRegistryUrl   pulumi.StringOutput
}

// MskConfig holds environment-specific MSK cluster settings.
type MskConfig struct {
	// BrokerCount is the number of MSK broker nodes. Must match or be a
	// multiple of the number of subnets provided. Default: 3.
	BrokerCount int
	// InstanceType is the MSK broker instance type. Default: "kafka.m5.large".
	InstanceType string
	// EbsVolumeSize is the EBS volume size in GB per broker. Default: 100.
	EbsVolumeSize int
	// KafkaVersion is the Apache Kafka version. Default: "3.6.0".
	KafkaVersion string
	// EnhancedMonitoring level. One of DEFAULT, PER_BROKER,
	// PER_TOPIC_PER_BROKER, PER_TOPIC_PER_PARTITION. Default: "PER_BROKER".
	EnhancedMonitoring string
	// Environment name (dev, staging, prod). Controls monitoring and cost defaults.
	Environment string
}

// DefaultMskConfig returns production-ready defaults.
func DefaultMskConfig() MskConfig {
	return MskConfig{
		BrokerCount:        3,
		InstanceType:       "kafka.m5.large",
		EbsVolumeSize:      100,
		KafkaVersion:       "3.6.0",
		EnhancedMonitoring: "PER_BROKER",
		Environment:        "dev",
	}
}
