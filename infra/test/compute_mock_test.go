// Package test contains Pulumi mock-based infrastructure tests for the ECS
// compute module: cluster creation, capacity providers, M4b launch template,
// and M4b Auto Scaling Group.
package test

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/compute"
)

// ---------------------------------------------------------------------------
// Helper: run NewCluster with universalMocks and return the mocks for queries.
// ---------------------------------------------------------------------------

func runClusterMock(t *testing.T) *universalMocks {
	t.Helper()
	mocks := &universalMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := compute.NewCluster(ctx, &compute.ClusterArgs{
			Environment:        "dev",
			M4bInstanceType:    "c5.xlarge",
			PrivateSubnetIds:   pulumi.ToStringArray([]string{"subnet-a", "subnet-b"}).ToStringArrayOutput(),
			M4bSecurityGroupId: pulumi.String("sg-m4b").ToStringOutput().ApplyT(func(s string) pulumi.ID { return pulumi.ID(s) }).(pulumi.IDOutput),
		})
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("Pulumi program failed: %v", err)
	}
	return mocks
}

// ---------------------------------------------------------------------------
// Test: ECS cluster created with Container Insights enabled
// ---------------------------------------------------------------------------

func TestEcsClusterCreated(t *testing.T) {
	mocks := runClusterMock(t)

	clusters := mocks.byType("aws:ecs/cluster:Cluster")
	if len(clusters) != 1 {
		t.Fatalf("expected 1 ECS cluster, got %d", len(clusters))
	}

	cluster := clusters[0]

	// Verify Container Insights is enabled via the settings array.
	settingsVal, ok := cluster.Inputs["settings"]
	if !ok || !settingsVal.IsArray() {
		t.Fatal("ECS cluster missing 'settings' array")
	}

	settings := settingsVal.ArrayValue()
	foundInsights := false
	for _, s := range settings {
		obj := s.ObjectValue()
		name, hasName := obj["name"]
		value, hasValue := obj["value"]
		if hasName && hasValue &&
			name.StringValue() == "containerInsights" &&
			value.StringValue() == "enabled" {
			foundInsights = true
			break
		}
	}
	if !foundInsights {
		t.Error("ECS cluster does not have containerInsights: \"enabled\" in settings")
	}
}

// ---------------------------------------------------------------------------
// Test: FARGATE and FARGATE_SPOT capacity providers registered
// ---------------------------------------------------------------------------

func TestFargateCapacityProviders(t *testing.T) {
	mocks := runClusterMock(t)

	// The ClusterCapacityProviders resource associates providers with the cluster.
	cpAssocs := mocks.byType("aws:ecs/clusterCapacityProviders:ClusterCapacityProviders")
	if len(cpAssocs) != 1 {
		t.Fatalf("expected 1 ClusterCapacityProviders resource, got %d", len(cpAssocs))
	}

	providersVal, ok := cpAssocs[0].Inputs["capacityProviders"]
	if !ok || !providersVal.IsArray() {
		t.Fatal("ClusterCapacityProviders missing 'capacityProviders' array")
	}

	providers := providersVal.ArrayValue()
	found := map[string]bool{"FARGATE": false, "FARGATE_SPOT": false}
	for _, p := range providers {
		name := p.StringValue()
		if _, want := found[name]; want {
			found[name] = true
		}
	}

	for name, present := range found {
		if !present {
			t.Errorf("capacity provider %q not found in ClusterCapacityProviders", name)
		}
	}
}

// ---------------------------------------------------------------------------
// Test: M4b EC2 launch template is created
// ---------------------------------------------------------------------------

func TestM4bLaunchTemplate(t *testing.T) {
	mocks := runClusterMock(t)

	lts := mocks.byType("aws:ec2/launchTemplate:LaunchTemplate")
	if len(lts) != 1 {
		t.Fatalf("expected 1 EC2 launch template, got %d", len(lts))
	}

	lt := lts[0]

	// Verify instance type matches the arg we passed.
	if v, ok := lt.Inputs["instanceType"]; !ok || v.StringValue() != "c5.xlarge" {
		t.Errorf("launch template instanceType = %v, want c5.xlarge", lt.Inputs["instanceType"])
	}

	// Verify it has the M4b naming convention.
	if !contains(lt.Name, "m4b") {
		t.Errorf("launch template name %q does not contain 'm4b'", lt.Name)
	}
}

// ---------------------------------------------------------------------------
// Test: M4b Auto Scaling Group is created
// ---------------------------------------------------------------------------

func TestM4bAsgInstanceType(t *testing.T) {
	mocks := runClusterMock(t)

	asgs := mocks.byType("aws:autoscaling/group:Group")
	if len(asgs) != 1 {
		t.Fatalf("expected 1 Auto Scaling Group, got %d", len(asgs))
	}

	asg := asgs[0]

	// Verify ASG naming contains m4b.
	if !contains(asg.Name, "m4b") {
		t.Errorf("ASG name %q does not contain 'm4b'", asg.Name)
	}

	// Verify min/max/desired = 1 (single-instance LMAX core).
	if v, ok := asg.Inputs["minSize"]; !ok || v.NumberValue() != 1 {
		t.Errorf("ASG minSize = %v, want 1", asg.Inputs["minSize"])
	}
	if v, ok := asg.Inputs["maxSize"]; !ok || v.NumberValue() != 1 {
		t.Errorf("ASG maxSize = %v, want 1", asg.Inputs["maxSize"])
	}
	if v, ok := asg.Inputs["desiredCapacity"]; !ok || v.NumberValue() != 1 {
		t.Errorf("ASG desiredCapacity = %v, want 1", asg.Inputs["desiredCapacity"])
	}
}
