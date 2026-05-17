package services

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewCanary is the preview-canary Cloud Run service. Image is Google's public
// hello-world container — no Artifact Registry dep, no DB/cache/secrets.
// Exists so the platform's per-service Cloud Run wiring (factory + SD + WI)
// is exercised end-to-end against `pulumi preview` without needing a real
// service image to be published first. Replace per-service in issues
// #488..#495 with the matching Artifact Registry URL from
// CICDOutputs.RepositoryURLs.
func NewCanary(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
) (*compute.CloudRunService, error) {
	return compute.NewCloudRunService(ctx, cfg, inputs, "preview-canary",
		&compute.Options{
			// Google's public hello-world image — exercises the helper
			// against `pulumi preview` without depending on a real
			// build/push of a Kaizen image. Replace per-service in
			// issues #488..#495 with the matching Artifact Registry URL
			// from CICDOutputs.RepositoryURLs.
			Image:         pulumi.String("us-docker.pkg.dev/cloudrun/container/hello"),
			ContainerPort: 8080,
			MinInstances:  0,
		})
}
