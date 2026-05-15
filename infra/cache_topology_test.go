// Parameterized topology tests for the cache slice of Deploy(). Runs the
// cache stage with mocks for both cloudProvider: aws and cloudProvider: gcp
// and asserts the parity contract:
//
//   - both providers populate types.CacheOutputs.Endpoint;
//   - the AWS replication group enforces transit + at-rest encryption;
//   - the GCP Memorystore instance is reachable only via VPC connector
//     (PRIVATE_SERVICE_ACCESS connect mode, AUTH + transit encryption on,
//     no public reachability surface), and the endpoint is the spec form
//     "redis://host:port".
//
// Together these cover the issue #485 acceptance criteria for parity and
// private reachability.
package main

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	awsfacade "github.com/kaizen-experimentation/infra/pkg/aws"
	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	gcpfacade "github.com/kaizen-experimentation/infra/pkg/gcp"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// gcpCacheMocks captures the resources the GCP cache stage registers.
// Re-implemented (rather than reused) to keep the apples-to-apples slice
// scope; the broader gcpFullstackMocks knows about CICD types it shouldn't
// need to here.
type gcpCacheMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *gcpCacheMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, fsResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	id := args.Name + "_id"
	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}

	switch args.TypeToken {
	case "gcp:redis/instance:Instance":
		// Memorystore returns the assigned IP + port at apply-time. The
		// host is private (RFC1918) — the topology test below asserts on
		// the resource inputs (ConnectMode etc.), not the host string.
		outputs["host"] = resource.NewStringProperty("10.0.16.10")
		outputs["port"] = resource.NewNumberProperty(6379)
		outputs["authString"] = resource.NewStringProperty("mock-auth-token")
		outputs["name"] = resource.NewStringProperty(args.Name)
	}
	return id, outputs, nil
}

func (m *gcpCacheMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

// TestCacheTopologyParameterized runs the cache stage for both providers and
// asserts the shared CacheOutputs contract plus provider-specific topology
// invariants. The network stage is also exercised because cache depends on
// it (security groups on AWS, the VPC self-link on GCP).
func TestCacheTopologyParameterized(t *testing.T) {
	t.Run("aws", func(t *testing.T) {
		mocks := &fullstackMocks{}
		var cacheOut types.CacheOutputs
		err := pulumi.RunErr(func(ctx *pulumi.Context) error {
			cfg := kconfig.LoadConfig(ctx)
			netOut, err := awsfacade.NewNetwork(ctx, cfg)
			if err != nil {
				return err
			}
			out, err := awsfacade.NewCache(ctx, cfg, netOut)
			if err != nil {
				return err
			}
			cacheOut = out
			return nil
		},
			pulumi.WithMocks("kaizen", "dev", mocks),
			configWithProvider("aws"),
		)
		if err != nil {
			t.Fatalf("aws NewCache failed: %v", err)
		}

		// Shape contract — Endpoint must be populated.
		if cacheOut.Endpoint == (pulumi.StringOutput{}) {
			t.Error("aws: CacheOutputs.Endpoint is zero-valued")
		}

		// Topology — exactly one ElastiCache replication group registered.
		if got := mocks.count("aws:elasticache/replicationGroup:ReplicationGroup"); got != 1 {
			t.Errorf("aws: expected 1 ElastiCache ReplicationGroup, got %d", got)
		}

		// Reachability — encryption in transit + at rest are on, no inputs
		// indicate a public surface (ElastiCache has no PubliclyAccessible
		// flag; isolation is enforced by the redis security-group which is
		// asserted in the network topology test).
		rgs := mocks.byType("aws:elasticache/replicationGroup:ReplicationGroup")
		if len(rgs) != 1 {
			t.Fatalf("aws: ReplicationGroup count guard: got %d, want 1", len(rgs))
		}
		rg := rgs[0]
		if v, ok := rg.Inputs["transitEncryptionEnabled"]; !ok || !v.BoolValue() {
			t.Error("aws: ReplicationGroup must have transitEncryptionEnabled=true")
		}
		if v, ok := rg.Inputs["atRestEncryptionEnabled"]; !ok || !v.BoolValue() {
			t.Error("aws: ReplicationGroup must have atRestEncryptionEnabled=true")
		}
	})

	t.Run("gcp", func(t *testing.T) {
		mocks := &gcpCacheMocks{}
		var cacheOut types.CacheOutputs
		var endpoint string
		err := pulumi.RunErr(func(ctx *pulumi.Context) error {
			cfg := kconfig.LoadConfig(ctx)
			netOut, err := gcpfacade.NewNetwork(ctx, cfg)
			if err != nil {
				return err
			}
			out, err := gcpfacade.NewCache(ctx, cfg, netOut)
			if err != nil {
				return err
			}
			cacheOut = out
			// Capture the endpoint string for assertion below — ApplyT must
			// run inside the pulumi.RunErr context.
			out.Endpoint.ApplyT(func(s string) string {
				endpoint = s
				return s
			})
			return nil
		},
			pulumi.WithMocks("kaizen", "dev", mocks),
			configWithProvider("gcp"),
		)
		if err != nil {
			t.Fatalf("gcp NewCache failed: %v", err)
		}

		// Shape contract — Endpoint must be populated.
		if cacheOut.Endpoint == (pulumi.StringOutput{}) {
			t.Error("gcp: CacheOutputs.Endpoint is zero-valued")
		}

		// Topology — exactly one Memorystore instance + one VPC Connector
		// (the connector is the only ingress path; cache must be reachable
		// only through it).
		if got := countByType(mocks.resources, "gcp:redis/instance:Instance"); got != 1 {
			t.Errorf("gcp: expected 1 Memorystore Redis Instance, got %d", got)
		}
		if got := countByType(mocks.resources, "gcp:vpcaccess/connector:Connector"); got != 1 {
			t.Errorf("gcp: expected 1 Serverless VPC Access connector (private reachability), got %d", got)
		}

		// Reachability — PRIVATE_SERVICE_ACCESS connect mode + AuthEnabled
		// + transit encryption + authorizedNetwork wired to the VPC. These
		// jointly guarantee the instance has no public endpoint and is
		// reachable only through the VPC connector.
		insts := filterByType(mocks.resources, "gcp:redis/instance:Instance")
		if len(insts) != 1 {
			t.Fatalf("gcp: Instance count guard: got %d, want 1", len(insts))
		}
		inst := insts[0]
		if v, ok := inst.Inputs["connectMode"]; !ok || v.StringValue() != "PRIVATE_SERVICE_ACCESS" {
			t.Errorf("gcp: connectMode must be PRIVATE_SERVICE_ACCESS for private-IP-only reachability, got %v", v)
		}
		if v, ok := inst.Inputs["authEnabled"]; !ok || !v.BoolValue() {
			t.Error("gcp: authEnabled must be true (AUTH token gated)")
		}
		if v, ok := inst.Inputs["transitEncryptionMode"]; !ok || v.StringValue() != "SERVER_AUTHENTICATION" {
			t.Errorf("gcp: transitEncryptionMode must be SERVER_AUTHENTICATION, got %v", v)
		}
		if v, ok := inst.Inputs["authorizedNetwork"]; !ok || v.StringValue() == "" {
			t.Error("gcp: authorizedNetwork must reference the VPC self-link (private peering)")
		}
		if v, ok := inst.Inputs["tier"]; !ok || v.StringValue() != "STANDARD_HA" {
			t.Errorf("gcp: tier must be STANDARD_HA for parity with AWS Multi-AZ, got %v", v)
		}

		// Endpoint format — spec says "redis://host:port".
		if !strings.HasPrefix(endpoint, "redis://") {
			t.Errorf("gcp: endpoint must use scheme redis://, got %q", endpoint)
		}
		if !strings.Contains(endpoint, ":6379") {
			t.Errorf("gcp: endpoint must include the Memorystore port 6379, got %q", endpoint)
		}
	})
}

func filterByType(records []fsResource, typeToken string) []fsResource {
	var out []fsResource
	for _, r := range records {
		if r.TypeToken == typeToken {
			out = append(out, r)
		}
	}
	return out
}
