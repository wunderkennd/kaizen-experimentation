// Package compute is the GCP compute factory. It exposes a single
// per-service helper, NewCloudRunService, that mirrors the AWS ECS Fargate
// task-def factory (pkg/aws/compute/services.go::newFargateService) so the
// GCP and AWS provider arms expose the same operational shape:
//
//	1. A dedicated runtime identity (Workload Identity service account here,
//	   ECS task role on AWS).
//	2. Consistent VPC wiring (Serverless VPC Access connector here, awsvpc
//	   network mode + private-subnet ENIs on AWS).
//	3. Service discovery registration (Service Directory endpoint here,
//	   Cloud Map service on AWS).
//	4. Secret injection from the cloud-native secret store
//	   (Secret Manager here, Secrets Manager on AWS).
//	5. Bucket / project IAM bindings driven by per-service options.
//
// Out of scope for this PR (issue #486):
//   - Per-Kaizen-service deploys (M1, M2, ..., M7) — issues #10..#17.
//   - GCE M4b stateful service — separate issue under the same sprint.
//
// Topology test guarantees (per spec gap mitigation #2): every Cloud Run
// service registered through this helper has a corresponding Workload
// Identity service account, IAM bindings for every secret/bucket/project
// role declared in opts, and a Service Directory endpoint registration.
package compute

import (
	"fmt"
	"sort"
	"strings"

	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/cloudrunv2"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/projects"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/secretmanager"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/serviceaccount"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/servicedirectory"
	"github.com/pulumi/pulumi-gcp/sdk/v8/go/gcp/storage"
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/config"
)

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

// Inputs holds the cross-stage values every Cloud Run service in the same
// stack consumes identically: project, region, environment, the Serverless
// VPC Access connector, and the Service Directory namespace.
//
// One Inputs value is constructed per Pulumi stack (in pkg/gcp/gcp.go's
// NewCompute aggregator) and reused across every NewCloudRunService call.
type Inputs struct {
	// Project is the GCP project ID hosting all compute resources.
	// Maps to cfg.GCPProjectID.
	Project string

	// Region is the GCP region for Cloud Run + the SD namespace, e.g.
	// "us-central1". Cloud Run is regional; the helper uses this verbatim.
	Region string

	// VpcConnectorSelfLink is the connector resource consumed by every
	// Cloud Run service's template.vpcAccess.connector. Sourced from
	// types.NetworkOutputs.VpcConnectorSelfLink, populated by the network
	// stage in pkg/gcp/network/vpc_connector.go.
	VpcConnectorSelfLink pulumi.StringOutput

	// ServiceDirectoryNamespaceID identifies the namespace each service
	// registers under as an SD service + endpoint. Sourced from
	// types.NetworkOutputs.ServiceDiscoveryId, populated by the network
	// stage in pkg/gcp/network/service_directory.go.
	ServiceDirectoryNamespaceID pulumi.StringOutput
}

// EnvVar models a single literal environment variable for a Cloud Run
// container. Use SecretEnv (below) for env vars whose value comes from
// Secret Manager.
type EnvVar struct {
	// Name is the env var name. Required, must be a C identifier.
	Name string
	// Value is a literal string, late-bound via pulumi.StringInput so
	// callers can thread Pulumi outputs (endpoints, bucket names, etc.).
	Value pulumi.StringInput
}

// SecretEnv binds an env var name to a Secret Manager secret version. The
// helper auto-creates the SecretIamMember binding granting the service's
// runtime SA roles/secretmanager.secretAccessor on the secret resource.
//
// SecretID is the local secret ID (e.g. "kaizen-dev-database") within the
// stack's project — NOT the version-qualified path. Cloud Run's
// secretKeyRef wants the bare secret ID (or a fully-qualified
// projects/<P>/secrets/<S> path for cross-project references). The helper
// passes Version through verbatim; "latest" is the conventional default.
type SecretEnv struct {
	// EnvName is the env var the container reads (required).
	EnvName string
	// SecretID is the local Secret Manager secret ID (required).
	SecretID pulumi.StringInput
	// Version is the secret version to mount; "latest" is recommended for
	// rotation. Empty defaults to "latest".
	Version string
}

// Options configures a single Cloud Run service. Passed to NewCloudRunService
// per service. Fields not set get safe defaults documented inline.
type Options struct {
	// Image is the container image URL (e.g.
	// "us-central1-docker.pkg.dev/<P>/kaizen/m1-assignment:latest" pulled
	// from CICDOutputs.RepositoryURLs). Required.
	Image pulumi.StringInput

	// ContainerPort is the port the container listens on (env $PORT). 0
	// defaults to 8080 — Cloud Run's documented default.
	ContainerPort int

	// MinInstances controls cold-start behavior. Defaults to 0; M1 and M7
	// override to 1 to hold the p99 < 5ms SLA per the multi-cloud spec
	// (Compute Model → Cold starts).
	MinInstances int

	// MaxInstances caps autoscaling. 0 defers to the Cloud Run default
	// (calculated from project quota; typically 100).
	MaxInstances int

	// EnvVars are static environment variables threaded into the container.
	EnvVars []EnvVar

	// Secrets are env vars whose value comes from Secret Manager. The
	// helper creates one SecretIamMember binding per entry granting the
	// runtime SA roles/secretmanager.secretAccessor.
	Secrets []SecretEnv

	// Buckets lists Cloud Storage bucket names the runtime SA needs
	// roles/storage.objectAdmin on. Pass StorageOutputs.DataBucketName /
	// MlflowBucketName / LogsBucketName as needed. Empty by default.
	Buckets []pulumi.StringInput

	// ProjectRoles lists project-level roles the runtime SA needs, e.g.
	// "roles/cloudsql.client" for Cloud SQL access, "roles/redis.editor"
	// for Memorystore, "roles/cloudtrace.agent" for Cloud Trace.
	ProjectRoles []string

	// ServiceID overrides the Service Directory service ID. Defaults to
	// `name`. Constraint: 1-63 chars, [a-z][a-z0-9-]*.
	ServiceID string

	// AllowPublicInvoke, if true, grants roles/run.invoker to allUsers,
	// making the service public. Defaults false (Cloud Run gates on IAM
	// when ingress allows external callers; M6 UI behind a load balancer
	// is the typical case where this would be true).
	AllowPublicInvoke bool
}

// CloudRunService is the per-service output bundle returned to callers
// (typically the GCP NewCompute aggregator). Tests inspect these fields
// directly to verify shape; production callers thread URL into
// ComputeOutputs.ServiceEndpoints.
type CloudRunService struct {
	// Name is the logical service name passed to NewCloudRunService
	// (e.g. "m1-assignment"). Convenience for callers building maps.
	Name string

	// Service is the Cloud Run resource. Pass it to
	// pulumi.DependsOn(...) when wiring downstream resources.
	Service *cloudrunv2.Service

	// URL is the Cloud Run service URL (https://<svc>-<hash>-<region>.a.run.app).
	// This is what callers stuff into ComputeOutputs.ServiceEndpoints[name].
	URL pulumi.StringOutput

	// ServiceAccount is the per-service Workload Identity SA resource.
	ServiceAccount *serviceaccount.Account

	// ServiceAccountEmail is the SA email the WI binding sets on the
	// Cloud Run revision template.
	ServiceAccountEmail pulumi.StringOutput

	// ServiceAccountMember is the IAM principal form ("serviceAccount:<email>").
	// Tests use this to verify role bindings reference the right principal.
	ServiceAccountMember pulumi.StringOutput

	// SDService is the Service Directory parent of the endpoint. Created
	// once per Cloud Run service so endpoints can co-exist (e.g., per-region).
	SDService *servicedirectory.Service

	// SDEndpoint is the Service Directory endpoint resource registered
	// under SDService. Tests assert on its presence per acceptance
	// criterion 3.
	SDEndpoint *servicedirectory.Endpoint
}

// ---------------------------------------------------------------------------
// Constructor
// ---------------------------------------------------------------------------

// NewCloudRunService provisions one Cloud Run service plus everything
// needed for it to function safely:
//
//	1. A dedicated Workload Identity service account.
//	2. roles/secretmanager.secretAccessor on each opts.Secrets entry.
//	3. roles/storage.objectAdmin on each opts.Buckets entry.
//	4. Each opts.ProjectRoles role bound at the project level to the SA.
//	5. The Cloud Run service itself, with the SA on the revision template,
//	   the VPC connector wired, env vars + secret env vars set, and
//	   min/max instances configured.
//	6. A Service Directory service + endpoint that resolves to the Cloud
//	   Run URL so other services can discover it via the namespace from
//	   inputs.ServiceDirectoryNamespaceID.
//
// Returns the bundle in CloudRunService. The caller composes URL into
// types.ComputeOutputs.ServiceEndpoints.
func NewCloudRunService(
	ctx *pulumi.Context,
	cfg *config.Config,
	inputs *Inputs,
	name string,
	opts *Options,
	resourceOpts ...pulumi.ResourceOption,
) (*CloudRunService, error) {
	if err := validateInputs(cfg, inputs, name, opts); err != nil {
		return nil, err
	}

	// Apply defaults in a copy so the caller's struct is not mutated.
	o := normalizeOptions(*opts, name)

	// ── Runtime service account ────────────────────────────────────────
	saAccountID, err := saAccountID(cfg.Environment, name)
	if err != nil {
		return nil, err
	}

	sa, err := serviceaccount.NewAccount(
		ctx,
		fmt.Sprintf("kaizen-%s-%s-sa", cfg.Environment, name),
		&serviceaccount.AccountArgs{
			Project:     pulumi.String(cfg.GCPProjectID),
			AccountId:   pulumi.String(saAccountID),
			DisplayName: pulumi.Sprintf("Kaizen %s %s runtime", cfg.Environment, name),
			Description: pulumi.String("Workload Identity SA for Kaizen Cloud Run service " + name),
		},
		resourceOpts...,
	)
	if err != nil {
		return nil, fmt.Errorf("create service account for %s: %w", name, err)
	}
	saMember := sa.Email.ApplyT(func(email string) string {
		return "serviceAccount:" + email
	}).(pulumi.StringOutput)

	// ── IAM: project-level roles ───────────────────────────────────────
	for _, role := range sortedUnique(o.ProjectRoles) {
		_, err := projects.NewIAMMember(
			ctx,
			fmt.Sprintf("kaizen-%s-%s-role-%s", cfg.Environment, name, slug(role)),
			&projects.IAMMemberArgs{
				Project: pulumi.String(cfg.GCPProjectID),
				Role:    pulumi.String(role),
				Member:  saMember,
			},
			resourceOpts...,
		)
		if err != nil {
			return nil, fmt.Errorf("bind project role %s to %s SA: %w", role, name, err)
		}
	}

	// ── IAM: per-secret accessor bindings ──────────────────────────────
	for i, sec := range o.Secrets {
		_, err := secretmanager.NewSecretIamMember(
			ctx,
			fmt.Sprintf("kaizen-%s-%s-secret-%d", cfg.Environment, name, i),
			&secretmanager.SecretIamMemberArgs{
				Project:  pulumi.String(cfg.GCPProjectID),
				SecretId: sec.SecretID,
				Role:     pulumi.String("roles/secretmanager.secretAccessor"),
				Member:   saMember,
			},
			resourceOpts...,
		)
		if err != nil {
			return nil, fmt.Errorf("bind secret accessor for %s/%s: %w", name, sec.EnvName, err)
		}
	}

	// ── IAM: per-bucket object admin bindings ──────────────────────────
	for i, bucket := range o.Buckets {
		_, err := storage.NewBucketIAMMember(
			ctx,
			fmt.Sprintf("kaizen-%s-%s-bucket-%d", cfg.Environment, name, i),
			&storage.BucketIAMMemberArgs{
				Bucket: bucket,
				Role:   pulumi.String("roles/storage.objectAdmin"),
				Member: saMember,
			},
			resourceOpts...,
		)
		if err != nil {
			return nil, fmt.Errorf("bind bucket %d for %s: %w", i, name, err)
		}
	}

	// ── Cloud Run service ──────────────────────────────────────────────
	envs := buildContainerEnvs(o.EnvVars, o.Secrets)

	scaling := &cloudrunv2.ServiceTemplateScalingArgs{
		MinInstanceCount: pulumi.Int(o.MinInstances),
	}
	if o.MaxInstances > 0 {
		scaling.MaxInstanceCount = pulumi.Int(o.MaxInstances)
	}

	svc, err := cloudrunv2.NewService(
		ctx,
		fmt.Sprintf("kaizen-%s-%s-run", cfg.Environment, name),
		&cloudrunv2.ServiceArgs{
			Name:     pulumi.String(fmt.Sprintf("kaizen-%s-%s", cfg.Environment, name)),
			Project:  pulumi.String(cfg.GCPProjectID),
			Location: pulumi.String(cfg.GCPRegion),
			// INGRESS_TRAFFIC_INTERNAL_AND_CLOUD_LOAD_BALANCING is the
			// safer default — service-to-service traffic stays on the
			// VPC; only the load balancer (Phase 3) can reach it
			// publicly. Mirrors AWS ECS services living on private
			// subnets with the ALB in front.
			Ingress: pulumi.String("INGRESS_TRAFFIC_INTERNAL_LOAD_BALANCER"),
			Labels: pulumi.StringMap{
				"project":     pulumi.String("kaizen"),
				"environment": pulumi.String(cfg.Environment),
				"service":     pulumi.String(name),
				"managed_by":  pulumi.String("pulumi"),
			},
			Template: &cloudrunv2.ServiceTemplateArgs{
				ServiceAccount: sa.Email,
				Scaling:        scaling,
				VpcAccess: &cloudrunv2.ServiceTemplateVpcAccessArgs{
					Connector: inputs.VpcConnectorSelfLink,
					// PRIVATE_RANGES_ONLY routes only RFC-1918 traffic
					// through the connector, keeping public egress (e.g.
					// to Artifact Registry image pulls) on the
					// platform-default path. ALL_TRAFFIC would force
					// every request through the connector and hit its
					// throughput limit.
					Egress: pulumi.String("PRIVATE_RANGES_ONLY"),
				},
				Containers: cloudrunv2.ServiceTemplateContainerArray{
					&cloudrunv2.ServiceTemplateContainerArgs{
						Image: o.Image,
						Ports: &cloudrunv2.ServiceTemplateContainerPortsArgs{
							ContainerPort: pulumi.Int(o.ContainerPort),
						},
						Envs: envs,
					},
				},
			},
		},
		resourceOpts...,
	)
	if err != nil {
		return nil, fmt.Errorf("create Cloud Run service %s: %w", name, err)
	}

	// Optional public-invoke binding (M6 UI behind a load balancer).
	if o.AllowPublicInvoke {
		_, err := cloudrunv2.NewServiceIamMember(
			ctx,
			fmt.Sprintf("kaizen-%s-%s-public", cfg.Environment, name),
			&cloudrunv2.ServiceIamMemberArgs{
				Project:  pulumi.String(cfg.GCPProjectID),
				Location: pulumi.String(cfg.GCPRegion),
				Name:     svc.Name,
				Role:     pulumi.String("roles/run.invoker"),
				Member:   pulumi.String("allUsers"),
			},
			resourceOpts...,
		)
		if err != nil {
			return nil, fmt.Errorf("public invoke binding for %s: %w", name, err)
		}
	}

	// ── Service Directory: service + endpoint ──────────────────────────
	// The SD service sits below the namespace. The endpoint sits below
	// the service and carries the resolvable URL. Other Cloud Run
	// services discover this via Service Directory's HTTP API or, when
	// enabled, the auto-published private DNS zone.
	sdSvc, err := servicedirectory.NewService(
		ctx,
		fmt.Sprintf("kaizen-%s-%s-sd-svc", cfg.Environment, name),
		&servicedirectory.ServiceArgs{
			Namespace: inputs.ServiceDirectoryNamespaceID,
			ServiceId: pulumi.String(o.ServiceID),
			Metadata: pulumi.StringMap{
				"environment": pulumi.String(cfg.Environment),
				"managed_by":  pulumi.String("pulumi"),
			},
		},
		resourceOpts...,
	)
	if err != nil {
		return nil, fmt.Errorf("create SD service for %s: %w", name, err)
	}

	// Cloud Run URLs are HTTPS; SD endpoints store host/port. We strip
	// the scheme and default to 443 so callers can resolve a routable
	// host:port pair from the namespace.
	sdEndpoint, err := servicedirectory.NewEndpoint(
		ctx,
		fmt.Sprintf("kaizen-%s-%s-sd-ep", cfg.Environment, name),
		&servicedirectory.EndpointArgs{
			Service:    sdSvc.ID(),
			EndpointId: pulumi.String("primary"),
			Address:    svc.Uri.ApplyT(stripScheme).(pulumi.StringOutput),
			Port:       pulumi.Int(443),
			Metadata: pulumi.StringMap{
				"protocol":    pulumi.String("https"),
				"backend":     pulumi.String("cloud-run"),
				"environment": pulumi.String(cfg.Environment),
			},
		},
		resourceOpts...,
	)
	if err != nil {
		return nil, fmt.Errorf("create SD endpoint for %s: %w", name, err)
	}

	return &CloudRunService{
		Name:                 name,
		Service:              svc,
		URL:                  svc.Uri,
		ServiceAccount:       sa,
		ServiceAccountEmail:  sa.Email,
		ServiceAccountMember: saMember,
		SDService:            sdSvc,
		SDEndpoint:           sdEndpoint,
	}, nil
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// validateInputs is a single-spot check that catches misconfiguration at
// program-build time rather than apply time (where Pulumi would surface a
// less-actionable Cloud Run / IAM API error).
func validateInputs(cfg *config.Config, inputs *Inputs, name string, opts *Options) error {
	if cfg == nil {
		return fmt.Errorf("compute.NewCloudRunService: cfg must not be nil")
	}
	if cfg.GCPProjectID == "" {
		// Required to scope project-level IAM bindings and Cloud Run
		// resource args. The GCP facade enforces presence at the
		// stage boundary (gcp.NewCICD already does this), so a missing
		// value here means the helper was called outside a GCP stack.
		return fmt.Errorf("compute.NewCloudRunService: cfg.GCPProjectID must not be empty")
	}
	if cfg.Environment == "" {
		return fmt.Errorf("compute.NewCloudRunService: cfg.Environment must not be empty")
	}
	if cfg.GCPRegion == "" {
		return fmt.Errorf("compute.NewCloudRunService: cfg.GCPRegion must not be empty (set kaizen-experimentation:gcpRegion)")
	}
	if inputs == nil {
		return fmt.Errorf("compute.NewCloudRunService: inputs must not be nil")
	}
	if name == "" {
		return fmt.Errorf("compute.NewCloudRunService: name must not be empty")
	}
	if !isValidServiceName(name) {
		return fmt.Errorf("compute.NewCloudRunService: name %q must match [a-z][a-z0-9-]* and be 1..49 chars", name)
	}
	if opts == nil {
		return fmt.Errorf("compute.NewCloudRunService: opts must not be nil")
	}
	if opts.Image == nil {
		return fmt.Errorf("compute.NewCloudRunService: opts.Image is required for service %s", name)
	}
	if opts.MinInstances < 0 {
		return fmt.Errorf("compute.NewCloudRunService: opts.MinInstances must be >= 0 (got %d)", opts.MinInstances)
	}
	if opts.MaxInstances < 0 {
		return fmt.Errorf("compute.NewCloudRunService: opts.MaxInstances must be >= 0 (got %d)", opts.MaxInstances)
	}
	if opts.MaxInstances > 0 && opts.MaxInstances < opts.MinInstances {
		return fmt.Errorf("compute.NewCloudRunService: opts.MaxInstances (%d) must be >= MinInstances (%d)",
			opts.MaxInstances, opts.MinInstances)
	}
	for _, sec := range opts.Secrets {
		if sec.EnvName == "" || sec.SecretID == nil {
			return fmt.Errorf("compute.NewCloudRunService: each opts.Secrets entry needs EnvName + SecretID (service %s)", name)
		}
	}
	for _, env := range opts.EnvVars {
		if env.Name == "" || env.Value == nil {
			return fmt.Errorf("compute.NewCloudRunService: each opts.EnvVars entry needs Name + Value (service %s)", name)
		}
	}
	return nil
}

// normalizeOptions returns a copy of opts with defaults applied. Defaults:
//   - ContainerPort: 8080 (Cloud Run platform default).
//   - ServiceID: name (matches the Cloud Run service name).
func normalizeOptions(opts Options, name string) Options {
	if opts.ContainerPort == 0 {
		opts.ContainerPort = 8080
	}
	if opts.ServiceID == "" {
		opts.ServiceID = name
	}
	return opts
}

// buildContainerEnvs assembles the env array consumed by the Cloud Run
// container. Literal env vars come first; secret-backed env vars are
// appended via valueSource.secretKeyRef so Cloud Run mounts them at start.
func buildContainerEnvs(literals []EnvVar, secrets []SecretEnv) cloudrunv2.ServiceTemplateContainerEnvArray {
	envs := cloudrunv2.ServiceTemplateContainerEnvArray{}
	for _, e := range literals {
		envs = append(envs, &cloudrunv2.ServiceTemplateContainerEnvArgs{
			Name:  pulumi.String(e.Name),
			Value: e.Value,
		})
	}
	for _, s := range secrets {
		version := s.Version
		if version == "" {
			version = "latest"
		}
		envs = append(envs, &cloudrunv2.ServiceTemplateContainerEnvArgs{
			Name: pulumi.String(s.EnvName),
			ValueSource: &cloudrunv2.ServiceTemplateContainerEnvValueSourceArgs{
				SecretKeyRef: &cloudrunv2.ServiceTemplateContainerEnvValueSourceSecretKeyRefArgs{
					Secret:  s.SecretID,
					Version: pulumi.String(version),
				},
			},
		})
	}
	return envs
}

// saAccountID returns the GCP service account local ID for a service.
// Format: <env>-<name>-run, e.g. "dev-m1-assignment-run". Caps at 30 chars
// per GCP's accountId constraint ([a-z][a-z0-9-]{4,28}[a-z0-9]).
//
// The error path catches names that would overflow once env is prepended,
// so the misconfiguration is visible at program-build time rather than as
// an opaque Cloud IAM 400 at apply.
func saAccountID(env, name string) (string, error) {
	id := fmt.Sprintf("%s-%s-run", env, name)
	if len(id) < 6 {
		return "", fmt.Errorf("compute: derived service account id %q is shorter than the 6-char GCP minimum", id)
	}
	if len(id) > 30 {
		return "", fmt.Errorf("compute: derived service account id %q is %d chars, exceeds the 30-char GCP maximum (env=%q, name=%q)",
			id, len(id), env, name)
	}
	return id, nil
}

// isValidServiceName accepts only DNS-label-compatible service names (which
// is also the SD service-id constraint). Cloud Run resource names extend
// to 49 chars in practice once the kaizen-<env>- prefix is added; we
// enforce the same upper bound on `name` so the resource name never
// overflows the 63-char Cloud Run limit even with "staging" as env.
func isValidServiceName(s string) bool {
	if len(s) == 0 || len(s) > 48 {
		return false
	}
	first := s[0]
	if first < 'a' || first > 'z' {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch {
		case c >= 'a' && c <= 'z':
		case c >= '0' && c <= '9':
		case c == '-':
		default:
			return false
		}
	}
	return true
}

// slug rewrites IAM role strings (e.g. "roles/cloudsql.client") into a
// Pulumi-resource-name-safe suffix ("cloudsql-client"). Used to keep
// per-binding resource names stable and readable.
func slug(s string) string {
	r := strings.NewReplacer("/", "-", ".", "-", "_", "-")
	out := strings.ToLower(r.Replace(s))
	// Strip any leading "roles-" prefix from the standard role slug so the
	// resulting Pulumi resource name reads "kaizen-dev-m1-role-cloudsql-client"
	// rather than "kaizen-dev-m1-role-roles-cloudsql-client".
	out = strings.TrimPrefix(out, "roles-")
	return out
}

// sortedUnique returns roles in sorted order with duplicates removed.
// Determinism makes Pulumi diffs stable across runs even if the caller
// hands us the same role twice or in a different order between two
// `pulumi up` invocations.
func sortedUnique(in []string) []string {
	seen := make(map[string]struct{}, len(in))
	out := make([]string, 0, len(in))
	for _, s := range in {
		if _, ok := seen[s]; ok {
			continue
		}
		seen[s] = struct{}{}
		out = append(out, s)
	}
	sort.Strings(out)
	return out
}

// stripScheme removes the "https://" / "http://" prefix from a Cloud Run
// service URL so the address can be stored in a Service Directory
// endpoint (which expects a bare host, not a URL).
func stripScheme(u string) string {
	for _, prefix := range []string{"https://", "http://"} {
		if strings.HasPrefix(u, prefix) {
			return strings.TrimPrefix(u, prefix)
		}
	}
	return u
}
