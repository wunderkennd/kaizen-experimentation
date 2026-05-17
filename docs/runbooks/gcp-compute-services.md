# GCP Cloud Run Services — Adding a New Service to the Registry

This runbook covers how to add a new Cloud Run service to the GCP infrastructure stack at `infra/pkg/gcp/services/`. It's the user-facing companion to the service-registry refactor (#542, PR #546).

If you're touching `infra/pkg/gcp/gcp.go::NewCompute` to add a new per-service Cloud Run deploy — **don't**. The monolithic `NewCompute` was extracted into a registry of per-service factories in May 2026 specifically to make new service additions touch ~3 files instead of forcing every parallel PR to fight over the same function body. Add your service through the registry; the contract below is short.

## Architecture in 60 seconds

`gcp.NewCompute` is a thin orchestrator (~50 lines of body) that:

1. Provisions the stateful M4b GCE/MIG slice (preamble — not a Cloud Run service).
2. Constructs a `compute.Inputs` value that every Cloud Run service consumes identically.
3. Constructs a `services.StageOutputs` struct bundling every upstream stage output (network, CICD, DB, cache, stream, secrets, storage).
4. Declares a `[]services.RegistryEntry` listing every Cloud Run service to provision, each entry binding a short key (e.g. `"m3"`) to a factory function.
5. Calls `services.Walk(...)` which invokes each factory in registry order and returns a `map[string]*compute.CloudRunService`.
6. Iterates that map to populate `ComputeOutputs.ServiceEndpoints` / `ServiceArns` and emit `ctx.Export("gcpComputeUrl_<key>", ...)` / `gcpComputeSaEmail_<key>` per service.

The stable consumer-facing stack output is `cloudRunUrl_<key>` (emitted by `infra/main.go` from `ServiceEndpoints`). Per-service `gcpComputeUrl_<key>` exports are internal diagnostics.

## The three files you touch

For a new service `M<N>` deploying issue `#<NNN>`:

```
infra/pkg/gcp/services/m<n>_<name>.go           # NEW — the factory
infra/pkg/gcp/services/m<n>_<name>_test.go      # NEW — scoped per-service tests
infra/pkg/gcp/gcp.go                            # MODIFY — add one registry entry
```

Plus, if your service writes a new SD entry, bump the count assertion in `infra/m4b_topology_test.go` (currently `!= 10`; the canonical source-of-truth is `pkg/gcp/services/registry_test.go::TestRegistry_WalkProducesEveryService`).

## Two architectural choices

Before writing the factory, decide:

### 1. Does your service read `m4bOut.Endpoint`?

`m4bOut` is the stateful GCE instance and is **not** part of `StageOutputs` (it's created in `NewCompute`'s preamble, not through the registry). If your service references the M4b endpoint (e.g. for direct dial, like M1 and M6 do), use the **5-param closure-capture pattern**. Otherwise use the standard **4-param signature**. The preview canary is the special case below — it ignores `StageOutputs` entirely because its image is the public hello-world container.

| Signature shape | Used by |
|----------------|---------|
| `func NewMx(ctx, cfg, inputs, stages) (*compute.CloudRunService, error)` | M2-Orch, M3, M4a, M5, M7, M2-Pipe |
| `func NewMx(ctx, cfg, inputs, stages, m4bEndpoint pulumi.StringInput) (*compute.CloudRunService, error)` | M1, M6 |
| `func NewMx(ctx, cfg, inputs) (*compute.CloudRunService, error)` | canary (no upstream stage outputs) |

Only the **4-param** form matches `RegistryEntry.Factory` directly; the **3-param canary** and **5-param** M1/M6 factories must be wrapped in a closure that adapts them to the 4-param `Factory` type. The registry has both shapes today:

```go
// 5-param M1 — closure passes m4bOut.Endpoint captured from NewCompute's scope.
{Key: "m1", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, s services.StageOutputs) (*compute.CloudRunService, error) {
    return services.NewM1Assignment(ctx, cfg, in, s, m4bOut.Endpoint)
}},

// 3-param canary — closure drops the unused stages argument.
{Key: "preview-canary", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, _ services.StageOutputs) (*compute.CloudRunService, error) {
    return services.NewCanary(ctx, cfg, in)
}},
```

If you're modeling a utility service after canary (no stage inputs, fixed public image), use the 3-param shape + a discarding closure. If you're modeling a real Kaizen service, the 4-param form is the right default.

### 2. How does your service mount Secret Manager secrets?

Two valid patterns currently live in the codebase. Choose deliberately:

| Pattern | Used by | When |
|---------|---------|------|
| **Bare `*SecretRef` pass-through** — `{SecretID: stages.Secrets.DatabaseSecretRef, ...}` | M3, M7 | Default. Cloud Run's `secretKeyRef.secret` accepts either bare local ID or full `projects/<P>/secrets/<S>` path. `NewSecrets` returns bare local IDs. |
| **`secretIDForRef` closure** — `{SecretID: secretIDForRef(stages.Secrets.DatabaseSecretRef, "database"), ...}` (local to the factory body) | M5 | When tests need to assert on the bare local ID specifically AND you want a deterministic ApplyT-threaded dependency edge guaranteeing the Cloud Run mount + IAM binding wait for the secret to exist. |

The bare pass-through pattern is simpler and matches main's intended design. Pick the closure only if your tests have strong opinions about the resolved string shape.

## Worked example — hypothetical M8

Say issue #560 asks you to deploy "M8 Observability" — a Rust service exporting Prometheus metrics on port 50060, reading from PostgreSQL, no M4b dep, default min-instances.

### Step 1 — Create the factory

`infra/pkg/gcp/services/m8_observability.go`:

```go
package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM8Observability wires M8 Observability (Rust experimentation-observability,
// issue #560) onto Cloud Run. Prometheus-scrape on port 50060, reads metric
// definitions from Cloud SQL. Default MinInstances (0) — batch path, no p99 SLA.
func NewM8Observability(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["observability"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM8Observability: cicdOut.RepositoryURLs missing \"observability\" key required for M8 deploy (#560)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m8-observability",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: 50060,
			MinInstances:  0,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "DATABASE_ENDPOINT", Value: stages.DB.Endpoint},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
			},
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
```

### Step 2 — Create the scoped test

`infra/pkg/gcp/services/m8_observability_test.go`:

```go
package services

import (
	"strings"
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// m8StageOutputs returns the minimal StageOutputs M8 reads. ONLY the
// "observability" repo, ONLY the inputs the factory actually consumes —
// do NOT cross-pollinate with other services' repo keys.
func m8StageOutputs() StageOutputs {
	return StageOutputs{
		CICD: types.CICDOutputs{
			RepositoryURLs: map[string]pulumi.StringOutput{
				"observability": pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/observability").ToStringOutput(),
			},
		},
		DB: types.DatabaseOutputs{
			Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
		},
	}
}

func TestM8Observability_Wiring(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM8Observability(ctx, scopedCfg(), scopedInputs(), m8StageOutputs())
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("NewM8Observability failed: %v", err)
	}

	svcs := mocks.byType("gcp:cloudrunv2/service:Service")
	if len(svcs) != 1 {
		t.Fatalf("expected 1 Cloud Run service, got %d", len(svcs))
	}
	svc := svcs[0]
	if name := svc.Inputs["name"].StringValue(); name != "kaizen-dev-m8-observability" {
		t.Errorf("service name = %q, want kaizen-dev-m8-observability", name)
	}
	tmpl := svc.Inputs["template"].ObjectValue()
	c := tmpl["containers"].ArrayValue()[0].ObjectValue()
	if port := c["ports"].ObjectValue()["containerPort"]; port.NumberValue() != 50060 {
		t.Errorf("containerPort = %v, want 50060", port)
	}
}

func TestM8Observability_MissingObservabilityRepoFails(t *testing.T) {
	bad := m8StageOutputs()
	bad.CICD = types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}}
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		_, err := NewM8Observability(ctx, scopedCfg(), scopedInputs(), bad)
		return err
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err == nil || !strings.Contains(err.Error(), "observability") {
		t.Errorf("expected missing-observability error, got %v", err)
	}
}
```

Reuse `scopedMocks` / `scopedInputs` / `scopedCfg` from `m3_metrics_test.go` — they're in the same package, no redeclaration.

### Step 3 — Add one registry entry

In `infra/pkg/gcp/gcp.go::NewCompute`, append to the `registry` slice (the entry's position in the slice determines provisioning order but not Pulumi dependency order — keep historical order or alphabetize):

```go
{Key: "m8", Factory: services.NewM8Observability},
```

That's it for code. The walker picks it up; the test in `m4b_topology_test.go` (currently `!= 10`) needs bumping to `!= 11`, and `registry_test.go::TestRegistry_WalkProducesEveryService` needs the count updated from `9` to `10` and `"m8"` added to the expected-keys slice.

## Common gotchas

1. **Don't cross-pollinate scoped test fixtures.** The whole point of the refactor was to eliminate fixtures that declare every other service's repo keys. If your test asserts on M8's wiring, your fixture should populate `"observability"` only — NOT `"metrics"`, `"flags"`, `"assignment"`, etc. The aggregator test in `registry_test.go` exercises the full multi-service walk; that's where the cross-product assertions live.

2. **Don't reach into `pkg/aws/compute/services.go`** to mirror an AWS service contract. The two cloud arms have parity at the `types.*Outputs` layer; below that, each provider's per-service factory is free to use cloud-native idioms.

3. **The factory's `compute.Options` is faithful to main's existing service blocks.** When in doubt, grep an existing factory (e.g. `m3_metrics.go`) and copy the option set, then strip / add per your service's needs.

4. **Bumping the SD count assertion** is two locations: `m4b_topology_test.go` (full Deploy() proof, includes M4b's own SD entry, so it's N+1 where N is the registry length) and `registry_test.go` (just the Cloud Run count, N).

5. **Avoid breaking the export-key contract for downstream consumers.** The stable consumer API is `cloudRunUrl_<key>` from `main.go`. New services get this for free since `main.go` iterates `ServiceEndpoints`. The per-service `gcpComputeUrl_<key>` / `gcpComputeSaEmail_<key>` exports inside `NewCompute` are internal diagnostics — fine for any new key.

## See also

- `infra/pkg/gcp/services/` — all factory implementations.
- `infra/pkg/gcp/services/registry.go` — `RegistryEntry` + `Walk`.
- `infra/pkg/gcp/services/registry_test.go` — canonical aggregator test for the full 9-service walk.
- `infra/pkg/gcp/compute/compute.go` — the lower-level `NewCloudRunService` factory + `Options` + `Inputs` types.
- `docs/superpowers/plans/2026-05-16-service-registry-refactor.md` — the refactor plan (architecture rationale).
- Issue #542, PR #546 — the refactor itself.
