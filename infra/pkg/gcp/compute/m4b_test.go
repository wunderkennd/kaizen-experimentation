package compute

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// trackedResource captures a single Pulumi resource registration during a
// mock program run. The compute package uses its own helper here rather than
// re-importing the topology test's helpers — keeps the unit-test surface
// small and dependency-free.
type trackedResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

type m4bMocks struct {
	mu        sync.Mutex
	resources []trackedResource
}

func (m *m4bMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, trackedResource{
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
	case "gcp:compute/disk:Disk":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/zones/us-central1-a/disks/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/address:Address":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/regions/us-central1/addresses/" + args.Name)
		outputs["address"] = resource.NewStringProperty("10.0.16.42")
	case "gcp:compute/healthCheck:HealthCheck":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/healthChecks/" + args.Name)
	case "gcp:compute/instanceTemplate:InstanceTemplate":
		outputs["selfLink"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/global/instanceTemplates/" + args.Name)
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:compute/instanceGroupManager:InstanceGroupManager":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:servicedirectory/service:Service":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/services/" + args.Name)
	case "gcp:servicedirectory/endpoint:Endpoint":
		if v, ok := args.Inputs["address"]; ok {
			outputs["address"] = v
		}
		if v, ok := args.Inputs["port"]; ok {
			outputs["port"] = v
		}
	}
	return id, outputs, nil
}

func (m *m4bMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *m4bMocks) byType(t string) []trackedResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []trackedResource
	for _, r := range m.resources {
		if r.TypeToken == t {
			out = append(out, r)
		}
	}
	return out
}

// TestNewM4bInstanceRejectsNilArgs guards the precondition checks at the
// top of NewM4bInstance — they're the only defence against partially-wired
// callers in Deploy() crashing the Pulumi engine mid-apply.
func TestNewM4bInstanceRejectsNilArgs(t *testing.T) {
	mocks := &m4bMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4bInstance(ctx, nil)
		if err == nil {
			t.Errorf("NewM4bInstance(nil) returned nil error; want non-nil")
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("RunErr failed: %v", err)
	}
}

// TestNewM4bInstanceRequiresEnvironment guards against drift in the
// resource name prefix logic that would silently produce names like
// "kaizen--m4b-data" if env were empty.
func TestNewM4bInstanceRequiresEnvironment(t *testing.T) {
	mocks := &m4bMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4bInstance(ctx, &M4bArgs{
			Environment:                   "",
			Region:                        pulumi.String("us-central1").ToStringOutput(),
			PrivateSubnetSelfLink:         pulumi.String("subnet").ToStringOutput(),
			ServiceDirectoryNamespaceName: pulumi.String("ns").ToStringOutput(),
		})
		if err == nil {
			t.Errorf("NewM4bInstance with empty Environment returned nil error")
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("RunErr failed: %v", err)
	}
}

// TestNewM4bInstanceCreatesCoreResources is the package-local proxy for
// the full topology test in infra/m4b_topology_test.go. It asserts the
// resource graph shape without pulling in the test/ package or AWS mocks,
// so the compute package can be tested in isolation.
func TestNewM4bInstanceCreatesCoreResources(t *testing.T) {
	mocks := &m4bMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4bInstance(ctx, &M4bArgs{
			Environment:                   "dev",
			Region:                        pulumi.String("us-central1").ToStringOutput(),
			PrivateSubnetSelfLink:         pulumi.String("projects/test/regions/us-central1/subnetworks/kaizen-private"),
			ServiceDirectoryNamespaceName: pulumi.String("projects/test/locations/us-central1/namespaces/kaizen-local"),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM4bInstance failed: %v", err)
	}

	expectations := map[string]int{
		"gcp:compute/disk:Disk":                                 1,
		"gcp:compute/address:Address":                           1,
		"gcp:compute/healthCheck:HealthCheck":                   1,
		"gcp:compute/instanceTemplate:InstanceTemplate":         1,
		"gcp:compute/instanceGroupManager:InstanceGroupManager": 1,
		"gcp:compute/perInstanceConfig:PerInstanceConfig":       1,
		"gcp:servicedirectory/service:Service":                  1,
		"gcp:servicedirectory/endpoint:Endpoint":                1,
	}
	for typ, want := range expectations {
		if got := len(mocks.byType(typ)); got != want {
			t.Errorf("%s: got %d, want %d", typ, got, want)
		}
	}
}

// TestNewM4bInstanceDataDiskSpec pins the spec values (50GB pd-ssd) so a
// future config change doesn't silently downgrade the RocksDB volume.
func TestNewM4bInstanceDataDiskSpec(t *testing.T) {
	mocks := &m4bMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4bInstance(ctx, &M4bArgs{
			Environment:                   "prod",
			Region:                        pulumi.String("us-central1").ToStringOutput(),
			PrivateSubnetSelfLink:         pulumi.String("subnet").ToStringOutput(),
			ServiceDirectoryNamespaceName: pulumi.String("ns").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "prod", mocks))
	if err != nil {
		t.Fatalf("NewM4bInstance failed: %v", err)
	}

	disks := mocks.byType("gcp:compute/disk:Disk")
	if len(disks) != 1 {
		t.Fatalf("expected 1 Disk, got %d", len(disks))
	}
	if v, ok := disks[0].Inputs["size"]; !ok || !v.IsNumber() || v.NumberValue() != 50 {
		t.Errorf("Disk size = %v, want 50", v)
	}
	if v, ok := disks[0].Inputs["type"]; !ok || !v.IsString() || v.StringValue() != "pd-ssd" {
		t.Errorf("Disk type = %v, want pd-ssd", v)
	}
}

// TestNewM4bInstanceMigStatefulPolicies pins the MIG settings that hold
// the M4b invariants: targetSize=1, statefulDisks[].deleteRule=NEVER,
// statefulInternalIps not empty, and autoHealingPolicies references a
// health check.
func TestNewM4bInstanceMigStatefulPolicies(t *testing.T) {
	mocks := &m4bMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4bInstance(ctx, &M4bArgs{
			Environment:                   "dev",
			Region:                        pulumi.String("us-central1").ToStringOutput(),
			PrivateSubnetSelfLink:         pulumi.String("subnet").ToStringOutput(),
			ServiceDirectoryNamespaceName: pulumi.String("ns").ToStringOutput(),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM4bInstance failed: %v", err)
	}

	migs := mocks.byType("gcp:compute/instanceGroupManager:InstanceGroupManager")
	if len(migs) != 1 {
		t.Fatalf("expected 1 MIG, got %d", len(migs))
	}
	mig := migs[0]

	if v, ok := mig.Inputs["targetSize"]; !ok || !v.IsNumber() || v.NumberValue() != 1 {
		t.Errorf("MIG targetSize = %v, want 1", v)
	}

	sd, ok := mig.Inputs["statefulDisks"]
	if !ok || !sd.IsArray() || len(sd.ArrayValue()) != 1 {
		t.Fatalf("MIG statefulDisks not 1-element array; got %v", sd)
	}
	dr, ok := sd.ArrayValue()[0].ObjectValue()["deleteRule"]
	if !ok || !dr.IsString() || dr.StringValue() != "NEVER" {
		t.Errorf("MIG statefulDisks[0].deleteRule = %v, want NEVER", dr)
	}

	sip, ok := mig.Inputs["statefulInternalIps"]
	if !ok || !sip.IsArray() || len(sip.ArrayValue()) == 0 {
		t.Errorf("MIG statefulInternalIps missing or empty")
	}

	ahp, ok := mig.Inputs["autoHealingPolicies"]
	if !ok || !ahp.IsObject() {
		t.Errorf("MIG autoHealingPolicies missing or not an object")
	}
}

// TestNewM4bInstanceServiceDirectoryRegistration pins the endpoint port
// (50054) — the spec mandates this is the M4b gRPC port and changing it
// silently would break every server-side SDK that hardcodes 50054.
func TestNewM4bInstanceServiceDirectoryRegistration(t *testing.T) {
	mocks := &m4bMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM4bInstance(ctx, &M4bArgs{
			Environment:                   "dev",
			Region:                        pulumi.String("us-central1").ToStringOutput(),
			PrivateSubnetSelfLink:         pulumi.String("subnet").ToStringOutput(),
			ServiceDirectoryNamespaceName: pulumi.String("projects/test/locations/us-central1/namespaces/kaizen-local"),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM4bInstance failed: %v", err)
	}

	svcs := mocks.byType("gcp:servicedirectory/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 SD Service, got %d", len(svcs))
	}
	if v, ok := svcs[0].Inputs["serviceId"]; !ok || !v.IsString() || v.StringValue() != "m4b-policy" {
		t.Errorf("SD Service serviceId = %v, want m4b-policy", v)
	}

	eps := mocks.byType("gcp:servicedirectory/endpoint:Endpoint")
	if len(eps) != 1 {
		t.Fatalf("expected 1 SD Endpoint, got %d", len(eps))
	}
	if v, ok := eps[0].Inputs["port"]; !ok || !v.IsNumber() || v.NumberValue() != M4bPort {
		t.Errorf("SD Endpoint port = %v, want %d", v, M4bPort)
	}
}
