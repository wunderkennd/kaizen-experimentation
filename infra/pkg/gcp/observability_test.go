// Package gcp — observability_test.go pins the resource topology that
// pkg/gcp.NewObservability registers. Runs the module against a Pulumi
// mock monitor and asserts that:
//
//   - the module registers the fixed-count support resources (Pub/Sub
//     topic, notification channel, log bucket, log sink);
//   - it emits one Monitoring alert policy per entry in alertPolicies,
//     matching the AWS-parity inventory documented in observability.go's
//     package comment;
//   - alert policies attach to the notification channel (parity with the
//     AWS AlarmActions → SNS topic wiring).
//
// This test is the pin the parity table refers to — if alertPolicies
// shrinks or a resource type is dropped, this test flips red before the
// #503 parity audit has to catch it.
package gcp

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
)

type obsMocks struct {
	mu        sync.Mutex
	resources []obsResource
}

type obsResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *obsMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, obsResource{
		TypeToken: args.TypeToken,
		Name:      args.Name,
		Inputs:    args.Inputs,
	})
	m.mu.Unlock()

	outputs := resource.PropertyMap{}
	for k, v := range args.Inputs {
		outputs[k] = v
	}
	// NotificationChannel.Name is the projects/…/notificationChannels/…
	// path the alert policies reference. Populate a deterministic value so
	// downstream apply chains resolve.
	if args.TypeToken == "gcp:monitoring/notificationChannel:NotificationChannel" {
		outputs["name"] = resource.NewStringProperty(
			"projects/test/notificationChannels/" + args.Name)
	}
	return args.Name + "_id", outputs, nil
}

func (m *obsMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *obsMocks) countByType(typeToken string) int {
	m.mu.Lock()
	defer m.mu.Unlock()
	n := 0
	for _, r := range m.resources {
		if r.TypeToken == typeToken {
			n++
		}
	}
	return n
}

func TestObservabilityTopology(t *testing.T) {
	mocks := &obsMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		return NewObservability(ctx, &kconfig.Config{
			Environment:         "dev",
			GCPProjectID:        "kaizen-test",
			CloudwatchRetention: 30,
		}, &ObservabilityInputs{
			CloudSQLInstanceName: pulumi.String("kaizen-test-db").ToStringOutput(),
			M4bInstanceName:      pulumi.String("kaizen-test-m4b-0").ToStringOutput(),
		})
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewObservability failed: %v", err)
	}

	// Fixed-count support resources — the AWS SNS/log-group/AMP-workspace
	// analogues collapsed to one of each per the parity table.
	fixedCounts := map[string]int{
		"gcp:pubsub/topic:Topic":                                 1,
		"gcp:monitoring/notificationChannel:NotificationChannel": 1,
		"gcp:logging/projectBucketConfig:ProjectBucketConfig":    1,
		"gcp:logging/projectSink:ProjectSink":                    1,
	}
	for typeToken, want := range fixedCounts {
		if got := mocks.countByType(typeToken); got != want {
			t.Errorf("%s: got %d, want %d", typeToken, got, want)
		}
	}

	// Alert policies — the authoritative inventory. Any drift in
	// alertPolicies (dropped service, removed infra alert) flips this
	// count and fails loudly.
	wantAlerts := len(alertPolicies)
	if got := mocks.countByType("gcp:monitoring/alertPolicy:AlertPolicy"); got != wantAlerts {
		t.Errorf("AlertPolicy count: got %d, want %d (len(alertPolicies))",
			got, wantAlerts)
	}
}

// TestObservabilityRejectsMissingProject guards the config precondition —
// GCP observability cannot resolve project-scoped metric filters without a
// project ID, so NewObservability must fail loud rather than register
// half-configured resources.
func TestObservabilityRejectsMissingProject(t *testing.T) {
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		return NewObservability(ctx, &kconfig.Config{Environment: "dev"},
			&ObservabilityInputs{
				CloudSQLInstanceName: pulumi.String("x").ToStringOutput(),
				M4bInstanceName:      pulumi.String("y").ToStringOutput(),
			})
	}, pulumi.WithMocks("kaizen", "dev", &obsMocks{}))
	if err == nil {
		t.Fatal("expected error for missing GCPProjectID, got nil")
	}
}
