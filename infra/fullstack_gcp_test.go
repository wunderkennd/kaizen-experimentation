package main

import (
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"
)

// gcpFullstackMocks records all GCP resources Deploy() registers under
// cloudProvider=gcp. Phase 1 only covers Artifact Registry — once compute,
// network, etc. land, this file should grow type-token cases for them.
type gcpFullstackMocks struct {
	mu        sync.Mutex
	resources []fsResource
}

func (m *gcpFullstackMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
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
	case "gcp:artifactregistry/repository:Repository":
		// Echo the inputs the AR module reads back via apply().
		if v, ok := args.Inputs["repositoryId"]; ok {
			outputs["repositoryId"] = v
			outputs["name"] = v
		}
		if v, ok := args.Inputs["location"]; ok {
			outputs["location"] = v
		}
		if v, ok := args.Inputs["project"]; ok {
			outputs["project"] = v
		}
	}
	return id, outputs, nil
}

func (m *gcpFullstackMocks) Call(args pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *gcpFullstackMocks) byType(t string) []fsResource {
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

// gcpFullstackConfig sets the minimal stack config that Pulumi.gcp-dev.yaml
// declares plus a CI push principal so the IAM bindings get exercised.
func gcpFullstackConfig() pulumi.RunOption {
	return func(info *pulumi.RunInfo) {
		info.Config = map[string]string{
			"kaizen-experimentation:environment":          "dev",
			"kaizen-experimentation:cloudProvider":        "gcp",
			"kaizen-experimentation:gcpProjectId":         "kaizen-experimentation-dev",
			"kaizen-experimentation:gcpRegion":            "us-central1",
			"kaizen-experimentation:gcpArLocation":        "us",
			"kaizen-experimentation:gcpCiPushPrincipal":   "serviceAccount:ci@kaizen-experimentation-dev.iam.gserviceaccount.com",
			"kaizen-experimentation:gcpRunPullPrincipals": "serviceAccount:run@kaizen-experimentation-dev.iam.gserviceaccount.com",
		}
	}
}

// TestFullStackDeploy_GCP runs Deploy() with cloudProvider=gcp and verifies
// the CICD slice landed cleanly. This is the unit-level proxy for the
// `pulumi preview --stack gcp-dev` acceptance criterion in issue #482 — if
// this test fails, preview will fail.
func TestFullStackDeploy_GCP(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		gcpFullstackConfig(),
	)
	if err != nil {
		t.Fatalf("Deploy(gcp) failed: %v", err)
	}
}

// TestFullStackResourceCounts_GCP validates that the expected number of
// Artifact Registry repos and IAM members were registered. The expected
// counts are kept in lockstep with pkg/gcp/cicd.{ServiceNames,UtilityImageNames}.
func TestFullStackResourceCounts_GCP(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		gcpFullstackConfig(),
	)
	if err != nil {
		t.Fatalf("Deploy(gcp) failed: %v", err)
	}

	// 9 services + 1 utility = 10 repos; identical to the ECR count.
	gotRepos := len(mocks.byType("gcp:artifactregistry/repository:Repository"))
	if gotRepos != 10 {
		t.Errorf("Artifact Registry repositories: got %d, want 10", gotRepos)
	}

	// One writer (push) + one reader (pull) per repo → 20 IAM members.
	gotIam := len(mocks.byType("gcp:artifactregistry/repositoryIamMember:RepositoryIamMember"))
	if gotIam != 20 {
		t.Errorf("Artifact Registry IAM members: got %d, want 20", gotIam)
	}

	// Sanity: nothing AWS-specific should be registered when on GCP.
	for _, r := range mocks.resources {
		if len(r.TypeToken) > 4 && r.TypeToken[:4] == "aws:" {
			t.Errorf("unexpected AWS resource registered under cloudProvider=gcp: %s", r.TypeToken)
		}
	}
}

// TestFullStackDeploy_GCP_RejectsMissingProject ensures Deploy() fails fast
// when the GCP project is not configured. Without this guard, AR would 400
// at apply time and waste an apply cycle.
func TestFullStackDeploy_GCP_RejectsMissingProject(t *testing.T) {
	mocks := &gcpFullstackMocks{}
	err := pulumi.RunErr(Deploy,
		pulumi.WithMocks("kaizen", "dev", mocks),
		func(info *pulumi.RunInfo) {
			info.Config = map[string]string{
				"kaizen-experimentation:environment":   "dev",
				"kaizen-experimentation:cloudProvider": "gcp",
				// no gcpProjectId
			}
		},
	)
	if err == nil {
		t.Fatal("expected error for missing gcpProjectId, got nil")
	}
}
