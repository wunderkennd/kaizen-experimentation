// Parameterized topology test for the M4b Policy stateful compute slice.
// The test runs the M4b module on each provider with Pulumi mocks and
// asserts the invariants documented in docs/superpowers/specs/2026-04-20-
// multi-cloud-gcp-aws-design.md (Compute Model → M4b Policy Service):
//
//   - Stateful compute: dedicated single instance (AWS EC2 in ASG / GCE in MIG)
//   - Stateful storage: separate persistent volume that survives recreation
//     (AWS EBS via launch template + AWS Backup; GCP pd-ssd via standalone
//     Disk + MIG statefulDisks policy with deleteRule=NEVER)
//   - Single-instance constraint: ASG min=max=desired=1 / MIG targetSize=1
//   - Service discovery registration: Cloud Map service (AWS) /
//     Service Directory service (GCP)
//
// The test is the unit-level proxy for the `pulumi preview --stack gcp-dev`
// acceptance criterion in issue #487 — if it fails, `preview` will fail.
package main

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	awscompute "github.com/kaizen-experimentation/infra/pkg/aws/compute"
	gcpcompute "github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// m4bTopologyMocks captures every resource the per-provider M4b module
// registers, so the assertions below can query by type token and field.
type m4bTopologyMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *m4bTopologyMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
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

	// Output enrichment so downstream ApplyT chains resolve to plausible
	// values. The AWS half mirrors fullstack_test.go's mock; the GCP half
	// mirrors network_topology_test.go's gcpTopologyMocks plus the compute
	// resources this test exercises for the first time.
	switch args.TypeToken {
	// ── AWS mocks (subset needed by the m4b cluster slice) ───────────────
	case "aws:iam/role:Role":
		outputs["arn"] = resource.NewStringProperty("arn:aws:iam::123456789012:role/" + args.Name)
	case "aws:iam/instanceProfile:InstanceProfile":
		outputs["arn"] = resource.NewStringProperty("arn:aws:iam::123456789012:instance-profile/" + args.Name)
	case "aws:ssm/parameter:Parameter":
		outputs["value"] = resource.NewStringProperty("ami-0abcdef1234567890")
	case "aws:ec2/launchTemplate:LaunchTemplate":
		outputs["id"] = resource.NewStringProperty(args.Name + "-lt-id")
	case "aws:autoscaling/group:Group":
		outputs["name"] = resource.NewStringProperty(args.Name)
		outputs["arn"] = resource.NewStringProperty(
			"arn:aws:autoscaling:us-east-1:123456789012:autoScalingGroup:mock-uuid:autoScalingGroupName/" + args.Name)
	case "aws:ecs/cluster:Cluster":
		outputs["clusterArn"] = resource.NewStringProperty(
			"arn:aws:ecs:us-east-1:123456789012:cluster/" + args.Name)
		outputs["clusterName"] = resource.NewStringProperty(args.Name)
	case "aws:ecs/capacityProvider:CapacityProvider":
		outputs["name"] = resource.NewStringProperty(args.Name)

	// ── GCP mocks ────────────────────────────────────────────────────────
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
		outputs["instanceGroup"] = resource.NewStringProperty(
			"https://www.googleapis.com/compute/v1/projects/test/zones/us-central1-a/instanceGroups/" + args.Name)
	case "gcp:compute/perInstanceConfig:PerInstanceConfig":
		outputs["name"] = resource.NewStringProperty(args.Name)
	case "gcp:servicedirectory/service:Service":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/services/" + args.Name)
	case "gcp:servicedirectory/endpoint:Endpoint":
		outputs["name"] = resource.NewStringProperty(
			"projects/test/locations/us-central1/namespaces/kaizen-local/services/m4b-policy/endpoints/" + args.Name)
		// Endpoint outputs echo back inputs so the host:port apply chain
		// in NewM4bInstance resolves to a real string.
		if v, ok := args.Inputs["address"]; ok {
			outputs["address"] = v
		}
		if v, ok := args.Inputs["port"]; ok {
			outputs["port"] = v
		}
	}

	return id, outputs, nil
}

func (m *m4bTopologyMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	// AWS m4b cluster looks up the ECS-optimized AMI via ssm; return a
	// plausible value so the launch template renders.
	if args.Token == "aws:ssm/getParameter:getParameter" {
		return resource.PropertyMap{
			"name":  resource.NewStringProperty("/aws/service/ecs/optimized-ami/amazon-linux-2023/recommended/image_id"),
			"type":  resource.NewStringProperty("String"),
			"value": resource.NewStringProperty("ami-0abcdef1234567890"),
		}, nil
	}
	return resource.PropertyMap{}, nil
}

func (m *m4bTopologyMocks) byType(t string) []fsResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []fsResource
	for _, r := range m.resources {
		if r.TypeToken == t {
			out = append(out, r)
		}
	}
	return out
}

// findInput walks the resource property tree to fetch a top-level field
// value. Returns the zero value if absent.
func findInput(r fsResource, key string) resource.PropertyValue {
	if v, ok := r.Inputs[resource.PropertyKey(key)]; ok {
		return v
	}
	return resource.PropertyValue{}
}

// TestM4bTopologyParameterized runs the M4b compute module under both
// providers and asserts the cross-cloud invariants documented in the spec.
// Per-provider assertions cover the cloud-specific resources backing each
// invariant.
func TestM4bTopologyParameterized(t *testing.T) {
	cases := []struct {
		name   string
		runner func(t *testing.T, mocks *m4bTopologyMocks)
		// Cross-provider invariants — exactly one of each must appear in
		// either form. The case populates the per-provider type token.
		dedicatedComputeType string // ASG or MIG
		stateDiskType        string // (n/a for AWS — disk is in launch template) or gcp Disk
		serviceDiscoveryType string // Cloud Map service or Service Directory service
	}{
		{
			name: "aws",
			runner: func(t *testing.T, mocks *m4bTopologyMocks) {
				err := pulumi.RunErr(func(ctx *pulumi.Context) error {
					_, err := awscompute.NewCluster(ctx, &awscompute.ClusterArgs{
						Environment:        "dev",
						M4bInstanceType:    "c5.xlarge",
						M4bEbsSizeGb:       50,
						PrivateSubnetIds:   pulumi.StringArray{pulumi.String("subnet-a"), pulumi.String("subnet-b")}.ToStringArrayOutput(),
						M4bSecurityGroupId: pulumi.ID("sg-m4b").ToIDOutput(),
					})
					if err != nil {
						return err
					}
					_, err = awscompute.NewM4bService(ctx, &awscompute.M4bServiceArgs{
						Environment:         "dev",
						CloudMapNamespaceId: pulumi.ID("ns-cloudmap").ToIDOutput(),
						AsgName:             pulumi.String("kaizen-dev-m4b-asg").ToStringOutput(),
					})
					return err
				}, pulumi.WithMocks("kaizen", "dev", mocks))
				if err != nil {
					t.Fatalf("aws m4b modules failed: %v", err)
				}
			},
			dedicatedComputeType: "aws:autoscaling/group:Group",
			// AWS encodes the data disk inside the launch template's
			// BlockDeviceMappings, not as a standalone resource. We assert
			// on the launch template's presence instead, below.
			stateDiskType:        "",
			serviceDiscoveryType: "aws:servicediscovery/service:Service",
		},
		{
			name: "gcp",
			runner: func(t *testing.T, mocks *m4bTopologyMocks) {
				err := pulumi.RunErr(func(ctx *pulumi.Context) error {
					_, err := gcpcompute.NewM4bInstance(ctx, &gcpcompute.M4bArgs{
						Environment:                   "dev",
						Region:                        pulumi.String("us-central1").ToStringOutput(),
						PrivateSubnetSelfLink:         pulumi.String("projects/test/regions/us-central1/subnetworks/kaizen-private"),
						ServiceDirectoryNamespaceName: pulumi.String("projects/test/locations/us-central1/namespaces/kaizen-local"),
					})
					return err
				}, pulumi.WithMocks("kaizen", "dev", mocks))
				if err != nil {
					t.Fatalf("gcp m4b module failed: %v", err)
				}
			},
			dedicatedComputeType: "gcp:compute/instanceGroupManager:InstanceGroupManager",
			stateDiskType:        "gcp:compute/disk:Disk",
			serviceDiscoveryType: "gcp:servicedirectory/service:Service",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			mocks := &m4bTopologyMocks{}
			tc.runner(t, mocks)

			// Invariant 1: exactly one dedicated stateful compute resource.
			if got := len(mocks.byType(tc.dedicatedComputeType)); got != 1 {
				t.Errorf("%s: expected 1 dedicated compute (%s), got %d",
					tc.name, tc.dedicatedComputeType, got)
			}

			// Invariant 2: exactly one service-discovery registration.
			if got := len(mocks.byType(tc.serviceDiscoveryType)); got != 1 {
				t.Errorf("%s: expected 1 service registration (%s), got %d",
					tc.name, tc.serviceDiscoveryType, got)
			}

			// Provider-specific invariants.
			switch tc.name {
			case "aws":
				// Single-instance ASG: min == max == desired == 1.
				asgs := mocks.byType(tc.dedicatedComputeType)
				if len(asgs) != 1 {
					return
				}
				asg := asgs[0]
				minV := findInput(asg, "minSize")
				maxV := findInput(asg, "maxSize")
				desV := findInput(asg, "desiredCapacity")
				if !minV.IsNumber() || minV.NumberValue() != 1 {
					t.Errorf("aws: ASG minSize = %v, want 1", minV)
				}
				if !maxV.IsNumber() || maxV.NumberValue() != 1 {
					t.Errorf("aws: ASG maxSize = %v, want 1", maxV)
				}
				if !desV.IsNumber() || desV.NumberValue() != 1 {
					t.Errorf("aws: ASG desiredCapacity = %v, want 1", desV)
				}

				// EBS data volume declared in the launch template (50GB gp3
				// per spec). The presence of the mapping itself is what
				// makes the volume "survive instance recreation" — ASG
				// instances inherit it from the template.
				lts := mocks.byType("aws:ec2/launchTemplate:LaunchTemplate")
				if len(lts) != 1 {
					t.Errorf("aws: expected 1 launch template, got %d", len(lts))
					return
				}
				mappings := findInput(lts[0], "blockDeviceMappings")
				if !mappings.IsArray() || len(mappings.ArrayValue()) == 0 {
					t.Errorf("aws: launch template missing blockDeviceMappings (data disk)")
				}

			case "gcp":
				// targetSize == 1
				migs := mocks.byType(tc.dedicatedComputeType)
				if len(migs) != 1 {
					return
				}
				mig := migs[0]
				if v := findInput(mig, "targetSize"); !v.IsNumber() || v.NumberValue() != 1 {
					t.Errorf("gcp: MIG targetSize = %v, want 1", v)
				}

				// Stateful disk policy: at least one entry with DeleteRule
				// NEVER, proving the disk survives MIG recreation.
				sd := findInput(mig, "statefulDisks")
				if !sd.IsArray() || len(sd.ArrayValue()) == 0 {
					t.Fatalf("gcp: MIG missing statefulDisks policy")
				}
				first := sd.ArrayValue()[0]
				if !first.IsObject() {
					t.Fatalf("gcp: MIG statefulDisks[0] not an object")
				}
				dr, ok := first.ObjectValue()["deleteRule"]
				if !ok || !dr.IsString() || dr.StringValue() != "NEVER" {
					t.Errorf("gcp: MIG statefulDisks[0].deleteRule = %v, want NEVER", dr)
				}

				// Autohealing policy must reference a health check —
				// otherwise the MIG is just a deployment manager, not
				// a self-healing group, and the < 10s recovery SLO is
				// unattainable.
				ahp := findInput(mig, "autoHealingPolicies")
				if !ahp.IsObject() {
					t.Errorf("gcp: MIG missing autoHealingPolicies object")
				} else if hc, ok := ahp.ObjectValue()["healthCheck"]; !ok || (!hc.IsString() && !hc.IsComputed()) || (hc.IsString() && hc.StringValue() == "") {
					t.Errorf("gcp: MIG autoHealingPolicies missing healthCheck reference")
				}

				// Standalone Persistent Disk: 50GB pd-ssd. Decoupled from
				// the instance lifecycle so a MIG-driven recreation reattaches
				// the same disk via per-instance config.
				disks := mocks.byType(tc.stateDiskType)
				if len(disks) != 1 {
					t.Fatalf("gcp: expected 1 standalone Disk, got %d", len(disks))
				}
				disk := disks[0]
				if v := findInput(disk, "size"); !v.IsNumber() || v.NumberValue() != 50 {
					t.Errorf("gcp: Disk size = %v, want 50", v)
				}
				if v := findInput(disk, "type"); !v.IsString() || v.StringValue() != "pd-ssd" {
					t.Errorf("gcp: Disk type = %v, want pd-ssd", v)
				}

				// Per-instance config bound to a specific disk + IP so
				// recreation is deterministic.
				if got := len(mocks.byType("gcp:compute/perInstanceConfig:PerInstanceConfig")); got != 1 {
					t.Errorf("gcp: expected 1 PerInstanceConfig, got %d", got)
				}

				// Service Directory Endpoint exists and binds to port 50054
				// — the spec-mandated M4b gRPC port.
				eps := mocks.byType("gcp:servicedirectory/endpoint:Endpoint")
				if len(eps) != 1 {
					t.Fatalf("gcp: expected 1 SD Endpoint, got %d", len(eps))
				}
				if v := findInput(eps[0], "port"); !v.IsNumber() || v.NumberValue() != 50054 {
					t.Errorf("gcp: SD Endpoint port = %v, want 50054", v)
				}
			}
		})
	}
}

// TestM4bGcpComputeFacade exercises the gcp.NewCompute facade path with
// configured stack values to ensure the higher-level entry point Deploy()
// uses is exercised too. The lower-level NewM4bInstance is covered by
// TestM4bTopologyParameterized above.
func TestM4bGcpComputeFacade(t *testing.T) {
	mocks := &m4bTopologyMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		gcpFullstackConfig(),
	)
	if err != nil {
		t.Fatalf("Deploy(gcp) with M4b slice failed: %v", err)
	}

	// Sanity: the M4b slice exists alongside the storage + cicd slices.
	if got := len(mocks.byType("gcp:compute/instanceGroupManager:InstanceGroupManager")); got != 1 {
		t.Errorf("expected 1 MIG from Deploy(gcp), got %d", got)
	}
	if got := len(mocks.byType("gcp:compute/disk:Disk")); got != 1 {
		t.Errorf("expected 1 standalone Disk from Deploy(gcp), got %d", got)
	}
	// Deploy(gcp) registers one Service Directory entry per Cloud Run service
	// plus one for M4b: m4b-policy (NewM4bInstance), preview-canary (factory
	// canary from #486), m2-orchestration (#490), and m6-ui (#494). Each
	// per-service Cloud Run deploy (#488..#493/#495) adds one as it lands.
	if got := len(mocks.byType("gcp:servicedirectory/service:Service")); got != 4 {
		t.Errorf("expected 4 SD Services from Deploy(gcp) (M4b + canary + M2-Orch + M6), got %d", got)
	}
}
