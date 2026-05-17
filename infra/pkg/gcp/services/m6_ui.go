package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM6UI wires M6 UI (Next.js 14 SSR) onto Cloud Run. Image comes from the
// "ui" Artifact Registry repo (#482 created the registry; the CI image
// pipeline pushes :latest). Default min-instances (0): M6 is request-driven
// UI traffic, not a p99-SLA gRPC path like M1/M7, so cold starts are
// acceptable and the scale-to-zero cost saving applies. The factory
// auto-mints the roles/secretmanager.secretAccessor binding for the auth
// secret and registers m6-ui in Service Directory so peers can resolve it.
//
// Takes m4bEndpoint as an extra argument because M4b is not a Cloud Run
// service and therefore not part of StageOutputs (it's the stateful GCE/MIG
// slice constructed in NewCompute's preamble). M6 resolves the rest of the
// MX_*_ENDPOINT vars via Service Directory as services land in
// #488..#493/#495.
func NewM6UI(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
	m4bEndpoint pulumi.StringInput,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["ui"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM6UI: cicdOut.RepositoryURLs missing the \"ui\" repo required to deploy M6 (#494)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m6-ui",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: 3000, // Next.js SSR port — parity with the AWS M6 Fargate task
			MinInstances:  0,    // default per #494; UI traffic is request-driven
			EnvVars: []compute.EnvVar{
				{Name: "NODE_ENV", Value: pulumi.String("production")},
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				// The one backend that exists at this stage. M4b registered
				// itself in Service Directory above; M6 reaches it via this
				// resolvable endpoint. The remaining MX_*_ENDPOINT vars are
				// added by #488..#493/#495 as those services land.
				{Name: "M4B_POLICY_ENDPOINT", Value: m4bEndpoint},
			},
			Secrets: []compute.SecretEnv{
				// SSR session layer. SecretID is the bare projects/<P>/secrets/<S>
				// path so Cloud Run's secretKeyRef.Secret and the auto-created
				// SecretIamMember both resolve; "latest" tracks rotation.
				{EnvName: "AUTH_SECRET", SecretID: stages.Secrets.AuthSecretRef, Version: "latest"},
			},
		})
}
