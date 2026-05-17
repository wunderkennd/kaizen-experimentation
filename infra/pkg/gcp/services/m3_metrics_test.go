package services

import (
	"strings"
	"sync"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/common/resource"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m3StageOutputs returns the minimal StageOutputs M3 reads. No other service's
// repo keys, no other service's secrets — true test scoping.
func m3StageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"metrics": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/metrics").ToStringOutput(),
			},
		},
		DB: types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
		},
		Stream: types.StreamingOutputs{
			BootstrapBrokers: pulumi.String("seed-0.kaizen-dev.fmc.prd.cloud.redpanda.com:9092").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef:    pulumi.String("kaizen-dev-kafka").ToStringOutput(),
		},
		Storage: types.StorageOutputs{
			DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
			DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
		},
	}
}

// scopedMocks is a minimal pulumi.MockResourceMonitor that records every
// resource and enriches the type tokens M3 actually creates. Shared across
// per-service tests in this package.
type scopedMocks struct {
	mu        sync.Mutex
	resources []scopedResource
}

type scopedResource struct {
	TypeToken string
	Name      string
	Inputs    resource.PropertyMap
}

func (m *scopedMocks) NewResource(args pulumi.MockResourceArgs) (string, resource.PropertyMap, error) {
	m.mu.Lock()
	m.resources = append(m.resources, scopedResource{args.TypeToken, args.Name, args.Inputs})
	m.mu.Unlock()
	out := resource.PropertyMap{}
	for k, v := range args.Inputs {
		out[k] = v
	}
	switch args.TypeToken {
	case "gcp:serviceaccount/account:Account":
		acct, proj := "", ""
		if v, ok := args.Inputs["accountId"]; ok && v.HasValue() {
			acct = v.StringValue()
		}
		if v, ok := args.Inputs["project"]; ok && v.HasValue() {
			proj = v.StringValue()
		}
		out["email"] = resource.NewStringProperty(acct + "@" + proj + ".iam.gserviceaccount.com")
	case "gcp:cloudrunv2/service:Service":
		name := ""
		if v, ok := args.Inputs["name"]; ok && v.HasValue() {
			name = v.StringValue()
		}
		out["uri"] = resource.NewStringProperty("https://" + name + "-mock.a.run.app")
	}
	return args.Name + "_id", out, nil
}

func (m *scopedMocks) Call(_ pulumi.MockCallArgs) (resource.PropertyMap, error) {
	return resource.PropertyMap{}, nil
}

func (m *scopedMocks) byType(tok string) []scopedResource {
	m.mu.Lock()
	defer m.mu.Unlock()
	var out []scopedResource
	for _, r := range m.resources {
		if r.TypeToken == tok {
			out = append(out, r)
		}
	}
	return out
}

func scopedInputs() *compute.Inputs {
	return &compute.Inputs{
		Project:                     "kaizen-experimentation-dev",
		Region:                      "us-central1",
		VpcConnectorSelfLink:        pulumi.String("projects/test/locations/us-central1/connectors/kaizen-vpc").ToStringOutput(),
		ServiceDirectoryNamespaceID: pulumi.String("projects/test/locations/us-central1/namespaces/kaizen-local").ToStringOutput(),
	}
}

func scopedCfg() *kconfig.Config {
	return &kconfig.Config{
		Project: "kaizen", Environment: "dev", Env: kconfig.EnvDev,
		GCPProjectID: "kaizen-experimentation-dev", GCPRegion: "us-central1",
	}
}

// TestM3Metrics_Wiring asserts M3's Cloud Run shape: name, port, image.
func TestM3Metrics_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM3Metrics(ctx, scopedCfg(), scopedInputs(), m3StageOutputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM3Metrics failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m3-metrics" {
		t.Errorf("service name = %q, want kaizen-dev-m3-metrics", name)
	}

	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != 50056 {
		t.Errorf("containerPort = %v, want 50056", port)
	}
	if img := c["image"].StringValue(); !strings.Contains(img, "metrics") {
		t.Errorf("image = %q, want substring \"metrics\"", img)
	}
}

// TestM3Metrics_MissingMetricsRepoFails asserts the missing-repo error path.
func TestM3Metrics_MissingMetricsRepoFails(t *testing.T) {
	bad := m3StageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM3Metrics(ctx, scopedCfg(), scopedInputs(), bad)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "metrics") {
		t.Errorf("expected missing-metrics error, got %v", err)
	}
}
