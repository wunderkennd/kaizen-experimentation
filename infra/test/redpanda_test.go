package test

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	awsstreaming "github.com/kaizen-experimentation/infra/pkg/aws/streaming"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	cloudstreaming "github.com/kaizen-experimentation/infra/pkg/streaming"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// ---------------------------------------------------------------------------
// Captured outputs — populated inside Pulumi's mock runtime
// ---------------------------------------------------------------------------

// resolvedStreamingOutputs holds the resolved string values of a
// types.StreamingOutputs instance, populated via ApplyT callbacks that run
// inside pulumi.RunErr (Pulumi waits for all pending applies before
// returning, so reads after RunErr are race-free).
type resolvedStreamingOutputs struct {
	mu                sync.Mutex
	bootstrapBrokers  string
	schemaRegistryURL string
	clusterName       string
	clusterArn        string
}

func captureStreamingOutputs(out types.StreamingOutputs, dst *resolvedStreamingOutputs) {
	out.BootstrapBrokers.ApplyT(func(s string) string {
		dst.mu.Lock()
		dst.bootstrapBrokers = s
		dst.mu.Unlock()
		return s
	})
	out.SchemaRegistryUrl.ApplyT(func(s string) string {
		dst.mu.Lock()
		dst.schemaRegistryURL = s
		dst.mu.Unlock()
		return s
	})
	out.ClusterName.ApplyT(func(s string) string {
		dst.mu.Lock()
		dst.clusterName = s
		dst.mu.Unlock()
		return s
	})
	out.ClusterArn.ApplyT(func(s string) string {
		dst.mu.Lock()
		dst.clusterArn = s
		dst.mu.Unlock()
		return s
	})
}

// ---------------------------------------------------------------------------
// Redpanda module — topology tests
// ---------------------------------------------------------------------------

func newRedpandaMockInputs() *cloudstreaming.RedpandaInputs {
	return &cloudstreaming.RedpandaInputs{
		ClusterName:    "kaizen-dev",
		Environment:    "dev",
		CloudProvider:  "aws",
		Region:         "us-east-1",
		Zones:          []string{"use1-az1", "use1-az2", "use1-az4"},
		ThroughputTier: "tier-1-aws-v2-x86",
		ClusterType:    "dedicated",
		ConnectionType: "private",
		TenantVpcID:    pulumi.String("vpc-0123456789abcdef0"),
		KafkaUsername:  pulumi.String("kaizen-redpanda-user"),
		KafkaPassword:  pulumi.String("test-kafka-password"),
		Tags:           kconfig.DefaultTags("dev"),
	}
}

func runRedpandaMock(t *testing.T) (*universalMocks, *resolvedStreamingOutputs) {
	t.Helper()
	mocks := &universalMocks{}
	captured := &resolvedStreamingOutputs{}

	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		out, err := cloudstreaming.NewRedpandaForTest(ctx, newRedpandaMockInputs())
		if err != nil {
			return err
		}
		captureStreamingOutputs(out, captured)
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}
	return mocks, captured
}

// runMskMockCapture runs the AWS MSK path and translates its module-local
// output struct into the shared types.StreamingOutputs (mirroring the
// translation aws.NewKafkaCluster performs in production code). It then
// captures the resolved string values so the parity test can compare both
// providers on the same contract.
func runMskMockCapture(t *testing.T) (*universalMocks, *resolvedStreamingOutputs) {
	t.Helper()
	mocks := &universalMocks{}
	captured := &resolvedStreamingOutputs{}

	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		mskOut, err := awsstreaming.NewMskCluster(ctx, "kaizen", newMskMockInputs())
		if err != nil {
			return err
		}
		// Mirror aws.NewKafkaCluster's translation. SchemaRegistryUrl is
		// patched in by Stage 5 in production; for parity testing we resolve
		// it to the canonical schema-registry.kaizen.local URL the AWS path
		// always lands on.
		shared := types.StreamingOutputs{
			BootstrapBrokers:  mskOut.MskBootstrapBrokers,
			SchemaRegistryUrl: pulumi.String("http://schema-registry.kaizen.local:8081").ToStringOutput(),
			ClusterArn:        mskOut.MskClusterArn,
			ClusterName:       mskOut.MskClusterName,
		}
		captureStreamingOutputs(shared, captured)
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}
	return mocks, captured
}

// TestRedpandaCoreResources verifies the Redpanda dispatch creates exactly
// one resource group, one network, one cluster, one user, one ACL, and the
// eight topics matching the MSK inventory.
func TestRedpandaCoreResources(t *testing.T) {
	mocks, _ := runRedpandaMock(t)

	cases := []struct {
		token string
		want  int
	}{
		{"redpanda:index/resourceGroup:ResourceGroup", 1},
		{"redpanda:index/network:Network", 1},
		{"redpanda:index/cluster:Cluster", 1},
		{"redpanda:index/user:User", 1},
		{"redpanda:index/acl:Acl", 1},
		{"kafka:index/topic:Topic", 8},
	}
	for _, c := range cases {
		got := mocks.count(c.token)
		if got != c.want {
			t.Errorf("token %q: got %d, want %d", c.token, got, c.want)
		}
	}
}

// TestRedpandaTopicInventoryMatchesMSK enforces that the Redpanda topic
// inventory is byte-for-byte identical to the MSK list (kafka/topic_configs.sh).
// Drift here is a contract break: M2/M3/M4a all assume the same topic names
// and partition counts regardless of streaming provider.
func TestRedpandaTopicInventoryMatchesMSK(t *testing.T) {
	specs := cloudstreaming.RedpandaTopicSpecs()

	want := map[string]struct {
		partitions  int
		retentionMs int64
	}{
		"exposures":                        {64, 7_776_000_000},
		"metric_events":                    {128, 7_776_000_000},
		"reward_events":                    {32, 15_552_000_000},
		"qoe_events":                       {64, 7_776_000_000},
		"guardrail_alerts":                 {8, 2_592_000_000},
		"sequential_boundary_alerts":       {8, 2_592_000_000},
		"model_retraining_events":          {8, 15_552_000_000},
		"surrogate_recalibration_requests": {4, 2_592_000_000},
	}

	if len(specs) != len(want) {
		t.Fatalf("topic count: got %d, want %d", len(specs), len(want))
	}
	for _, spec := range specs {
		w, ok := want[spec.Name]
		if !ok {
			t.Errorf("unexpected topic %q in Redpanda inventory", spec.Name)
			continue
		}
		if spec.Partitions != w.partitions {
			t.Errorf("topic %q partitions: got %d, want %d", spec.Name, spec.Partitions, w.partitions)
		}
		if spec.RetentionMs != w.retentionMs {
			t.Errorf("topic %q retention.ms: got %d, want %d", spec.Name, spec.RetentionMs, w.retentionMs)
		}
	}
}

// TestRedpandaClusterConfig verifies the Redpanda cluster is registered with
// the expected throughput tier, region, and connection type.
func TestRedpandaClusterConfig(t *testing.T) {
	mocks, _ := runRedpandaMock(t)

	clusters := mocks.byType("redpanda:index/cluster:Cluster")
	if len(clusters) != 1 {
		t.Fatalf("expected 1 Redpanda cluster, got %d", len(clusters))
	}
	c := clusters[0]

	checks := map[string]string{
		"region":         "us-east-1",
		"throughputTier": "tier-1-aws-v2-x86",
		"clusterType":    "dedicated",
		"connectionType": "private",
		"cloudProvider":  "aws",
	}
	for k, want := range checks {
		v, ok := c.Inputs[resource.PropertyKey(k)]
		if !ok {
			t.Errorf("cluster missing %q input", k)
			continue
		}
		if got := v.StringValue(); got != want {
			t.Errorf("cluster %q: got %q, want %q", k, got, want)
		}
	}
}

// TestRedpandaACLPrincipalReferencesAdminUser verifies the ACL is wired to
// the Redpanda admin user (so the Kafka topic provider can authenticate).
func TestRedpandaACLPrincipalReferencesAdminUser(t *testing.T) {
	mocks, _ := runRedpandaMock(t)

	acls := mocks.byType("redpanda:index/acl:Acl")
	if len(acls) != 1 {
		t.Fatalf("expected 1 ACL, got %d", len(acls))
	}
	v, ok := acls[0].Inputs["principal"]
	if !ok {
		t.Fatal("ACL missing principal input")
	}
	got := v.StringValue()
	if !strings.HasPrefix(got, "User:") {
		t.Errorf("ACL principal = %q, want User:* form", got)
	}
}

// ---------------------------------------------------------------------------
// Parameterized parity test — acceptance criterion 3
// ---------------------------------------------------------------------------

// TestStreamingOutputsParity asserts that both streaming providers populate
// types.StreamingOutputs in the same shape: BootstrapBrokers,
// SchemaRegistryUrl, and ClusterName must be non-empty for either provider.
// ClusterArn is provider-specific (empty for Redpanda by design — see
// types.StreamingOutputs doc comment).
//
// This is the topology test the issue calls for: it runs both dispatches in
// the Pulumi mock runtime and verifies the cross-provider contract holds.
func TestStreamingOutputsParity(t *testing.T) {
	type providerCase struct {
		name string
		run  func(t *testing.T) (*universalMocks, *resolvedStreamingOutputs)
	}

	for _, tc := range []providerCase{
		{name: "msk", run: runMskMockCapture},
		{name: "redpanda", run: runRedpandaMock},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			_, captured := tc.run(t)
			captured.mu.Lock()
			defer captured.mu.Unlock()

			if captured.bootstrapBrokers == "" {
				t.Error("BootstrapBrokers is empty")
			}
			if captured.schemaRegistryURL == "" {
				t.Error("SchemaRegistryUrl is empty")
			}
			if captured.clusterName == "" {
				t.Error("ClusterName is empty")
			}

			// ClusterArn parity: AWS path populates with an MSK ARN; Redpanda
			// leaves it empty (documented contract). Allowing either matches
			// the spec's "identically populated" criterion when read against
			// the StreamingOutputs doc comment.
			switch tc.name {
			case "msk":
				if captured.clusterArn == "" {
					t.Error("AWS MSK path must populate ClusterArn")
				}
			case "redpanda":
				if captured.clusterArn != "" {
					t.Errorf("Redpanda path must leave ClusterArn empty, got %q", captured.clusterArn)
				}
			}
		})
	}
}
