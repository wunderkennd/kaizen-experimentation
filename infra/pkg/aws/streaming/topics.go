// Package streaming provides Pulumi modules for Kafka topic provisioning
// on Amazon MSK. Topics match the canonical definitions in kafka/topic_configs.sh.
package streaming

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/pulumi/pulumi-kafka/sdk/v3/go/kafka"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// topicSpec defines the per-topic configuration derived from kafka/topic_configs.sh.
type topicSpec struct {
	Name            string
	Partitions      int
	RetentionMs     int64 // retention.ms in milliseconds
	SegmentBytes    int   // segment.bytes
	MaxMessageBytes int   // max.message.bytes
}

// Common settings applied to every topic (task requirement + topic_configs.sh).
const (
	replicationFactor = 3
	minInsyncReplicas = 2
	cleanupPolicy     = "delete"
	compressionType   = "lz4"
)

// Retention constants in milliseconds.
const (
	retention30d  int64 = 2_592_000_000  // 30 days
	retention90d  int64 = 7_776_000_000  // 90 days
	retention180d int64 = 15_552_000_000 // 180 days
)

// Segment size constants in bytes.
const (
	segment1GB   = 1_073_741_824 // 1 GB
	segment512MB = 536_870_912   // 512 MB
	segment256MB = 268_435_456   // 256 MB
)

// maxMessageBytes is 1 MB, consistent across all topics.
const maxMessageBytes = 1_048_576

// topics defines the 8 experimentation platform Kafka topics.
// Each entry matches kafka/topic_configs.sh exactly.
var topics = []topicSpec{
	// High-volume event topics (M2 → M3/M4a)
	{Name: "exposures", Partitions: 64, RetentionMs: retention90d, SegmentBytes: segment1GB, MaxMessageBytes: maxMessageBytes},
	{Name: "metric_events", Partitions: 128, RetentionMs: retention90d, SegmentBytes: segment1GB, MaxMessageBytes: maxMessageBytes},
	{Name: "reward_events", Partitions: 32, RetentionMs: retention180d, SegmentBytes: segment512MB, MaxMessageBytes: maxMessageBytes},
	{Name: "qoe_events", Partitions: 64, RetentionMs: retention90d, SegmentBytes: segment1GB, MaxMessageBytes: maxMessageBytes},

	// Alert topics (low-volume, low-latency)
	{Name: "guardrail_alerts", Partitions: 8, RetentionMs: retention30d, SegmentBytes: segment256MB, MaxMessageBytes: maxMessageBytes},
	{Name: "sequential_boundary_alerts", Partitions: 8, RetentionMs: retention30d, SegmentBytes: segment256MB, MaxMessageBytes: maxMessageBytes},

	// Operational topics
	{Name: "model_retraining_events", Partitions: 8, RetentionMs: retention180d, SegmentBytes: segment256MB, MaxMessageBytes: maxMessageBytes},
	{Name: "surrogate_recalibration_requests", Partitions: 4, RetentionMs: retention30d, SegmentBytes: segment256MB, MaxMessageBytes: maxMessageBytes},
}

// TopicsArgs holds the inputs for Kafka topic provisioning.
type TopicsArgs struct {
	// BootstrapBrokers is the comma-separated MSK SASL_SSL bootstrap broker string.
	BootstrapBrokers pulumi.StringInput
	// SaslUsername for SCRAM-SHA-512 authentication against MSK.
	SaslUsername pulumi.StringInput
	// SaslPassword for SCRAM-SHA-512 authentication against MSK.
	SaslPassword pulumi.StringInput
	// KafkaVersion matches the MSK cluster's Kafka version (e.g. "3.5.1").
	KafkaVersion string
}

// TopicsOutputs holds references to all created Kafka topic resources.
type TopicsOutputs struct {
	// Topics maps topic name to its Pulumi resource for downstream references.
	Topics map[string]*kafka.Topic
}

// NewTopics provisions the 8 experimentation platform Kafka topics against
// the MSK cluster identified by the bootstrap brokers in args.
//
// Each topic's partition count, retention, segment size, and message size
// exactly match kafka/topic_configs.sh. All topics share replication factor 3,
// min.insync.replicas 2, cleanup.policy delete, and compression.type lz4.
//
// The Kafka provider is created internally so that topic resources depend on
// the MSK cluster being available. In Sprint I.0 this module is code-only;
// it will be wired to MSK outputs in Sprint I.1.
func NewTopics(ctx *pulumi.Context, args *TopicsArgs) (*TopicsOutputs, error) {
	// Create a Kafka provider that targets the MSK cluster via SASL_SSL.
	// MSK SASL/SCRAM bootstrap brokers come as "b-1:9096,b-2:9096,b-3:9096".
	provider, err := kafka.NewProvider(ctx, "kaizen-kafka", &kafka.ProviderArgs{
		BootstrapServers: args.BootstrapBrokers.ToStringOutput().ApplyT(func(brokers string) []string {
			return strings.Split(brokers, ",")
		}).(pulumi.StringArrayOutput),
		TlsEnabled:    pulumi.BoolPtr(true),
		SaslMechanism: pulumi.StringPtr("scram-sha512"),
		SaslUsername:   args.SaslUsername,
		SaslPassword:   args.SaslPassword,
		KafkaVersion:   pulumi.StringPtr(args.KafkaVersion),
	})
	if err != nil {
		return nil, fmt.Errorf("creating Kafka provider: %w", err)
	}

	outputs := &TopicsOutputs{
		Topics: make(map[string]*kafka.Topic, len(topics)),
	}

	for _, spec := range topics {
		topic, err := kafka.NewTopic(ctx, spec.Name, &kafka.TopicArgs{
			Name:              pulumi.String(spec.Name),
			Partitions:        pulumi.Int(spec.Partitions),
			ReplicationFactor: pulumi.Int(replicationFactor),
			Config: pulumi.StringMap{
				"retention.ms":        pulumi.String(strconv.FormatInt(spec.RetentionMs, 10)),
				"cleanup.policy":      pulumi.String(cleanupPolicy),
				"compression.type":    pulumi.String(compressionType),
				"segment.bytes":       pulumi.String(strconv.Itoa(spec.SegmentBytes)),
				"max.message.bytes":   pulumi.String(strconv.Itoa(spec.MaxMessageBytes)),
				"min.insync.replicas": pulumi.String(strconv.Itoa(minInsyncReplicas)),
			},
		}, pulumi.Provider(provider))
		if err != nil {
			return nil, fmt.Errorf("creating topic %q: %w", spec.Name, err)
		}

		outputs.Topics[spec.Name] = topic
	}

	return outputs, nil
}
