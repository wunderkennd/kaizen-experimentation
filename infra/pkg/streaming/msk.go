// Package streaming provides Pulumi components for the Kaizen platform's
// event streaming infrastructure: MSK Kafka cluster, topic provisioning,
// and Schema Registry (ECS service).
package streaming

import (
	"fmt"

	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/cloudwatch"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/kms"
	"github.com/pulumi/pulumi-aws/sdk/v6/go/aws/msk"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// MskInputs are the parameters required to create the MSK cluster.
// VPC-level resources (subnets, security groups) are passed in as inputs
// and wired by the caller in Sprint I.1.
type MskInputs struct {
	// SubnetIds for broker placement. Length must divide BrokerCount evenly.
	SubnetIds pulumi.StringArrayInput
	// SecurityGroupIds to attach to the MSK brokers.
	SecurityGroupIds pulumi.StringArrayInput
	// KafkaSecretArn is the Secrets Manager secret ARN containing SASL/SCRAM
	// credentials. Wired in Sprint I.1 to enable SCRAM authentication.
	KafkaSecretArn pulumi.StringInput
	// Config holds environment-specific sizing and monitoring settings.
	Config config.MskConfig
	// Tags applied to all resources created by this module.
	Tags pulumi.StringMapInput
}

// NewMskCluster provisions a KMS key, MSK configuration, CloudWatch log group,
// and MSK cluster. It returns the subset of StreamingOutputs that this module
// owns (MskClusterArn, MskBootstrapBrokers).
func NewMskCluster(ctx *pulumi.Context, name string, inputs *MskInputs, opts ...pulumi.ResourceOption) (*config.StreamingOutputs, error) {
	cfg := inputs.Config

	// --- KMS key for encryption at rest ---
	encryptionKey, err := kms.NewKey(ctx, fmt.Sprintf("%s-msk-key", name), &kms.KeyArgs{
		Description:          pulumi.Sprintf("MSK encryption key for %s", name),
		EnableKeyRotation:    pulumi.Bool(true),
		DeletionWindowInDays: pulumi.Int(7),
		Tags:                 inputs.Tags,
	}, opts...)
	if err != nil {
		return nil, fmt.Errorf("creating MSK KMS key: %w", err)
	}

	// --- MSK configuration ---
	serverProperties := "auto.create.topics.enable=false\n" +
		"default.replication.factor=3\n" +
		"min.insync.replicas=2\n" +
		"compression.type=lz4\n" +
		"log.retention.hours=168\n" +
		"log.segment.bytes=1073741824\n"

	mskConfig, err := msk.NewConfiguration(ctx, fmt.Sprintf("%s-msk-config", name), &msk.ConfigurationArgs{
		Name:             pulumi.Sprintf("kaizen-%s-msk-config", name),
		KafkaVersions:    pulumi.StringArray{pulumi.String(cfg.KafkaVersion)},
		ServerProperties: pulumi.String(serverProperties),
		Description:      pulumi.String("Kaizen experimentation platform MSK configuration"),
	}, opts...)
	if err != nil {
		return nil, fmt.Errorf("creating MSK configuration: %w", err)
	}

	// --- CloudWatch log group for broker logs ---
	logGroup, err := cloudwatch.NewLogGroup(ctx, fmt.Sprintf("%s-msk-logs", name), &cloudwatch.LogGroupArgs{
		Name:            pulumi.Sprintf("/aws/msk/%s", name),
		RetentionInDays: pulumi.Int(logRetentionDays(cfg.Environment)),
		Tags:            inputs.Tags,
	}, opts...)
	if err != nil {
		return nil, fmt.Errorf("creating MSK log group: %w", err)
	}

	// --- MSK cluster ---
	cluster, err := msk.NewCluster(ctx, fmt.Sprintf("%s-msk", name), &msk.ClusterArgs{
		ClusterName:         pulumi.String(name),
		KafkaVersion:        pulumi.String(cfg.KafkaVersion),
		NumberOfBrokerNodes: pulumi.Int(cfg.BrokerCount),

		BrokerNodeGroupInfo: &msk.ClusterBrokerNodeGroupInfoArgs{
			InstanceType:   pulumi.String(cfg.InstanceType),
			ClientSubnets:  inputs.SubnetIds,
			SecurityGroups: inputs.SecurityGroupIds,
			StorageInfo: &msk.ClusterBrokerNodeGroupInfoStorageInfoArgs{
				EbsStorageInfo: &msk.ClusterBrokerNodeGroupInfoStorageInfoEbsStorageInfoArgs{
					VolumeSize: pulumi.Int(cfg.EbsVolumeSize),
				},
			},
		},

		ConfigurationInfo: &msk.ClusterConfigurationInfoArgs{
			Arn:      mskConfig.Arn,
			Revision: mskConfig.LatestRevision,
		},

		EncryptionInfo: &msk.ClusterEncryptionInfoArgs{
			EncryptionAtRestKmsKeyArn: encryptionKey.Arn,
			EncryptionInTransit: &msk.ClusterEncryptionInfoEncryptionInTransitArgs{
				ClientBroker: pulumi.String("TLS"),
				InCluster:    pulumi.Bool(true),
			},
		},

		ClientAuthentication: &msk.ClusterClientAuthenticationArgs{
			Sasl: &msk.ClusterClientAuthenticationSaslArgs{
				Scram: pulumi.Bool(true),
			},
		},

		EnhancedMonitoring: pulumi.String(enhancedMonitoring(cfg)),

		LoggingInfo: &msk.ClusterLoggingInfoArgs{
			BrokerLogs: &msk.ClusterLoggingInfoBrokerLogsArgs{
				CloudwatchLogs: &msk.ClusterLoggingInfoBrokerLogsCloudwatchLogsArgs{
					Enabled:  pulumi.Bool(true),
					LogGroup: logGroup.Name,
				},
			},
		},

		Tags: inputs.Tags,
	}, opts...)
	if err != nil {
		return nil, fmt.Errorf("creating MSK cluster: %w", err)
	}

	// --- SCRAM secret association ---
	// Links the Secrets Manager SASL/SCRAM credential to the MSK cluster,
	// enabling clients to authenticate via SASL/SCRAM-SHA-512 on port 9096.
	if inputs.KafkaSecretArn != nil {
		_, err = msk.NewSingleScramSecretAssociation(ctx, fmt.Sprintf("%s-scram-assoc", name), &msk.SingleScramSecretAssociationArgs{
			ClusterArn: cluster.Arn,
			SecretArn:  inputs.KafkaSecretArn,
		}, opts...)
		if err != nil {
			return nil, fmt.Errorf("creating SCRAM secret association: %w", err)
		}
	}

	return &config.StreamingOutputs{
		MskClusterArn:       cluster.Arn,
		MskBootstrapBrokers: cluster.BootstrapBrokersSaslScram,
	}, nil
}

// enhancedMonitoring returns the monitoring level based on config.
// Prod gets PER_TOPIC_PER_BROKER; other environments use the configured
// default (typically PER_BROKER) to control CloudWatch costs.
func enhancedMonitoring(cfg config.MskConfig) string {
	if cfg.Environment == "prod" {
		return "PER_TOPIC_PER_BROKER"
	}
	if cfg.EnhancedMonitoring != "" {
		return cfg.EnhancedMonitoring
	}
	return "PER_BROKER"
}

// logRetentionDays returns CloudWatch log retention by environment,
// matching the IaC plan's environment-specific settings.
func logRetentionDays(env string) int {
	switch env {
	case "prod":
		return 30
	case "staging":
		return 14
	default:
		return 7
	}
}
