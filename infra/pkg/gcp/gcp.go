// Package gcp is the GCP-side facade for Deploy(). It mirrors pkg/aws (one
// stage-aggregating function per Deploy() switch arm) and is intentionally
// thin — actual resource creation happens in pkg/gcp/<module>/ sub-packages.
//
// Phase 1 ships network (#519), cicd (Artifact Registry, #516), and storage
// (Cloud Storage, #480). Subsequent phases will fill in database, cache,
// secrets, compute, and edge.
package gcp

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/cache"
	"github.com/kaizen-experimentation/infra/pkg/gcp/cicd"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
	"github.com/kaizen-experimentation/infra/pkg/gcp/database"
	"github.com/kaizen-experimentation/infra/pkg/gcp/network"
	"github.com/kaizen-experimentation/infra/pkg/gcp/storage"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// ─── Stage 1: Network ───────────────────────────────────────────────────────

// NewNetwork creates the GCP networking foundation: a custom VPC with public
// and private regional subnets, Cloud Router + Cloud NAT for egress, six
// firewall rules whose target tags match the AWS security-group keys, a
// Service Directory namespace for service discovery, and a Serverless VPC
// Access connector so Cloud Run services can reach private resources.
//
// Returns types.NetworkOutputs with provider-specific zero values for the
// AWS-only fields PrivateRouteTableIds and S3VpcEndpointId — GCP networks
// route implicitly and have no S3 gateway endpoint analogue. Documented in
// pkg/types/outputs.go.
func NewNetwork(ctx *pulumi.Context, _ *kconfig.Config) (types.NetworkOutputs, error) {
	vpcOut, err := network.NewVpc(ctx)
	if err != nil {
		return types.NetworkOutputs{}, err
	}

	fwRes, err := network.NewFirewallRules(ctx, &network.FirewallArgs{
		NetworkId: vpcOut.NetworkId,
	})
	if err != nil {
		return types.NetworkOutputs{}, err
	}

	sdOut, err := network.NewServiceDirectory(ctx, &network.ServiceDirectoryArgs{
		Region: vpcOut.Region,
	})
	if err != nil {
		return types.NetworkOutputs{}, err
	}
	ctx.Export("serviceDirectoryNamespaceId", sdOut.NamespaceId)
	ctx.Export("serviceDirectoryNamespaceName", sdOut.NamespaceName)

	connOut, err := network.NewVpcConnector(ctx, &network.VpcConnectorArgs{
		NetworkName: vpcOut.NetworkName,
		Region:      vpcOut.Region,
	})
	if err != nil {
		return types.NetworkOutputs{}, err
	}
	ctx.Export("vpcConnectorId", connOut.ConnectorId)
	ctx.Export("vpcConnectorSelfLink", connOut.ConnectorSelfLink)

	// Private Service Access — VPC peering with Google's managed services
	// tenant projects so Cloud SQL (#484) and Memorystore (#485) can be
	// reached via private IPs. Provisioned here because PSA is VPC-scoped:
	// one peering serves every Google-managed data store on the VPC.
	psaOut, err := network.NewPrivateServiceAccess(ctx, &network.PrivateServiceAccessArgs{
		NetworkId: vpcOut.NetworkId,
	})
	if err != nil {
		return types.NetworkOutputs{}, err
	}
	ctx.Export("psaReservedRangeName", psaOut.ReservedRangeName)

	return types.NetworkOutputs{
		VpcId:                    vpcOut.NetworkId,
		PublicSubnetIds:          vpcOut.PublicSubnetIds,
		PrivateSubnetIds:         vpcOut.PrivateSubnetIds,
		SecurityGroupIds:         fwRes.Rules,
		ServiceDiscoveryId:       sdOut.NamespaceId,
		ReservedPeeringRangeName: psaOut.ReservedRangeName,
		// Zero-valued on GCP per types.NetworkOutputs documentation:
		// PrivateRouteTableIds — GCP routes implicitly via subnet definitions.
		// S3VpcEndpointId — no S3 gateway endpoint analogue on GCP.
	}, nil
}

// ─── Stage 2: Storage + IAM ─────────────────────────────────────────────────

// NewStorage creates the Cloud Storage buckets (data, mlflow, logs) and
// returns them via the cross-provider types.StorageOutputs contract.
//
// Unlike AWS, GCS buckets do not consume any input from the network stage:
// VPC-private access on GCP is enforced via VPC Service Controls (an
// org-level resource, not bucket IAM) and is intentionally deferred to a
// follow-up PR. The `_ types.NetworkOutputs` parameter is kept to maintain
// signature parity with `aws.NewStorage` so Deploy() can call either
// without per-provider wiring differences.
func NewStorage(ctx *pulumi.Context, cfg *kconfig.Config, _ types.NetworkOutputs) (types.StorageOutputs, error) {
	out, err := storage.NewStorage(ctx, cfg.Environment, &storage.StorageInputs{})
	if err != nil {
		return types.StorageOutputs{}, err
	}
	ctx.Export("dataBucketName", out.DataBucketName)
	ctx.Export("mlflowBucketName", out.MlflowBucketName)
	ctx.Export("logsBucketName", out.LogsBucketName)
	return types.StorageOutputs{
		DataBucketName:   out.DataBucketName,
		DataBucketRef:    out.DataBucketURI,
		MlflowBucketName: out.MlflowBucketName,
		MlflowBucketRef:  out.MlflowBucketURI,
		LogsBucketName:   out.LogsBucketName,
		LogsBucketRef:    out.LogsBucketURI,
	}, nil
}

// ─── Stage 3: Data Stores ───────────────────────────────────────────────────

// NewCache provisions the Memorystore Redis instance and narrows the
// pkg/gcp/cache outputs to the cross-provider types.CacheOutputs shape.
//
// Parity with pkg/aws.NewCache:
//   - HA (1 primary + 1 replica via Tier=STANDARD_HA + READ_REPLICAS_ENABLED)
//   - AUTH + transit encryption enabled
//   - Private-IP-only reachability via PRIVATE_SERVICE_ACCESS connect mode,
//     so Cloud Run reaches the instance through the Serverless VPC Access
//     connector wired in NewNetwork. There is no public endpoint.
//
// The AUTH password is generated by Memorystore and surfaced on the module's
// internal RedisOutputs.AuthString. The Phase 1 secrets wiring PR consumes
// that and writes it to Secret Manager so downstream services read it via
// SecretsOutputs.RedisSecretRef. The cache module does not depend on the
// secrets module — wiring is one-directional cache → secrets.
//
// netOut.VpcId is the VPC self-link (GCP returns the self-link from .ID()
// on compute.Network). Memorystore peers with this network for private IP
// allocation.
func NewCache(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.CacheOutputs, error) {
	region := cfg.GCPRegion
	if region == "" {
		region = "us-central1"
	}

	out, err := cache.NewRedis(ctx, &cache.RedisConfig{
		Name:              "kaizen-redis",
		Region:            pulumi.String(region),
		AuthorizedNetwork: netOut.VpcId.ToStringOutput(),
		Labels:            gcpLabels(cfg),
	})
	if err != nil {
		return types.CacheOutputs{}, err
	}

	ctx.Export("memorystoreEndpoint", out.Endpoint)
	ctx.Export("memorystoreInstanceId", out.InstanceId)

	return types.CacheOutputs{
		Endpoint: out.Endpoint,
	}, nil
}

// NewDatabase creates the Cloud SQL for PostgreSQL instance and returns the
// shared types.DatabaseOutputs (Endpoint as host:port, Port, InstanceId).
// The instance is configured for parity with pkg/aws.NewDatabase: regional
// HA in staging/prod, daily backups with PITR, 7-day retention, IAM-DB
// authentication enabled, and reachable only via the VPC through Private
// Service Access.
func NewDatabase(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.DatabaseOutputs, error) {
	out, err := database.NewCloudSQL(ctx, cfg, &database.CloudSQLInputs{
		PrivateNetwork:       netOut.VpcId.ToStringOutput(),
		PsaReservedRangeName: netOut.ReservedPeeringRangeName,
	})
	if err != nil {
		return types.DatabaseOutputs{}, err
	}
	ctx.Export("cloudSqlEndpoint", out.Endpoint)
	ctx.Export("cloudSqlInstanceId", out.InstanceId)
	return types.DatabaseOutputs{
		Endpoint:   out.Endpoint,
		Port:       out.Port,
		InstanceId: out.InstanceId,
	}, nil
}

// gcpLabels returns the standard label set for GCP resources. GCP label
// values must match [a-z0-9_-]+, so we lowercase and substitute the AWS
// tag values that don't fit. Kept private until a second module needs the
// same helper.
func gcpLabels(cfg *kconfig.Config) pulumi.StringMap {
	env := cfg.Environment
	if env == "" {
		env = string(kconfig.EnvDev)
	}
	return pulumi.StringMap{
		"project":     pulumi.String("kaizen"),
		"environment": pulumi.String(env),
		"managed_by":  pulumi.String("pulumi"),
	}
}


// ─── Stage 4: Streaming + Secrets + CICD ────────────────────────────────────

// NewCICD provisions Artifact Registry repositories for all Kaizen services.
// Returns the same shared types.CICDOutputs shape as pkg/aws.NewCICD so the
// compute layer (and CI dual-push job) consume an identical map regardless
// of cloud provider.
//
// Required cfg fields:
//   - GCPProjectID — GCP project hosting the registry.
//
// Optional cfg fields (all default to safe zero-values):
//   - GCPARLocation — registry location, defaults to "us" multi-region.
//   - GCPCIPushPrincipal — IAM principal granted writer per repo.
//   - GCPRunPullPrincipals — Cloud Run runtime SAs granted reader per repo.
func NewCICD(ctx *pulumi.Context, cfg *kconfig.Config) (types.CICDOutputs, error) {
	if cfg.GCPProjectID == "" {
		return types.CICDOutputs{}, fmt.Errorf(
			"gcp.NewCICD: cfg.GCPProjectID is required when cloudProvider=gcp " +
				"(set via `pulumi config set kaizen-experimentation:gcpProjectId <ID>`)")
	}

	out, err := cicd.NewArtifactRegistryRepositories(ctx, cicd.Config{
		Environment:    cfg.Environment,
		Project:        cfg.GCPProjectID,
		Location:       cfg.GCPARLocation,
		PushPrincipal:  cfg.GCPCIPushPrincipal,
		PullPrincipals: cfg.GCPRunPullPrincipals,
	})
	if err != nil {
		return types.CICDOutputs{}, err
	}

	// Export a single sentinel URL for smoke tests. The AWS facade exports
	// "ecrAssignmentUrl"; the GCP equivalent uses a clearly distinct key so
	// dashboards and stack-export consumers can detect provider drift.
	if url, ok := out.RepositoryURLs["assignment"]; ok {
		ctx.Export("artifactRegistryAssignmentUrl", url)
	}

	return types.CICDOutputs{
		RepositoryURLs: out.RepositoryURLs,
	}, nil
}

// ─── Stage 5: Compute ───────────────────────────────────────────────────────

// NewCompute provisions the GCP compute layer. Phase 1 ships only the
// stateful M4b Policy slice — the eight stateless Cloud Run services land in
// #486 and will populate ServiceEndpoints/ServiceArns on the returned
// ComputeOutputs.
//
// The M4b slice consumes:
//   - The private subnet self-link (held in NetworkOutputs.PrivateSubnetIds[0])
//     for the instance's network interface and reserved internal IP.
//   - The Service Directory namespace resource name (held in
//     NetworkOutputs.ServiceDiscoveryId — see types.NetworkOutputs doc) so
//     m4b-policy registers under kaizen-local.
//   - The GCP region from kconfig; falls back to us-central1 to keep the
//     gcp-dev stack working with a partially-configured Pulumi.gcp-dev.yaml.
//
// Returned ComputeOutputs has the M4b* fields populated and other fields
// zero-valued; the compute-stage early-return in Deploy() exports
// independently. When #486 lands, this function grows the Cloud Run services
// and the early-return marker in Deploy() moves below the edge stage.
func NewCompute(ctx *pulumi.Context, cfg *kconfig.Config, netOut types.NetworkOutputs) (types.ComputeOutputs, error) {
	region := cfg.GCPRegion
	if region == "" {
		region = "us-central1"
	}

	// Pick the first (and only — GCP private subnets are regional) self-link
	// from the network output's array. ApplyT keeps the lazy output chain
	// intact through the topology test.
	privateSubnetSelfLink := netOut.PrivateSubnetIds.ApplyT(func(ids []string) string {
		if len(ids) == 0 {
			return ""
		}
		return ids[0]
	}).(pulumi.StringOutput)

	// types.NetworkOutputs.ServiceDiscoveryId is documented as the Cloud Map
	// namespace ID on AWS and the Service Directory namespace *resource name*
	// on GCP (projects/<P>/locations/<R>/namespaces/<N>). The GCP network
	// facade populates it from servicedirectory.Namespace.ID(), which is
	// exactly that resource name.
	namespaceName := netOut.ServiceDiscoveryId.ToStringOutput()

	m4bOut, err := compute.NewM4bInstance(ctx, &compute.M4bArgs{
		Environment:                   cfg.Environment,
		Region:                        pulumi.String(region).ToStringOutput(),
		PrivateSubnetSelfLink:         privateSubnetSelfLink,
		ServiceDirectoryNamespaceName: namespaceName,
	})
	if err != nil {
		return types.ComputeOutputs{}, err
	}

	ctx.Export("m4bMigName", m4bOut.MigName)
	ctx.Export("m4bInstanceName", m4bOut.InstanceName)
	ctx.Export("m4bEndpoint", m4bOut.Endpoint)
	ctx.Export("m4bServiceDirectoryServiceName", m4bOut.ServiceName)
	ctx.Export("m4bDataDiskName", m4bOut.DataDiskName)

	return types.ComputeOutputs{
		// Phase 1 ships only the M4b slice; Cloud Run services follow in #486.
		// The cluster fields stay zero-valued until then because GCP has no
		// orchestrator analog for Cloud Run (each service is independent).
		M4bInstanceId: m4bOut.InstanceName,
		M4bEndpoint:   m4bOut.Endpoint,
		M4bAsgName:    m4bOut.MigName,
	}, nil
}
