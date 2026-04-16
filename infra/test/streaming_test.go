// Package test contains Pulumi mock tests for the Kaizen streaming module
// (MSK Kafka cluster, KMS encryption, CloudWatch logging).
package test

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/streaming"
)

// newMskMockInputs returns a standard MskInputs suitable for mock testing.
func newMskMockInputs() *streaming.MskInputs {
	return &streaming.MskInputs{
		SubnetIds:        pulumi.StringArray{pulumi.String("subnet-a"), pulumi.String("subnet-b"), pulumi.String("subnet-c")},
		SecurityGroupIds: pulumi.StringArray{pulumi.String("sg-msk")},
		KafkaSecretArn:   nil,
		Config: kconfig.MskConfig{
			KafkaVersion:  "3.5.1",
			BrokerCount:   3,
			InstanceType:  "kafka.m5.large",
			EbsVolumeSize: 100,
			Environment:   "dev",
		},
		Tags: kconfig.DefaultTags("dev"),
	}
}

// runMskMock executes NewMskCluster inside Pulumi's mock runtime and returns
// the universalMocks instance populated with all created resources.
func runMskMock(t *testing.T) *universalMocks {
	t.Helper()
	mocks := &universalMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := streaming.NewMskCluster(ctx, "kaizen", newMskMockInputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}
	return mocks
}

// ---------------------------------------------------------------------------
// TestMskClusterCreated — verify 1 MSK cluster resource is created
// ---------------------------------------------------------------------------

func TestMskClusterCreated(t *testing.T) {
	mocks := runMskMock(t)

	clusters := mocks.byType("aws:msk/cluster:Cluster")
	if len(clusters) != 1 {
		t.Fatalf("expected 1 MSK cluster, got %d", len(clusters))
	}
}

// ---------------------------------------------------------------------------
// TestMskClusterKmsEncryption — verify KMS key with enableKeyRotation: true
// ---------------------------------------------------------------------------

func TestMskClusterKmsEncryption(t *testing.T) {
	mocks := runMskMock(t)

	keys := mocks.byType("aws:kms/key:Key")
	if len(keys) != 1 {
		t.Fatalf("expected 1 KMS key, got %d", len(keys))
	}

	key := keys[0]
	v, ok := key.Inputs["enableKeyRotation"]
	if !ok {
		t.Fatal("KMS key missing enableKeyRotation property")
	}
	if !v.BoolValue() {
		t.Error("KMS key enableKeyRotation = false, want true")
	}
}

// ---------------------------------------------------------------------------
// TestMskClusterBrokerCount — verify numberOfBrokerNodes matches config (3)
// ---------------------------------------------------------------------------

func TestMskClusterBrokerCount(t *testing.T) {
	mocks := runMskMock(t)

	clusters := mocks.byType("aws:msk/cluster:Cluster")
	if len(clusters) != 1 {
		t.Fatalf("expected 1 MSK cluster, got %d", len(clusters))
	}

	cluster := clusters[0]
	v, ok := cluster.Inputs["numberOfBrokerNodes"]
	if !ok {
		t.Fatal("MSK cluster missing numberOfBrokerNodes property")
	}
	if got := int(v.NumberValue()); got != 3 {
		t.Errorf("numberOfBrokerNodes = %d, want 3", got)
	}
}

// ---------------------------------------------------------------------------
// TestMskClusterKafkaVersion — verify kafka version is "3.5.1"
// ---------------------------------------------------------------------------

func TestMskClusterKafkaVersion(t *testing.T) {
	mocks := runMskMock(t)

	clusters := mocks.byType("aws:msk/cluster:Cluster")
	if len(clusters) != 1 {
		t.Fatalf("expected 1 MSK cluster, got %d", len(clusters))
	}

	cluster := clusters[0]
	v, ok := cluster.Inputs["kafkaVersion"]
	if !ok {
		t.Fatal("MSK cluster missing kafkaVersion property")
	}
	if got := v.StringValue(); got != "3.5.1" {
		t.Errorf("kafkaVersion = %q, want %q", got, "3.5.1")
	}
}

// ---------------------------------------------------------------------------
// TestMskCloudWatchLogging — verify CloudWatch log group for MSK broker logs
// ---------------------------------------------------------------------------

func TestMskCloudWatchLogging(t *testing.T) {
	mocks := runMskMock(t)

	logGroups := mocks.byType("aws:cloudwatch/logGroup:LogGroup")
	if len(logGroups) != 1 {
		t.Fatalf("expected 1 CloudWatch log group, got %d", len(logGroups))
	}

	lg := logGroups[0]

	// Verify the log group name contains the MSK identifier.
	if v, ok := lg.Inputs["name"]; !ok {
		t.Error("CloudWatch log group missing name property")
	} else if got := v.StringValue(); got != "/aws/msk/kaizen" {
		t.Errorf("log group name = %q, want %q", got, "/aws/msk/kaizen")
	}

	// Verify retention is set (dev = 7 days).
	if v, ok := lg.Inputs["retentionInDays"]; !ok {
		t.Error("CloudWatch log group missing retentionInDays property")
	} else if got := int(v.NumberValue()); got != 7 {
		t.Errorf("retentionInDays = %d, want 7", got)
	}
}
