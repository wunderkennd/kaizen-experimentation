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
	"github.com/kaizen-experimentation/infra/pkg/gcp/secrets"
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
		VpcConnectorSelfLink:     connOut.ConnectorSelfLink,
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

// serviceImage resolves a Kaizen service's container image reference from the
// CICD stage's Artifact Registry repository map and pins the :latest tag.
// Returns an error (fail-fast at program-build time) when the registry key is
// absent, rather than letting Cloud Run surface an opaque image-pull failure
// at apply. registryKey is the pkg/gcp/cicd repository key (e.g.
// "orchestration"), NOT the ServiceEndpoints map key.
func serviceImage(cicdOut types.CICDOutputs, registryKey string) (pulumi.StringInput, error) {
	repoURL, ok := cicdOut.RepositoryURLs[registryKey]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.NewCompute: CICDOutputs.RepositoryURLs missing key %q (Artifact Registry repo not provisioned by gcp.NewCICD)",
			registryKey)
	}
	return pulumi.Sprintf("%s:latest", repoURL), nil
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

// NewSecrets provisions the four GCP Secret Manager secrets (database, kafka,
// redis, auth) and narrows pkg/gcp/secrets' richer output to the cross-cloud
// types.SecretsOutputs contract. It mirrors pkg/aws.NewSecrets so Deploy()
// composes either provider identically.
//
// Inputs flow lazily through Pulumi outputs from the upstream Stage 3
// (database, cache) and Stage 4 (streaming) modules:
//   - dbOut.Endpoint        → DatabaseSecret.Host
//   - streamOut.BootstrapBrokers → KafkaSecret.BootstrapBrokers
//   - cacheOut.Endpoint     → RedisSecret.Endpoint
//
// Contract note: types.SecretsOutputs.*SecretRef is documented as the native
// reference — on GCP that is the bare "projects/<P>/secrets/<S>" resource
// name (NOT the version-qualified accessor path). That is exactly what Cloud
// Run's container env secretKeyRef.secret and Secret Manager IAM bindings
// expect, with the version supplied separately ("latest"). We therefore map
// the secrets module's *SecretName (bare path) onto *SecretRef here.
func NewSecrets(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	dbOut types.DatabaseOutputs,
	streamOut types.StreamingOutputs,
	cacheOut types.CacheOutputs,
) (types.SecretsOutputs, error) {
	out, err := secrets.NewSecrets(ctx, cfg, &secrets.SecretsInputs{
		CloudSqlEndpoint:      dbOut.Endpoint,
		KafkaBootstrapBrokers: streamOut.BootstrapBrokers,
		RedisEndpoint:         cacheOut.Endpoint,
	})
	if err != nil {
		return types.SecretsOutputs{}, err
	}
	ctx.Export("secretManagerDatabaseName", out.DatabaseSecretName)
	ctx.Export("secretManagerKafkaName", out.KafkaSecretName)
	// *SecretName → *SecretRef is intentional, not a mapping bug: the GCP
	// "native reference" for the SecretsOutputs contract is the bare
	// `projects/<P>/secrets/<S>` path that Cloud Run's secretKeyRef.secret
	// and Secret Manager IAM bindings consume, NOT the version-qualified
	// `/versions/latest` accessor path. See the function preamble and
	// types.SecretsOutputs docs for the full contract.
	return types.SecretsOutputs{
		DatabaseSecretRef: out.DatabaseSecretName,
		KafkaSecretRef:    out.KafkaSecretName,
		RedisSecretRef:    out.RedisSecretName,
		AuthSecretRef:     out.AuthSecretName,
	}, nil
}

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

// ─── Stage 5: Compute (M4b stateful + Cloud Run service factory) ───────────

// NewCompute provisions the GCP compute layer for Phase 1: the stateful
// M4b Policy slice on GCE + persistent disk + autohealing MIG (issue #487),
// the reusable Cloud Run service factory + a trivial canary so
// `pulumi preview --stack gcp-dev` succeeds end-to-end (issue #486), and the
// per-Kaizen-service Cloud Run deploys as they land (issues #488..#495).
// This PR wires M2 Orchestration (issue #490).
//
// The M4b slice consumes:
//   - The private subnet self-link (held in NetworkOutputs.PrivateSubnetIds[0])
//     for the instance's network interface and reserved internal IP.
//   - The Service Directory namespace resource name (held in
//     NetworkOutputs.ServiceDiscoveryId — see types.NetworkOutputs doc) so
//     m4b-policy registers under kaizen-local.
//
// The stateless Cloud Run services consume:
//   - cicdOut.RepositoryURLs[<registry key>] for the container image.
//   - dbOut.Endpoint / streamOut.BootstrapBrokers as literal env vars so the
//     service can dial Cloud SQL (through the VPC connector) and Redpanda.
//   - secretsOut.*SecretRef for Secret Manager-backed env vars; the factory
//     auto-creates the matching roles/secretmanager.secretAccessor bindings.
//
// The Cloud Run canary uses Google's public hello-world image so the slice
// never blocks on a real Kaizen image build. It is retained until all of
// #488..#495 land, at which point it can be retired.
//
// Returns types.ComputeOutputs with M4b fields AND ServiceEndpoints populated
// for every Cloud Run service (canary + per-service). ClusterId is
// zero-valued — Cloud Run is serverless and M4b is a single instance, not an
// orchestrator cluster.
func NewCompute(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	netOut types.NetworkOutputs,
	cicdOut types.CICDOutputs,
	dbOut types.DatabaseOutputs,
	streamOut types.StreamingOutputs,
	secretsOut types.SecretsOutputs,
	storageOut types.StorageOutputs,
) (types.ComputeOutputs, error) {
	region := cfg.GCPRegion
	if region == "" {
		region = "us-central1"
	}

	// ─── M4b slice (issue #487) ───────────────────────────────────────────
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

	// ─── Cloud Run service factory + canary (issue #486) ──────────────────
	if cfg.GCPProjectID == "" {
		return types.ComputeOutputs{}, fmt.Errorf(
			"gcp.NewCompute: cfg.GCPProjectID is required when cloudProvider=gcp")
	}

	cloudRunInputs := &compute.Inputs{
		Project:                     cfg.GCPProjectID,
		Region:                      cfg.GCPRegion,
		VpcConnectorSelfLink:        netOut.VpcConnectorSelfLink,
		ServiceDirectoryNamespaceID: netOut.ServiceDiscoveryId.ToStringOutput(),
	}

	canary, err := compute.NewCloudRunService(ctx, cfg, cloudRunInputs, "preview-canary",
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
	if err != nil {
		return types.ComputeOutputs{}, err
	}

	endpoints := map[string]pulumi.StringOutput{
		"preview-canary": canary.URL,
	}
	arns := map[string]pulumi.StringOutput{
		"preview-canary": canary.Service.ID().ToStringOutput(),
	}

	ctx.Export("gcpComputeCanaryUrl", canary.URL)
	ctx.Export("gcpComputeCanarySaEmail", canary.ServiceAccountEmail)

	// ─── M2 Orchestration (issue #490) ────────────────────────────────────
	// Stateless coordinator. Needs Cloud SQL for orchestration state and
	// Redpanda for event flow. Service name "m2-orchestration" (matches the
	// AWS Cloud Map name + the dev-m2-orchestration-run SA convention); the
	// ServiceEndpoints map key is "m2-orch" (matches the AWS service key and
	// the #490 acceptance criterion). Default min-instances (0) — the spec's
	// Compute Model marks M2-Orch a stateless orchestrator, NOT an M1/M7
	// cold-start-sensitive service.
	m2OrchImage, err := serviceImage(cicdOut, "orchestration")
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	m2Orch, err := compute.NewCloudRunService(ctx, cfg, cloudRunInputs, "m2-orchestration",
		&compute.Options{
			Image:         m2OrchImage,
			ContainerPort: 50058,
			MinInstances:  0,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "LOG_LEVEL", Value: pulumi.String("info")},
				// Cloud SQL host:port — reachable from the container only
				// through the Serverless VPC Access connector wired by the
				// factory (acceptance criterion #3).
				{Name: "DATABASE_ENDPOINT", Value: dbOut.Endpoint},
				// Redpanda Kafka-protocol bootstrap brokers for event flow.
				{Name: "KAFKA_BOOTSTRAP_BROKERS", Value: streamOut.BootstrapBrokers},
			},
			Secrets: []compute.SecretEnv{
				// secretsOut.*SecretRef is the bare projects/<P>/secrets/<S>
				// path on GCP (see gcp.NewSecrets contract note); the factory
				// grants roles/secretmanager.secretAccessor on each — i.e.
				// "read DB creds, read Kafka creds".
				{EnvName: "DATABASE_SECRET", SecretID: secretsOut.DatabaseSecretRef, Version: "latest"},
				{EnvName: "KAFKA_SECRET", SecretID: secretsOut.KafkaSecretRef, Version: "latest"},
			},
			// Cloud SQL connection scope. The secret-accessor bindings above
			// cover credential reads; this grants the IAM scope to open the
			// connection itself.
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m2-orch"] = m2Orch.URL
	arns["m2-orch"] = m2Orch.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM2OrchUrl", m2Orch.URL)
	ctx.Export("gcpComputeM2OrchSaEmail", m2Orch.ServiceAccountEmail)

	// ─── M6 UI (issue #494) ────────────────────────────────────────────────
	// Next.js 14 SSR. Image comes from the "ui" Artifact Registry repo
	// (#482 created the registry; the CI image pipeline pushes :latest).
	// Default min-instances (0): M6 is request-driven UI traffic, not a
	// p99-SLA gRPC path like M1/M7, so cold starts are acceptable and the
	// scale-to-zero cost saving applies. The factory auto-mints the
	// roles/secretmanager.secretAccessor binding for the auth secret and
	// registers m6-ui in Service Directory so peers can resolve it.
	uiRepoURL, ok := cicdOut.RepositoryURLs["ui"]
	if !ok {
		return types.ComputeOutputs{}, fmt.Errorf(
			"gcp.NewCompute: cicdOut.RepositoryURLs missing the \"ui\" repo required to deploy M6 (#494)")
	}
	m6, err := compute.NewCloudRunService(ctx, cfg, cloudRunInputs, "m6-ui",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", uiRepoURL),
			ContainerPort: 3000, // Next.js SSR port — parity with the AWS M6 Fargate task
			MinInstances:  0,    // default per #494; UI traffic is request-driven
			EnvVars: []compute.EnvVar{
				{Name: "NODE_ENV", Value: pulumi.String("production")},
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				// The one backend that exists at this stage. M4b registered
				// itself in Service Directory above; M6 reaches it via this
				// resolvable endpoint. The remaining MX_*_ENDPOINT vars are
				// added by #488..#493/#495 as those services land.
				{Name: "M4B_POLICY_ENDPOINT", Value: m4bOut.Endpoint},
			},
			Secrets: []compute.SecretEnv{
				// SSR session layer. SecretID is the bare projects/<P>/secrets/<S>
				// path so Cloud Run's secretKeyRef.Secret and the auto-created
				// SecretIamMember both resolve; "latest" tracks rotation.
				{EnvName: "AUTH_SECRET", SecretID: secretsOut.AuthSecretRef, Version: "latest"},
			},
		})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m6"] = m6.URL
	arns["m6"] = m6.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM6Url", m6.URL)
	ctx.Export("gcpComputeM6SaEmail", m6.ServiceAccountEmail)

	// ─── M4a Analysis (issue #492) ────────────────────────────────────────
	// CPU-intensive batch (Rust gRPC). Elevated CPU/memory above the
	// default Cloud Run sizing; gRPC startup probe verifies the standard
	// gRPC Health Checking Protocol responds before traffic is routed.
	// Not in the p99 < 5ms cold-start-sensitive set (only M1/M7 pin
	// min-instances=1); batch analysis tolerates cold starts.
	const m4aPort = 50053
	m4aRepoURL, ok := cicdOut.RepositoryURLs["analysis"]
	if !ok {
		return types.ComputeOutputs{}, fmt.Errorf(
			"gcp.NewCompute: cicdOut.RepositoryURLs missing \"analysis\" repo for M4a (#492)")
	}
	m4a, err := compute.NewCloudRunService(ctx, cfg, cloudRunInputs, "m4a-analysis",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", m4aRepoURL),
			ContainerPort: m4aPort,
			MinInstances:  0,
			// 2 vCPU / 4Gi mirrors the AWS Fargate M4a Tier-2 sizing intent.
			CPULimit:    "2",
			MemoryLimit: "4Gi",
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "DATABASE_ENDPOINT", Value: dbOut.Endpoint},
				{Name: "DATA_BUCKET", Value: storageOut.DataBucketName},
				{Name: "DATA_BUCKET_URI", Value: storageOut.DataBucketRef},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: secretsOut.DatabaseSecretRef, Version: "latest"},
			},
			Buckets:      []pulumi.StringInput{storageOut.DataBucketName},
			ProjectRoles: []string{"roles/cloudsql.client"},
			HealthCheck: &compute.HealthProbe{
				Type:                "grpc",
				Port:                m4aPort,
				InitialDelaySeconds: 10,
				PeriodSeconds:       10,
				FailureThreshold:    6,
			},
		})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m4a"] = m4a.URL
	arns["m4a"] = m4a.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM4aUrl", m4a.URL)
	ctx.Export("gcpComputeM4aSaEmail", m4a.ServiceAccountEmail)

	// ─── M1 Assignment (issue #488) ───────────────────────────────────────
	// The platform's strictest latency budget (p99 < 5ms) — MinInstances=1
	// keeps one warm instance so Cloud Run never cold-starts a request.
	assignmentRepo, ok := cicdOut.RepositoryURLs["assignment"]
	if !ok {
		return types.ComputeOutputs{}, fmt.Errorf(
			"gcp.NewCompute: CICDOutputs.RepositoryURLs is missing the \"assignment\" repo (required for the M1 image)")
	}
	m1Image := assignmentRepo.ApplyT(func(repo string) string {
		return repo + ":latest"
	}).(pulumi.StringOutput)

	m1, err := compute.NewCloudRunService(ctx, cfg, cloudRunInputs, "m1-assignment",
		&compute.Options{
			Image:         m1Image,
			ContainerPort: 8080,
			MinInstances:  1, // p99 < 5ms SLA — no cold starts.
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "GRPC_ADDR", Value: pulumi.String("0.0.0.0:50051")},
				{Name: "HTTP_ADDR", Value: pulumi.String("0.0.0.0:8080")},
				{Name: "KAFKA_BOOTSTRAP_BROKERS", Value: streamOut.BootstrapBrokers},
				{Name: "M4B_ADDR", Value: m4bOut.Endpoint},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: secretsOut.DatabaseSecretRef, Version: "latest"},
				{EnvName: "REDIS_SECRET", SecretID: secretsOut.RedisSecretRef, Version: "latest"},
				{EnvName: "KAFKA_SECRET", SecretID: secretsOut.KafkaSecretRef, Version: "latest"},
			},
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m1"] = m1.URL
	arns["m1"] = m1.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM1Url", m1.URL)
	ctx.Export("gcpComputeM1SaEmail", m1.ServiceAccountEmail)

	// ─── M3 Metrics (issue #491) ──────────────────────────────────────────
	// Go service. Spark SQL orchestration → Delta Lake on GCS; reads metric
	// definitions from Cloud SQL. Default min-instances; batch path, not
	// p99-sensitive. Image from the "metrics" Artifact Registry repo.
	m3RepoURL, ok := cicdOut.RepositoryURLs["metrics"]
	if !ok {
		return types.ComputeOutputs{}, fmt.Errorf(
			"gcp.NewCompute: cicdOut.RepositoryURLs missing \"metrics\" key required for M3 deploy (#491)")
	}
	m3, err := compute.NewCloudRunService(ctx, cfg, cloudRunInputs, "m3-metrics",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", m3RepoURL),
			ContainerPort: 50056,
			MinInstances:  0,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "LOG_LEVEL", Value: pulumi.String("info")},
				{Name: "DATABASE_ENDPOINT", Value: dbOut.Endpoint},
				{Name: "DATA_BUCKET", Value: storageOut.DataBucketName},
				{Name: "DATA_BUCKET_URI", Value: storageOut.DataBucketRef},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: secretsOut.DatabaseSecretRef, Version: "latest"},
			},
			// roles/storage.objectAdmin on the data bucket (#491 AC3).
			Buckets: []pulumi.StringInput{storageOut.DataBucketName},
			// roles/cloudsql.client — connect to Cloud SQL for metric defs.
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m3"] = m3.URL
	arns["m3"] = m3.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM3Url", m3.URL)
	ctx.Export("gcpComputeM3SaEmail", m3.ServiceAccountEmail)

	return types.ComputeOutputs{
		// ClusterId / ClusterName / ClusterArn intentionally zero-valued:
		// Cloud Run is serverless (no cluster); M4b is a single MIG.
		M4bInstanceId:    m4bOut.InstanceName,
		M4bEndpoint:      m4bOut.Endpoint,
		M4bAsgName:       m4bOut.MigName,
		ServiceEndpoints: endpoints,
		ServiceArns:      arns,
	}, nil
}
