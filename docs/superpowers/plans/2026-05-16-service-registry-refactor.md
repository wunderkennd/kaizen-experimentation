# Service-Registry Refactor of `gcp.NewCompute` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the 525-line god-aggregator `gcp.NewCompute` into a registry-walked composition of 9 per-service factory files, each with typed `Deps`, so future per-service work touches one file (not the monolith) and tests no longer cross-pollinate fixtures across services.

**Architecture:** Strangler-fig migration. Each task extracts one service from `pkg/gcp/gcp.go` into `pkg/gcp/services/<name>.go` and replaces its inline block in `NewCompute` with a call to the new factory. Existing tests stay green at every commit — no behavior change, only structural. The registry + slimmed `NewCompute` + `StageOutputs` struct land in the final three tasks, after all services have been extracted.

**Tech Stack:** Go 1.26, Pulumi Go SDK v3.229, `pulumi-gcp` v8 SDK, `pulumi.WithMocks` for topology tests. Resolves issue #542.

**Issue:** #542 (`Refactor gcp.NewCompute to service-registry pattern (post-#540)`)

**Branch:** `agent-4/refactor/542-service-registry`

---

## Authoritative source snapshots

The line ranges referenced below are from `origin/main @ 719edc9` (post-#540). Each extraction task quotes its source range; re-verify with `git log --oneline origin/main -1` before starting.

| Service | Inline block in `gcp.NewCompute` | Helper functions |
|---------|----------------------------------|------------------|
| canary  | `pkg/gcp/gcp.go:412-426` | — |
| M2-Orch | `pkg/gcp/gcp.go:449-460` (approx — see source) | — |
| M6 UI   | `pkg/gcp/gcp.go:498-520` | — |
| M4a     | `pkg/gcp/gcp.go:539-568` | — |
| M1      | `pkg/gcp/gcp.go:587-608` | — |
| M3      | `pkg/gcp/gcp.go:637-671` | — |
| M7      | `pkg/gcp/gcp.go:696-715` | — |
| M2-Pipe | `pkg/gcp/gcp.go:730-737` (call site) | `newM2PipelineService` at `pkg/gcp/gcp.go:755-797` |
| M5      | `pkg/gcp/gcp.go:742-753` (call site) | `newM5ManagementService` at `pkg/gcp/gcp.go:810-873`, closure `secretIDForRef` lives inside the body |

`M4b` (the stateful GCE/MIG slice) is **not** in scope — it isn't a Cloud Run service. It stays in `NewCompute`'s preamble unchanged.

---

## File Structure

**New files (created by this plan):**

```
infra/pkg/gcp/services/
  deps.go              # StageOutputs + 9 per-service M*Deps structs
  registry.go          # registry slice + walker (final task)
  registry_test.go     # aggregator test verifying full walk
  canary.go            # NewCanary
  m1_assignment.go     # NewM1Assignment
  m2_orchestration.go  # NewM2Orchestration
  m2_pipeline.go       # NewM2Pipeline (moves newM2PipelineService body verbatim)
  m3_metrics.go        # NewM3Metrics
  m4a_analysis.go      # NewM4aAnalysis
  m5_management.go     # NewM5Management (moves newM5ManagementService body verbatim, including closure)
  m6_ui.go             # NewM6UI
  m7_flags.go          # NewM7Flags
  # per-service tests land here in Task 13
```

**Files modified (incremental, across tasks):**

- `infra/pkg/gcp/gcp.go` — each extraction trims one inline block; final task slims `NewCompute` to a registry walk.
- `infra/main.go` — final task swaps the 9-arg call site to `gcp.NewCompute(ctx, cfg, stages)`.
- `infra/m4b_topology_test.go` — final task replaces hardcoded SD-count `!= 10` with `len(registry) + 1` (the +1 is M4b itself).

**Files deleted (final task):**

- `infra/m1_topology_test.go`, `infra/gcp_compute_m6_test.go`, `infra/test/gcp_m7_flags_topology_test.go`, `infra/test/gcp_m2_pipeline_topology_test.go`, `infra/test/gcp_m2orch_topology_test.go`, `infra/gcp_m5_topology_test.go`, `infra/pkg/gcp/gcp_test.go` — superseded by scoped tests next to each factory.
- `infra/fullstack_gcp_test.go` — `TestGCPCompute_M4a*` (3 functions) and `gcpComputeInputs` helper deleted; the file's other Deploy() tests remain.

---

## Execution discipline

1. **Tests stay green at every commit.** Run `cd infra && go test ./... 2>&1 | grep -E 'FAIL|^ok'` after every task. The known pre-existing AWS storage panic (`TestFullStackDeploy` — `pulumi.ID` type assertion) is ignored — every other test must pass.
2. **No behavior change in tasks 1–11.** Extractions are pure relocations: copy the inline block verbatim, wrap in `NewMxxx(...)`, replace the inline block with a call. `git diff origin/main -- 'infra/pkg/gcp/services/*.go' infra/pkg/gcp/gcp.go` should show only file moves + the registry call.
3. **Strangler-fig order.** `NewCompute` keeps its current signature and body until Task 13. Every extracted factory is called from inside the existing `NewCompute` body. The final task is the big switch.
4. **Use `git mv`-equivalent care.** Each extraction is one `Write` (new file) + one `Edit` (gcp.go inline block → factory call). No squash, no rebase between tasks.

---

### Task 1: Create services package + define `StageOutputs` and per-service `Deps` structs

**Files:**
- Create: `infra/pkg/gcp/services/deps.go`
- Create: `infra/pkg/gcp/services/deps_test.go`

**Why:** Locks the contract every extraction will satisfy. No behavior change. Reviewable in isolation.

- [ ] **Step 1: Write the failing test**

Create `infra/pkg/gcp/services/deps_test.go`:

```go
package services

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// TestStageOutputs_FieldsAccessible is a compile-time + runtime guard that
// StageOutputs exposes every stage output gcp.NewCompute consumes. If a new
// stage is added (e.g. observability), this test must be extended.
func TestStageOutputs_FieldsAccessible(t *testing.T) {
	s := StageOutputs{
		Net:     types.NetworkOutputs{},
		CICD:    types.CICDOutputs{RepositoryURLs: map[string]pulumi.StringOutput{}},
		DB:      types.DatabaseOutputs{},
		Cache:   types.CacheOutputs{},
		Stream:  types.StreamingOutputs{},
		Secrets: types.SecretsOutputs{},
		Storage: types.StorageOutputs{},
	}
	// Field access is the assertion — if any field is missing or misnamed
	// the package will not compile.
	_ = s.Net
	_ = s.CICD
	_ = s.DB
	_ = s.Cache
	_ = s.Stream
	_ = s.Secrets
	_ = s.Storage
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd infra && go test ./pkg/gcp/services/...`
Expected: FAIL with `package github.com/kaizen-experimentation/infra/pkg/gcp/services is not in std (no Go files in .../pkg/gcp/services)` (the directory does not exist yet).

- [ ] **Step 3: Create the `deps.go` file**

Create `infra/pkg/gcp/services/deps.go`:

```go
// Package services contains one Cloud Run factory per Kaizen per-service
// deploy (M1/M2-Orch/M2-Pipe/M3/M4a/M5/M6/M7 + the preview canary). Each
// factory is invoked from gcp.NewCompute via the registry in registry.go.
// See docs/superpowers/plans/2026-05-16-service-registry-refactor.md for
// the migration rationale (issue #542).
package services

import (
	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
	"github.com/kaizen-experimentation/infra/pkg/types"
)

// StageOutputs bundles every upstream stage output gcp.NewCompute threads
// into the per-service factories. Constructed once per Deploy() call in
// infra/main.go and passed by value (the fields are pulumi.*Output handles —
// cheap to copy, shared backing state).
type StageOutputs struct {
	Net     types.NetworkOutputs
	CICD    types.CICDOutputs
	DB      types.DatabaseOutputs
	Cache   types.CacheOutputs
	Stream  types.StreamingOutputs
	Secrets types.SecretsOutputs
	Storage types.StorageOutputs
}

// CommonInputs is the shared compute.Inputs every Cloud Run factory needs.
// Constructed once in NewCompute (from cfg + StageOutputs.Net) and passed to
// each factory verbatim.
type CommonInputs = compute.Inputs
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd infra && go test ./pkg/gcp/services/...`
Expected: PASS — `ok  github.com/kaizen-experimentation/infra/pkg/gcp/services`

- [ ] **Step 5: Run the full test suite to verify nothing else broke**

Run: `cd infra && go test ./... 2>&1 | grep -E 'FAIL|^ok' | head -20`
Expected: every line either `ok` or the known `infra` panic from AWS storage (`pulumi.ID` type assertion in `TestFullStackDeploy`). No new failures.

- [ ] **Step 6: Commit**

```bash
git add infra/pkg/gcp/services/deps.go infra/pkg/gcp/services/deps_test.go
git commit -m "refactor(gcp): introduce services package with StageOutputs (#542)"
```

---

### Task 2: Extract canary into `services/canary.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:412-426` (current inline block)
- Create: `infra/pkg/gcp/services/canary.go`
- Modify: `infra/pkg/gcp/gcp.go:412-426`

**Why:** Canary is the simplest service — no image lookup (it's `gcr.io/cloudrun/hello`), no DB/cache/secrets, no Buckets/ProjectRoles. Validates the extraction pattern with the smallest surface area.

- [ ] **Step 1: Read the current canary block**

Run: `sed -n '412,426p' infra/pkg/gcp/gcp.go`
Expected: the `canary, err := compute.NewCloudRunService(...)` block ending with `ctx.Export("gcpComputeCanarySaEmail", canary.ServiceAccountEmail)`.

- [ ] **Step 2: Create `services/canary.go` with the factory**

Create `infra/pkg/gcp/services/canary.go`:

```go
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
// service image to be published first.
func NewCanary(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
) (*compute.CloudRunService, error) {
	return compute.NewCloudRunService(ctx, cfg, inputs, "preview-canary",
		&compute.Options{
			Image:         pulumi.String("gcr.io/cloudrun/hello").ToStringOutput(),
			ContainerPort: 8080,
			MinInstances:  0,
		})
}
```

- [ ] **Step 3: Replace the inline canary block in `NewCompute` with a factory call**

Edit `infra/pkg/gcp/gcp.go` — replace the entire `canary, err := compute.NewCloudRunService(ctx, cfg, cloudRunInputs, "preview-canary", &compute.Options{...})` block (lines 412 through 426, including the body and the `ctx.Export(...)` lines) with:

```go
	canary, err := services.NewCanary(ctx, cfg, cloudRunInputs)
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["canary"] = canary.URL
	arns["canary"] = canary.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeCanaryUrl", canary.URL)
	ctx.Export("gcpComputeCanarySaEmail", canary.ServiceAccountEmail)
```

Add the import. In the `import (...)` block at the top of `infra/pkg/gcp/gcp.go`, add (alphabetical position):

```go
	"github.com/kaizen-experimentation/infra/pkg/gcp/services"
```

- [ ] **Step 4: Build to verify no syntax errors**

Run: `cd infra && go build ./...`
Expected: no output (clean build).

- [ ] **Step 5: Run the topology tests that exercise the canary**

Run: `cd infra && go test -count=1 -run 'TestM4bGcpComputeFacade|TestGCPCompute' . ./test/... 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok` or `[no tests to run]`. The pre-existing AWS panic does not run with these `-run` filters.

- [ ] **Step 6: Commit**

```bash
git add infra/pkg/gcp/services/canary.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract preview-canary to services package (#542)"
```

---

### Task 3: Extract M3 Metrics into `services/m3_metrics.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:625-671`
- Create: `infra/pkg/gcp/services/m3_metrics.go`
- Modify: `infra/pkg/gcp/gcp.go:625-680` (block + downstream `endpoints["m3"]` / `ctx.Export(...)` lines)

**Why:** M3 is the simplest Kaizen service — no closure, single Image, standard env/secret pattern, has both `streamOut` and `storageOut` deps so it tests the full StageOutputs surface.

- [ ] **Step 1: Read the current M3 block**

Run: `sed -n '625,680p' infra/pkg/gcp/gcp.go`
Expected: starts with the `m3RepoURL, ok := cicdOut.RepositoryURLs["metrics"]` lookup, contains the comment about Cloud Run v2 single-port limit, ends with the four `endpoints["m3"]` / `arns["m3"]` / `ctx.Export("gcpComputeM3Url"...)` / `ctx.Export("gcpComputeM3SaEmail"...)` lines.

- [ ] **Step 2: Create `services/m3_metrics.go`**

Create `infra/pkg/gcp/services/m3_metrics.go`:

```go
package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM3Metrics wires M3 Metrics (Go service, services/metrics) onto Cloud Run.
// Spark SQL orchestration + Delta Lake writer on GCS; reads metric defs from
// Cloud SQL. Default MinInstances (0) — batch path, no p99 SLA.
//
// NOTE: AWS M3 exposes a second port 50059 for the Prometheus scrape endpoint
// (services/metrics/cmd/main.go:88 — METRICS_PORT default). Cloud Run v2
// supports only one ingress port per container, so 50059 is not reachable
// from outside. Follow-up: merge /metrics onto 50056, sidecar push to Cloud
// Managed Prometheus, or use Cloud Run native metrics integration. Filed as a
// GCP-observability follow-up; not blocking #491.
func NewM3Metrics(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["metrics"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM3Metrics: cicdOut.RepositoryURLs missing \"metrics\" key required for M3 deploy (#491)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m3-metrics",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: 50056,
			MinInstances:  0,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "LOG_LEVEL", Value: pulumi.String("info")},
				{Name: "DATABASE_ENDPOINT", Value: stages.DB.Endpoint},
				{Name: "DATA_BUCKET", Value: stages.Storage.DataBucketName},
				{Name: "DATA_BUCKET_URI", Value: stages.Storage.DataBucketRef},
				// KAFKA_BROKERS (not KAFKA_BOOTSTRAP_BROKERS) is the name the
				// Go service reads at services/metrics/cmd/main.go:57.
				{Name: "KAFKA_BROKERS", Value: stages.Stream.BootstrapBrokers},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
				{EnvName: "KAFKA_SECRET", SecretID: stages.Secrets.KafkaSecretRef, Version: "latest"},
			},
			Buckets:      []pulumi.StringInput{stages.Storage.DataBucketName},
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
```

- [ ] **Step 3: Replace the inline M3 block in `NewCompute` with a factory call**

Edit `infra/pkg/gcp/gcp.go` — replace the block from the M3 comment header down through the last `ctx.Export("gcpComputeM3SaEmail", m3.ServiceAccountEmail)` line with:

```go
	// ─── M3 Metrics (issue #491) ──────────────────────────────────────────
	m3, err := services.NewM3Metrics(ctx, cfg, cloudRunInputs, services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m3"] = m3.URL
	arns["m3"] = m3.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM3Url", m3.URL)
	ctx.Export("gcpComputeM3SaEmail", m3.ServiceAccountEmail)
```

- [ ] **Step 4: Build + run M3 tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestM3|TestM4bGcpComputeFacade' . ./test/... ./pkg/gcp/... 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok` or `[no tests to run]`.

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m3_metrics.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M3 Metrics to services package (#542)"
```

---

### Task 4: Extract M6 UI into `services/m6_ui.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:486-525` (lookup + factory call + endpoint export)
- Create: `infra/pkg/gcp/services/m6_ui.go`
- Modify: `infra/pkg/gcp/gcp.go` (same block range)

**Why:** M6 is the second-simplest — Next.js UI, needs `AuthSecretRef`, no DB/Redis/Bucket; introduces the pattern for services that don't use streaming.

- [ ] **Step 1: Read the current M6 block**

Run: `sed -n '486,525p' infra/pkg/gcp/gcp.go`
Expected: starts with the `uiRepoURL, ok := cicdOut.RepositoryURLs["ui"]` lookup, ends with the `ctx.Export("gcpComputeM6SaEmail"...)` line.

- [ ] **Step 2: Create `services/m6_ui.go`**

Create `infra/pkg/gcp/services/m6_ui.go`:

```go
package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// NewM6UI wires M6 UI (Next.js 14, React 18) onto Cloud Run. Port 3000
// matches the Next.js SSR server. Auth via AUTH_SECRET (NextAuth.js shared
// secret); no DB/cache/Kafka deps because the UI talks to backend services
// via Service Directory — credentials never leave the backend.
func NewM6UI(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["ui"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM6UI: cicdOut.RepositoryURLs missing \"ui\" key required for M6 deploy (#494)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m6-ui",
		&compute.Options{
			Image:         pulumi.Sprintf("%s:latest", repoURL),
			ContainerPort: 3000,
			MinInstances:  0,
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "NODE_ENV", Value: pulumi.String("production")},
				// Service Directory backend resolution (M4b is the one backend
				// guaranteed present pre-#488; others follow as services land).
				{Name: "M4B_POLICY_ENDPOINT", Value: inputs.ServiceDirectoryNamespaceID},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "AUTH_SECRET", SecretID: stages.Secrets.AuthSecretRef, Version: "latest"},
			},
		})
}
```

Note: copy the **exact body** of the original `compute.Options{...}` from gcp.go:498-520 — the snippet above is a structural template. If the original has `M4B_POLICY_ENDPOINT` wired differently or extra env vars, reproduce them faithfully.

- [ ] **Step 3: Replace the inline M6 block in `NewCompute`**

Edit `infra/pkg/gcp/gcp.go` — replace the lookup + `compute.NewCloudRunService(... "m6-ui" ...)` block + endpoint exports with:

```go
	// ─── M6 UI (issue #494) ───────────────────────────────────────────────
	m6, err := services.NewM6UI(ctx, cfg, cloudRunInputs, services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m6"] = m6.URL
	arns["m6"] = m6.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM6Url", m6.URL)
	ctx.Export("gcpComputeM6SaEmail", m6.ServiceAccountEmail)
```

- [ ] **Step 4: Build + run M6 tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestGCPCompute_M6|TestM4bGcpComputeFacade' . 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok`.

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m6_ui.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M6 UI to services package (#542)"
```

---

### Task 5: Extract M4a Analysis into `services/m4a_analysis.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:528-571`
- Create: `infra/pkg/gcp/services/m4a_analysis.go`
- Modify: `infra/pkg/gcp/gcp.go` (same range)

**Why:** M4a is CPU-intensive (uses `CPULimit`/`MemoryLimit`); validates that non-default `compute.Options` fields round-trip cleanly through the factory.

- [ ] **Step 1: Read the current M4a block**

Run: `sed -n '528,571p' infra/pkg/gcp/gcp.go`
Expected: starts with `m4aRepoURL, ok := cicdOut.RepositoryURLs["analysis"]`, contains CPU/Memory limit assignments, ends with `ctx.Export("gcpComputeM4aSaEmail"...)`.

- [ ] **Step 2: Create `services/m4a_analysis.go`**

Create `infra/pkg/gcp/services/m4a_analysis.go` mirroring the M3 template, with the M4a-specific `compute.Options{...}` body copied verbatim from gcp.go:539-568. The factory signature is:

```go
func NewM4aAnalysis(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["analysis"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM4aAnalysis: cicdOut.RepositoryURLs missing \"analysis\" key required for M4a deploy (#492)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m4a-analysis", &compute.Options{
		Image: pulumi.Sprintf("%s:latest", repoURL),
		// ... copy the full Options{} body from gcp.go:539-568 verbatim,
		// substituting cicdOut→stages.CICD, dbOut→stages.DB,
		// secretsOut→stages.Secrets, storageOut→stages.Storage,
		// streamOut→stages.Stream, cacheOut→stages.Cache.
	})
}
```

Imports: same as `m3_metrics.go`. Add `"strings"` only if the original body imports it (M4a does not — confirm with `grep '"strings"' infra/pkg/gcp/gcp.go`).

- [ ] **Step 3: Replace the inline M4a block in `NewCompute`**

Same pattern as Task 3 Step 3 — replace lookup + factory call + endpoint exports with the 4-line `services.NewM4aAnalysis(...)` call + endpoint/arn/export lines.

- [ ] **Step 4: Build + run M4a tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestGCPCompute_M4a|TestM4bGcpComputeFacade' . 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok`.

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m4a_analysis.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M4a Analysis to services package (#542)"
```

---

### Task 6: Extract M1 Assignment into `services/m1_assignment.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:572-611` (lookup + ApplyT image resolution + factory call + endpoint exports)
- Create: `infra/pkg/gcp/services/m1_assignment.go`
- Modify: `infra/pkg/gcp/gcp.go` (same range)

**Why:** M1 carries the p99 < 5ms SLA (`MinInstances: 1`) and an explicit `ApplyT` image resolution. References `m4bOut.Endpoint` — that variable must remain accessible in `NewCompute`'s scope (it is, since M4b stays in the preamble).

- [ ] **Step 1: Read the current M1 block**

Run: `sed -n '572,611p' infra/pkg/gcp/gcp.go`
Expected: starts with `assignmentRepo, ok := cicdOut.RepositoryURLs["assignment"]`, includes the `m1Image := assignmentRepo.ApplyT(...)` line and the `M4B_ADDR` env reference to `m4bOut.Endpoint`.

- [ ] **Step 2: Create `services/m1_assignment.go`**

The M1 factory's signature accepts the `m4bOut.Endpoint` as an extra `pulumi.StringInput` argument since M4b is not in `StageOutputs`:

```go
func NewM1Assignment(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
	m4bEndpoint pulumi.StringInput,
) (*compute.CloudRunService, error) {
	repo, ok := stages.CICD.RepositoryURLs["assignment"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM1Assignment: CICDOutputs.RepositoryURLs is missing the \"assignment\" repo (required for the M1 image)")
	}
	m1Image := repo.ApplyT(func(s string) string {
		return s + ":latest"
	}).(pulumi.StringOutput)

	return compute.NewCloudRunService(ctx, cfg, inputs, "m1-assignment",
		&compute.Options{
			Image:         m1Image,
			ContainerPort: 8080,
			MinInstances:  1, // p99 < 5ms SLA — no cold starts.
			EnvVars: []compute.EnvVar{
				{Name: "ENVIRONMENT", Value: pulumi.String(cfg.Environment)},
				{Name: "RUST_LOG", Value: pulumi.String("info")},
				{Name: "GRPC_ADDR", Value: pulumi.String("0.0.0.0:50051")},
				{Name: "HTTP_ADDR", Value: pulumi.String("0.0.0.0:8080")},
				{Name: "KAFKA_BOOTSTRAP_BROKERS", Value: stages.Stream.BootstrapBrokers},
				{Name: "M4B_ADDR", Value: m4bEndpoint},
			},
			Secrets: []compute.SecretEnv{
				{EnvName: "DATABASE_SECRET", SecretID: stages.Secrets.DatabaseSecretRef, Version: "latest"},
				{EnvName: "REDIS_SECRET", SecretID: stages.Secrets.RedisSecretRef, Version: "latest"},
				{EnvName: "KAFKA_SECRET", SecretID: stages.Secrets.KafkaSecretRef, Version: "latest"},
			},
			ProjectRoles: []string{"roles/cloudsql.client"},
		})
}
```

Imports: `fmt`, `github.com/pulumi/pulumi/sdk/v3/go/pulumi`, `kconfig "github.com/kaizen-experimentation/infra/pkg/config"`, `"github.com/kaizen-experimentation/infra/pkg/gcp/compute"`.

- [ ] **Step 3: Replace the inline M1 block in `NewCompute`**

```go
	// ─── M1 Assignment (issue #488) ───────────────────────────────────────
	m1, err := services.NewM1Assignment(ctx, cfg, cloudRunInputs, services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	}, m4bOut.Endpoint)
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m1"] = m1.URL
	arns["m1"] = m1.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM1Url", m1.URL)
	ctx.Export("gcpComputeM1SaEmail", m1.ServiceAccountEmail)
```

- [ ] **Step 4: Build + run M1 tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestM1|TestM4bGcpComputeFacade' . 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok`.

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m1_assignment.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M1 Assignment to services package (#542)"
```

---

### Task 7: Extract M2-Orch into `services/m2_orchestration.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:428-462` (lookup + factory call + endpoint exports)
- Create: `infra/pkg/gcp/services/m2_orchestration.go`
- Modify: `infra/pkg/gcp/gcp.go` (same range)

- [ ] **Step 1: Read the current M2-Orch block**

Run: `sed -n '428,462p' infra/pkg/gcp/gcp.go`
Expected: starts with `orchestrationRepoURL, ok := cicdOut.RepositoryURLs["orchestration"]`, ends with `ctx.Export("gcpComputeM2OrchSaEmail"...)`.

- [ ] **Step 2: Create `services/m2_orchestration.go`**

Mirror the M3 template. Factory signature:

```go
func NewM2Orchestration(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
) (*compute.CloudRunService, error) {
	repoURL, ok := stages.CICD.RepositoryURLs["orchestration"]
	if !ok {
		return nil, fmt.Errorf(
			"gcp.services.NewM2Orchestration: cicdOut.RepositoryURLs missing \"orchestration\" key required for M2-Orch deploy (#490)")
	}
	return compute.NewCloudRunService(ctx, cfg, inputs, "m2-orchestration", &compute.Options{
		// ... copy compute.Options{} body from gcp.go:449-460 verbatim,
		// substituting *Out vars with stages.* equivalents.
	})
}
```

- [ ] **Step 3: Replace the inline M2-Orch block in `NewCompute`**

```go
	// ─── M2 Orchestration (issue #490) ────────────────────────────────────
	m2Orch, err := services.NewM2Orchestration(ctx, cfg, cloudRunInputs, services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m2-orch"] = m2Orch.URL
	arns["m2-orch"] = m2Orch.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM2OrchUrl", m2Orch.URL)
	ctx.Export("gcpComputeM2OrchSaEmail", m2Orch.ServiceAccountEmail)
```

- [ ] **Step 4: Build + run M2-Orch tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestM2Orch|TestM4bGcpComputeFacade' . ./test/... 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok`.

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m2_orchestration.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M2 Orchestration to services package (#542)"
```

---

### Task 8: Extract M2-Pipeline into `services/m2_pipeline.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:725-797` (call site at 725-737 + `newM2PipelineService` def at 755-797 + the two constants `m2PipelinePort` / `m2PipelineMaxInstances`)
- Create: `infra/pkg/gcp/services/m2_pipeline.go`
- Modify: `infra/pkg/gcp/gcp.go` — remove call site, constants, and `newM2PipelineService` body

**Why:** M2-Pipeline already lives in its own helper function (`newM2PipelineService`) — this task moves the helper verbatim and renames it.

- [ ] **Step 1: Read all M2-Pipeline source**

Run: `grep -nE 'm2PipelinePort|m2PipelineMaxInstances|newM2PipelineService|m2pipe' infra/pkg/gcp/gcp.go`
Expected: 2 const decls (`m2PipelinePort = 50052`, `m2PipelineMaxInstances = 100`), the `newM2PipelineService` func def, and the 4-line call site inside `NewCompute`.

- [ ] **Step 2: Create `services/m2_pipeline.go` with the verbatim function body**

Copy lines 738-797 (constants + `newM2PipelineService` body) into `infra/pkg/gcp/services/m2_pipeline.go`, with these changes:

1. Rename `m2PipelinePort` → `M2PipelinePort` (export it — only used inside the factory now, but exported for symmetry).
2. Rename `m2PipelineMaxInstances` → `M2PipelineMaxInstances`.
3. Rename `newM2PipelineService` → `NewM2Pipeline`.
4. Change the signature from `(ctx, cfg, inputs, cicdOut, streamOut, secretsOut)` to `(ctx, cfg, inputs, stages StageOutputs)`.
5. Substitute `cicdOut.RepositoryURLs` → `stages.CICD.RepositoryURLs`, `streamOut.BootstrapBrokers` → `stages.Stream.BootstrapBrokers`, etc.

The result is a single Go file with constants + factory func. Imports: `fmt`, `pulumi`, `kconfig`, `compute`.

- [ ] **Step 3: Replace the call site + delete the now-orphaned helper from `gcp.go`**

In `infra/pkg/gcp/gcp.go`:

1. Replace the M2-Pipeline call site (the `m2pipe, err := newM2PipelineService(...)` block + endpoint exports) with:

```go
	// ─── M2 Pipeline (issue #489) ─────────────────────────────────────────
	m2pipe, err := services.NewM2Pipeline(ctx, cfg, cloudRunInputs, services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m2-pipeline"] = m2pipe.URL
	arns["m2-pipeline"] = m2pipe.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM2PipelineUrl", m2pipe.URL)
	ctx.Export("gcpComputeM2PipelineSaEmail", m2pipe.ServiceAccountEmail)
```

2. Delete the `m2PipelinePort` + `m2PipelineMaxInstances` const decls and the entire `newM2PipelineService(...)` func body from `gcp.go`.

- [ ] **Step 4: Build + run M2-Pipeline tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestM2Pipeline|TestM4bGcpComputeFacade' . ./test/... 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok`.

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m2_pipeline.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M2 Pipeline to services package (#542)"
```

---

### Task 9: Extract M7 Flags into `services/m7_flags.go`

**Files:**
- Read: `infra/pkg/gcp/gcp.go:685-723`
- Create: `infra/pkg/gcp/services/m7_flags.go`
- Modify: `infra/pkg/gcp/gcp.go` (same range)

**Why:** M7 also has the p99 < 5ms SLA (`MinInstances: 1`) and adds `REDIS_ENDPOINT` env from `cacheOut.Endpoint`.

- [ ] **Step 1: Read the current M7 block**

Run: `sed -n '685,723p' infra/pkg/gcp/gcp.go`
Expected: starts with `flagsRepo, ok := cicdOut.RepositoryURLs["flags"]`, ends with `ctx.Export("gcpComputeM7SaEmail"...)`.

- [ ] **Step 2: Create `services/m7_flags.go`**

Mirror the M3 template. Body of `compute.Options{...}` copied verbatim from gcp.go:696-714 with `*Out` → `stages.*` substitution. `MaxInstances: 10` and `MinInstances: 1` preserved.

- [ ] **Step 3: Replace the inline M7 block in `NewCompute`**

```go
	// ─── M7 Flags (issue #495) ────────────────────────────────────────────
	m7, err := services.NewM7Flags(ctx, cfg, cloudRunInputs, services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m7"] = m7.URL
	arns["m7"] = m7.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM7Url", m7.URL)
	ctx.Export("gcpComputeM7SaEmail", m7.ServiceAccountEmail)
```

- [ ] **Step 4: Build + run M7 tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestM7|TestM4bGcpComputeFacade' . ./test/... 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok`.

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m7_flags.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M7 Flags to services package (#542)"
```

---

### Task 10: Extract M5 Management into `services/m5_management.go` (closure case)

**Files:**
- Read: `infra/pkg/gcp/gcp.go:802-873` (constant + `newM5ManagementService` def + closure)
- Create: `infra/pkg/gcp/services/m5_management.go`
- Modify: `infra/pkg/gcp/gcp.go` (call site + delete helper + delete constant)

**Why:** M5 is the only service that uses the `secretIDForRef` closure (PR #540's intentional pattern). The closure stays inside the factory verbatim — DO NOT unify with M3/M7's bare-Ref pattern in this PR. Unification is captured as a separate follow-up (see Task 14).

- [ ] **Step 1: Read all M5 source**

Run: `grep -nE 'm5ManagementPort|newM5ManagementService|secretIDForRef' infra/pkg/gcp/gcp.go`
Expected: 1 const decl (`m5ManagementPort = 50055`), the `newM5ManagementService` func def with the inline closure, and the call site inside `NewCompute`.

- [ ] **Step 2: Create `services/m5_management.go` with the verbatim body**

Copy the const + `newM5ManagementService` body into `infra/pkg/gcp/services/m5_management.go` with these changes:

1. Rename `m5ManagementPort` → `M5ManagementPort`.
2. Rename `newM5ManagementService` → `NewM5Management`.
3. Change signature to `(ctx, cfg, inputs, stages StageOutputs)`.
4. Substitute `*Out` → `stages.*`.
5. Keep the `secretIDForRef` closure verbatim — it lives inside `NewM5Management` body.
6. Import `"github.com/kaizen-experimentation/infra/pkg/gcp/secrets"` (the closure calls `secrets.SecretID(cfg, component)`).

- [ ] **Step 3: Replace the M5 call site + delete the orphaned helper**

In `infra/pkg/gcp/gcp.go`:

1. Replace the M5 call site with:

```go
	// ─── M5 Management (issue #493) ───────────────────────────────────────
	m5, err := services.NewM5Management(ctx, cfg, cloudRunInputs, services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	})
	if err != nil {
		return types.ComputeOutputs{}, err
	}
	endpoints["m5"] = m5.URL
	arns["m5"] = m5.Service.ID().ToStringOutput()
	ctx.Export("gcpComputeM5Url", m5.URL)
	ctx.Export("gcpComputeM5SaEmail", m5.ServiceAccountEmail)
```

2. Delete the `m5ManagementPort` const decl and the entire `newM5ManagementService(...)` func body from `gcp.go`.

3. Check whether `"github.com/kaizen-experimentation/infra/pkg/gcp/secrets"` is still used elsewhere in `gcp.go` (it is — `NewSecrets` calls into the package). Leave the import.

- [ ] **Step 4: Build + run M5 tests**

Run: `cd infra && go build ./... && go test -count=1 -run 'TestM5|TestM4bGcpComputeFacade' . 2>&1 | grep -E 'FAIL|^ok' | head -10`
Expected: every line `ok` (M5's bare-name assertion still passes — the closure produces `"kaizen-dev-database"` etc. from `secrets.SecretID`).

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/m5_management.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): extract M5 Management to services package (#542)"
```

---

### Task 11: Build the registry + slim `NewCompute`

**Files:**
- Create: `infra/pkg/gcp/services/registry.go`
- Modify: `infra/pkg/gcp/gcp.go` — `NewCompute` body becomes a registry walk + M4b preamble

**Why:** Replaces 9 nearly-identical call sites in `NewCompute` with a single loop. After this task, adding a future service is ~3 lines (factory file + registry entry).

- [ ] **Step 1: Create `services/registry.go`**

```go
package services

import (
	"fmt"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	kconfig "github.com/kaizen-experimentation/infra/pkg/config"
	"github.com/kaizen-experimentation/infra/pkg/gcp/compute"
)

// RegistryEntry binds a service's short key (e.g. "m3") to its factory.
// The factory signature is uniform — services that need extra args (today
// only M1, which reads M4b's endpoint) accept them through closure capture
// in the registry slice below.
type RegistryEntry struct {
	Key     string
	Factory func(ctx *pulumi.Context, cfg *kconfig.Config, inputs *compute.Inputs, stages StageOutputs) (*compute.CloudRunService, error)
}

// Walk invokes every factory in declaration order, returning a map of service
// key → recorded service. Callers (gcp.NewCompute) compose .URL into
// types.ComputeOutputs.ServiceEndpoints and .Service.ID() into ServiceArns.
//
// Order matters only for the Cloud Run dependency graph (Pulumi handles
// dependencies via output edges, not call order) — but stable iteration
// keeps ctx.Export key emission deterministic.
func Walk(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	inputs *compute.Inputs,
	stages StageOutputs,
	registry []RegistryEntry,
) (map[string]*compute.CloudRunService, error) {
	out := make(map[string]*compute.CloudRunService, len(registry))
	for _, entry := range registry {
		svc, err := entry.Factory(ctx, cfg, inputs, stages)
		if err != nil {
			return nil, fmt.Errorf("services.Walk: factory %q failed: %w", entry.Key, err)
		}
		out[entry.Key] = svc
	}
	return out, nil
}
```

- [ ] **Step 2: Modify `gcp.NewCompute` to use the registry**

In `infra/pkg/gcp/gcp.go`, the `NewCompute` body becomes:

```go
func NewCompute(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	netOut types.NetworkOutputs,
	cicdOut types.CICDOutputs,
	dbOut types.DatabaseOutputs,
	streamOut types.StreamingOutputs,
	secretsOut types.SecretsOutputs,
	storageOut types.StorageOutputs,
	cacheOut types.CacheOutputs,
) (types.ComputeOutputs, error) {
	region := cfg.GCPRegion
	if region == "" {
		region = "us-central1"
	}

	// M4b stays here — stateful GCE/MIG slice, not a Cloud Run service.
	privateSubnetSelfLink := netOut.PrivateSubnetIds.ApplyT(func(ids []string) string {
		if len(ids) == 0 {
			return ""
		}
		return ids[0]
	}).(pulumi.StringOutput)
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

	stages := services.StageOutputs{
		Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
		Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
	}

	// Registry order is the historical order each per-service issue landed.
	registry := []services.RegistryEntry{
		{Key: "canary", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, _ services.StageOutputs) (*compute.CloudRunService, error) {
			return services.NewCanary(ctx, cfg, in)
		}},
		{Key: "m2-orch", Factory: services.NewM2Orchestration},
		{Key: "m6", Factory: services.NewM6UI},
		{Key: "m4a", Factory: services.NewM4aAnalysis},
		// M1 closes over m4bOut.Endpoint — captured in the closure.
		{Key: "m1", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, s services.StageOutputs) (*compute.CloudRunService, error) {
			return services.NewM1Assignment(ctx, cfg, in, s, m4bOut.Endpoint)
		}},
		{Key: "m3", Factory: services.NewM3Metrics},
		{Key: "m7", Factory: services.NewM7Flags},
		{Key: "m2-pipeline", Factory: services.NewM2Pipeline},
		{Key: "m5", Factory: services.NewM5Management},
	}

	svcs, err := services.Walk(ctx, cfg, cloudRunInputs, stages, registry)
	if err != nil {
		return types.ComputeOutputs{}, err
	}

	endpoints := make(map[string]pulumi.StringOutput, len(svcs))
	arns := make(map[string]pulumi.StringOutput, len(svcs))
	for key, svc := range svcs {
		endpoints[key] = svc.URL
		arns[key] = svc.Service.ID().ToStringOutput()
		ctx.Export("gcpComputeUrl_"+key, svc.URL)
		ctx.Export("gcpComputeSaEmail_"+key, svc.ServiceAccountEmail)
	}

	return types.ComputeOutputs{
		M4bInstanceId:    m4bOut.InstanceName,
		M4bEndpoint:      m4bOut.Endpoint,
		M4bAsgName:       m4bOut.MigName,
		ServiceEndpoints: endpoints,
		ServiceArns:      arns,
	}, nil
}
```

This deletes ~400 lines from `gcp.go` (the old inline service blocks AND the per-service `ctx.Export` calls). The export key naming changes from per-service (`gcpComputeM1Url`) to uniform (`gcpComputeUrl_m1`) — this is a breaking change for any downstream consumer of pulumi stack outputs. If staging/prod stack files reference the old keys, audit before merge.

- [ ] **Step 3: Audit stack-output consumers**

Run: `git grep -nE 'gcpComputeM[0-9]Url|gcpComputeM[0-9].*SaEmail|gcpComputeCanary|gcpComputeM2OrchUrl|gcpComputeM2PipelineUrl' --` from repo root.
Expected: only matches inside `infra/` (the `ctx.Export` calls we just deleted and any test assertions). If matches appear under `services/`, `sdks/`, or stack YAMLs, decide: keep both export schemes during a deprecation window, or update consumers in this same task.

- [ ] **Step 4: Build + run the full test sweep**

Run: `cd infra && go build ./... && go test -count=1 ./... 2>&1 | grep -E 'FAIL|^ok' | head -30`
Expected: every line `ok` or the pre-existing AWS panic. Many existing test fixtures (m1_topology, m6_compute, m7_flags, m2_pipeline, m2orch, m5_topology, fullstack, pkg/gcp/gcp_test) still have all the cross-pollinated repo keys — they should pass unchanged because `NewCompute` still requires every key (every factory still fails on missing repo).

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/services/registry.go infra/pkg/gcp/gcp.go
git commit -m "refactor(gcp): replace inline service blocks with registry walk (#542)"
```

---

### Task 12: Introduce `StageOutputs` at the call site

**Files:**
- Modify: `infra/pkg/gcp/gcp.go` — `NewCompute` signature collapses to 4 params
- Modify: `infra/main.go` — single call site
- Modify: every test file calling `gcp.NewCompute` — collapse to 4-arg form

**Why:** The 9-arg signature is the user-visible API smell. Collapsing to `(ctx, cfg, stages)` is the ergonomic payoff of the refactor.

- [ ] **Step 1: Change `gcp.NewCompute` signature**

In `infra/pkg/gcp/gcp.go`, change `NewCompute` to:

```go
func NewCompute(
	ctx *pulumi.Context,
	cfg *kconfig.Config,
	stages services.StageOutputs,
) (types.ComputeOutputs, error) {
```

Inside the body, replace `netOut` → `stages.Net`, `cicdOut` → `stages.CICD`, `dbOut` → `stages.DB`, `streamOut` → `stages.Stream`, `secretsOut` → `stages.Secrets`, `storageOut` → `stages.Storage`, `cacheOut` → `stages.Cache`. Delete the redundant `stages := services.StageOutputs{...}` literal (the parameter IS already the stages struct).

- [ ] **Step 2: Update `main.go` call site**

In `infra/main.go`, find the line `gcpComputeOut, err := gcp.NewCompute(ctx, cfg, netOut, cicdOut, dbOut, gcpStreamOut, gcpSecretsOut, storageOut, cacheOut)` and replace with:

```go
		gcpComputeOut, err := gcp.NewCompute(ctx, cfg, services.StageOutputs{
			Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
			Stream: gcpStreamOut, Secrets: gcpSecretsOut, Storage: storageOut,
		})
```

Add import `"github.com/kaizen-experimentation/infra/pkg/gcp/services"` to `main.go`.

- [ ] **Step 3: Update every existing test that calls `gcp.NewCompute`**

Find them:
```bash
grep -rnE 'gcp(facade)?\.NewCompute|gcpfacade\.NewCompute' infra/ | grep -v 'pkg/gcp/services/'
```

Expected matches: `infra/m1_topology_test.go`, `infra/gcp_compute_m6_test.go`, `infra/gcp_m5_topology_test.go`, `infra/fullstack_gcp_test.go`, `infra/test/gcp_m2orch_topology_test.go`, `infra/test/gcp_m7_flags_topology_test.go`, `infra/test/gcp_m2_pipeline_topology_test.go`, `infra/pkg/gcp/gcp_test.go`.

In each, replace the 9-arg call with the 3-arg form:

```go
out, err := gcp.NewCompute(ctx, cfg, services.StageOutputs{
	Net: netOut, CICD: cicdOut, DB: dbOut, Cache: cacheOut,
	Stream: streamOut, Secrets: secretsOut, Storage: storageOut,
})
```

Add import `"github.com/kaizen-experimentation/infra/pkg/gcp/services"` to each file.

(Most of these test files will be deleted in Task 13. This task keeps them compilable so the test suite passes between Task 11 and Task 13.)

- [ ] **Step 4: Build + run full test sweep**

Run: `cd infra && go build ./... && go test -count=1 ./... 2>&1 | grep -E 'FAIL|^ok' | head -30`
Expected: every line `ok` (or known AWS panic).

- [ ] **Step 5: Commit**

```bash
git add infra/pkg/gcp/gcp.go infra/main.go infra/pkg/gcp/gcp_test.go infra/m1_topology_test.go infra/gcp_compute_m6_test.go infra/gcp_m5_topology_test.go infra/fullstack_gcp_test.go infra/test/
git commit -m "refactor(gcp): collapse NewCompute to (ctx, cfg, StageOutputs) (#542)"
```

---

### Task 13: Replace cross-pollinated fixtures with per-service scoped tests

**Files:**
- Create: `infra/pkg/gcp/services/m1_assignment_test.go`
- Create: `infra/pkg/gcp/services/m2_orchestration_test.go`
- Create: `infra/pkg/gcp/services/m2_pipeline_test.go`
- Create: `infra/pkg/gcp/services/m3_metrics_test.go`
- Create: `infra/pkg/gcp/services/m4a_analysis_test.go`
- Create: `infra/pkg/gcp/services/m5_management_test.go`
- Create: `infra/pkg/gcp/services/m6_ui_test.go`
- Create: `infra/pkg/gcp/services/m7_flags_test.go`
- Create: `infra/pkg/gcp/services/registry_test.go`
- Delete: `infra/m1_topology_test.go`, `infra/gcp_compute_m6_test.go`, `infra/gcp_m5_topology_test.go`, `infra/pkg/gcp/gcp_test.go`, `infra/test/gcp_m2orch_topology_test.go`, `infra/test/gcp_m7_flags_topology_test.go`, `infra/test/gcp_m2_pipeline_topology_test.go`
- Modify: `infra/fullstack_gcp_test.go` — delete `TestGCPCompute_M4a*` (3 funcs) + `gcpComputeInputs` helper

**Why:** The cross-pollinated `RepositoryURLs` maps go away once each factory is tested in isolation. This is the visible payoff users see.

- [ ] **Step 1: Create per-service scoped tests (M3 template)**

Create `infra/pkg/gcp/services/m3_metrics_test.go`:

```go
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

// TestM3Metrics_Wiring asserts M3's Cloud Run shape: name, port, data bucket
// env vars, KAFKA_BROKERS env var, DB + Kafka secret mounts.
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
```

- [ ] **Step 2: Create the other 7 per-service test files**

For each service (M1/M2-Orch/M2-Pipe/M4a/M5/M6/M7), create `pkg/gcp/services/<name>_test.go` following the M3 template:

1. A `<service>StageOutputs()` helper returning the minimal StageOutputs that service reads.
2. A `Test<Service>_Wiring` test calling `New<Service>(ctx, scopedCfg(), scopedInputs(), <service>StageOutputs())` and asserting on the Cloud Run service shape (name, port, image substring).
3. A `Test<Service>_MissingRepoFails` test for the missing-repo error path.
4. Service-specific assertions: M1/M7 assert `MinInstances: 1`; M2-Pipe asserts `MaxInstances: 100`; M6 asserts AUTH_SECRET; M5 asserts the `secretIDForRef` produces bare names ("kaizen-dev-database" etc.).

Reuse the `scopedMocks`/`scopedInputs`/`scopedCfg` helpers from `m3_metrics_test.go` (same package — no need to redeclare).

- [ ] **Step 3: Create the aggregator test**

Create `infra/pkg/gcp/services/registry_test.go`:

```go
package services

import (
	"testing"

	"github.com/pulumi/pulumi/sdk/v3/go/pulumi"

	"github.com/kaizen-experimentation/infra/pkg/types"
)

// fullStageOutputs is the superset every factory consumes — used only by
// the registry-walk aggregator test below.
func fullStageOutputs() StageOutputs {
	repos := map[string]pulumi.StringOutput{}
	for _, k := range []string{"assignment", "orchestration", "ui", "analysis", "metrics", "flags", "pipeline", "management"} {
		repos[k] = pulumi.String("us-docker.pkg.dev/kaizen-experimentation-dev/kaizen/" + k).ToStringOutput()
	}
	return StageOutputs{
		CICD: types.CICDOutputs{RepositoryURLs: repos},
		DB:   types.DatabaseOutputs{Endpoint: pulumi.String("10.99.0.3:5432").ToStringOutput()},
		Cache: types.CacheOutputs{Endpoint: pulumi.String("redis://10.99.1.1:6379").ToStringOutput()},
		Stream: types.StreamingOutputs{
			BootstrapBrokers:  pulumi.String("seed-0.redpanda.cloud:9092").ToStringOutput(),
			SchemaRegistryUrl: pulumi.String("https://schema-registry.redpanda.cloud:30081").ToStringOutput(),
		},
		Secrets: types.SecretsOutputs{
			DatabaseSecretRef: pulumi.String("kaizen-dev-database").ToStringOutput(),
			KafkaSecretRef:    pulumi.String("kaizen-dev-kafka").ToStringOutput(),
			RedisSecretRef:    pulumi.String("kaizen-dev-redis").ToStringOutput(),
			AuthSecretRef:     pulumi.String("kaizen-dev-auth").ToStringOutput(),
		},
		Storage: types.StorageOutputs{
			DataBucketName: pulumi.String("kaizen-dev-data").ToStringOutput(),
			DataBucketRef:  pulumi.String("gs://kaizen-dev-data").ToStringOutput(),
		},
	}
}

// TestRegistry_WalkProducesEveryService asserts the walker produces exactly
// the 9 Cloud Run services every per-service issue is responsible for. If a
// future service lands, this count is the canonical place to bump.
func TestRegistry_WalkProducesEveryService(t *testing.T) {
	mocks := &scopedMocks{}
	err := pulumi.RunErr(func(ctx *pulumi.Context) error {
		// Reproduce the registry from gcp.NewCompute — including the M1
		// closure capture of m4bEndpoint (use a constant for testing).
		m4bEndpoint := pulumi.String("10.0.16.42:50054").ToStringOutput()
		registry := []RegistryEntry{
			{Key: "canary", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, _ StageOutputs) (*compute.CloudRunService, error) {
				return NewCanary(ctx, cfg, in)
			}},
			{Key: "m2-orch", Factory: NewM2Orchestration},
			{Key: "m6", Factory: NewM6UI},
			{Key: "m4a", Factory: NewM4aAnalysis},
			{Key: "m1", Factory: func(ctx *pulumi.Context, cfg *kconfig.Config, in *compute.Inputs, s StageOutputs) (*compute.CloudRunService, error) {
				return NewM1Assignment(ctx, cfg, in, s, m4bEndpoint)
			}},
			{Key: "m3", Factory: NewM3Metrics},
			{Key: "m7", Factory: NewM7Flags},
			{Key: "m2-pipeline", Factory: NewM2Pipeline},
			{Key: "m5", Factory: NewM5Management},
		}
		out, err := Walk(ctx, scopedCfg(), scopedInputs(), fullStageOutputs(), registry)
		if err != nil {
			return err
		}
		if got := len(out); got != 9 {
			t.Errorf("Walk produced %d services, want 9", got)
		}
		for _, key := range []string{"canary", "m1", "m2-orch", "m2-pipeline", "m3", "m4a", "m5", "m6", "m7"} {
			if _, ok := out[key]; !ok {
				t.Errorf("Walk missing service %q", key)
			}
		}
		return nil
	}, pulumi.WithMocks("kaizen", "dev", mocks))
	if err != nil {
		t.Fatalf("registry Walk failed: %v", err)
	}

	if got := len(mocks.byType("gcp:cloudrunv2/service:Service")); got != 9 {
		t.Errorf("expected 9 Cloud Run services registered, got %d", got)
	}
	if got := len(mocks.byType("gcp:servicedirectory/service:Service")); got != 9 {
		t.Errorf("expected 9 Service Directory services registered, got %d", got)
	}
}
```

Add the import for `compute` and `kconfig` at the top of the file.

- [ ] **Step 4: Delete superseded fixture files**

```bash
rm infra/m1_topology_test.go infra/gcp_compute_m6_test.go infra/gcp_m5_topology_test.go
rm infra/pkg/gcp/gcp_test.go
rm infra/test/gcp_m2orch_topology_test.go infra/test/gcp_m7_flags_topology_test.go infra/test/gcp_m2_pipeline_topology_test.go
```

- [ ] **Step 5: Trim `infra/fullstack_gcp_test.go`**

In `infra/fullstack_gcp_test.go`, delete `TestGCPCompute_M4aInServiceEndpoints`, `TestGCPCompute_M4aHealthProbeAndResources`, `TestGCPCompute_M4aDataBucketAndSecretIAM`, and the `gcpComputeInputs` helper. Keep the file's `TestFullStackDeploy_GCP`, `TestFullStackResourceCounts_GCP`, and `TestFullStackDeploy_GCP_RejectsMissingProject` tests — those are end-to-end Deploy() tests, not per-service.

- [ ] **Step 6: Update `m4b_topology_test.go` SD-count assertion**

In `infra/m4b_topology_test.go`, replace the hardcoded `got != 10` SD-services assertion (the one comparing `M4b + canary + ... + M5`) with:

```go
	// 9 per-service Cloud Run SD entries + 1 M4b stateful entry = 10.
	// The 9-count is canonicalized in pkg/gcp/services/registry_test.go.
	if got := len(mocks.byType("gcp:servicedirectory/service:Service")); got != 10 {
		t.Errorf("expected 10 SD Services from Deploy(gcp) (M4b + 9 Cloud Run services), got %d", got)
	}
```

(Keeping the count assertion in `m4b_topology_test.go` makes sense — it's the highest-level Deploy() proof. The per-service count lives in `registry_test.go`.)

- [ ] **Step 7: Build + run full sweep**

Run: `cd infra && go build ./... && go test -count=1 ./... 2>&1 | grep -E 'FAIL|^ok' | head -30`
Expected: every line `ok` or known AWS panic. The new `pkg/gcp/services` package should report `ok` with the 9+ tests (8 per-service + 1 registry).

- [ ] **Step 8: Commit**

```bash
git add infra/pkg/gcp/services/ infra/fullstack_gcp_test.go infra/m4b_topology_test.go
git rm infra/m1_topology_test.go infra/gcp_compute_m6_test.go infra/gcp_m5_topology_test.go infra/pkg/gcp/gcp_test.go infra/test/gcp_m2orch_topology_test.go infra/test/gcp_m7_flags_topology_test.go infra/test/gcp_m2_pipeline_topology_test.go
git commit -m "test(gcp): replace cross-pollinated fixtures with per-service scoped tests (#542)"
```

---

### Task 14: Final cleanup, docs, and PR

**Files:**
- Modify: `CLAUDE.md` (if needed — likely just the architecture description)
- Modify: `infra/pkg/gcp/gcp.go` (audit imports — `secrets` package may no longer be needed if `NewSecrets` doesn't use it; `compute` import stays for `M4bArgs`)
- Modify: `docs/adrs/` (optional — file a brief decision note if the pattern is novel enough to warrant)

- [ ] **Step 1: Audit imports in `gcp.go`**

Run: `cd infra && goimports -l pkg/gcp/gcp.go`
Expected: nothing emitted (file is correctly imported). If it suggests removing unused imports, accept the suggestion.

- [ ] **Step 2: Verify the public API of `pkg/gcp`**

Run: `cd infra && go doc -all ./pkg/gcp | head -40`
Expected: `NewCompute` shows the new 3-param signature; `NewNetwork`, `NewStorage`, `NewCache`, `NewDatabase`, `NewSecrets`, `NewCICD` are unchanged.

- [ ] **Step 3: Update `CLAUDE.md` (if needed)**

Run: `grep -n 'NewCompute\|gcp.NewCompute\|service.*registry' CLAUDE.md`
Expected: probably no matches — `CLAUDE.md` describes module architecture, not factory internals. If there's a passage that needs updating (e.g., a count of Cloud Run services in the agent guide), update it; otherwise skip.

- [ ] **Step 4: Run the full test sweep one last time**

Run: `cd infra && go test -count=1 ./... 2>&1 | grep -E 'FAIL|^ok' | head -30`
Expected: every line `ok` or the known AWS panic.

- [ ] **Step 5: Verify line count reduction**

Run: `wc -l infra/pkg/gcp/gcp.go && wc -l infra/pkg/gcp/services/*.go`
Expected: `gcp.go` < 500 lines (down from 873); per-service files each ~50 lines.

- [ ] **Step 6: Final commit + PR**

```bash
git add CLAUDE.md   # only if Step 3 made changes
git commit --allow-empty -m "refactor(gcp): finalize service-registry refactor (Closes #542)"
git push -u origin agent-4/refactor/542-service-registry
gh pr create --title "Refactor gcp.NewCompute to service-registry pattern (Closes #542)" --body "$(cat <<'EOF'
## Summary

- Refactors 873-line monolithic `gcp.NewCompute` into a registry-walked composition of 9 per-service factories under `pkg/gcp/services/`.
- Collapses 9-arg `NewCompute` signature to `(ctx, cfg, StageOutputs)`.
- Replaces cross-pollinated `RepositoryURLs` fixtures with scoped per-service tests — each test declares only the inputs its service consumes.
- No behavior change. All existing topology + Deploy tests continue to pass.

Closes #542.

## Test plan

- [ ] `cd infra && go test ./...` is green (excl. pre-existing AWS storage `pulumi.ID` panic)
- [ ] `pulumi preview --stack gcp-dev` against the new branch produces zero resource diff from main (verifies refactor is behavior-preserving)
- [ ] `just test-infra` passes
- [ ] Reviewer spot-checks per-service factory code matches the source extracted from the old `NewCompute` (cross-reference with `git log -p origin/main -- infra/pkg/gcp/gcp.go`)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-review

After saving this plan, the author (Claude) ran the three required self-review checks:

**1. Spec coverage** — every acceptance criterion in issue #542 maps to a task:
   - "Each service file exports a single `NewMXX(...)` constructor" → Tasks 2–10.
   - "`gcp.NewCompute` signature reduced to `(ctx, cfg, StageOutputs)`" → Task 12.
   - "`gcp.NewCompute` body is < 50 lines" → Task 11 + 12 (final body is preamble + M4b + registry + result assembly ≈ 50 lines).
   - "Per-service tests live next to factories; scoped fixtures" → Task 13.
   - "Aggregator test verifies the full walk produces 9 Cloud Run services + 10 SD entries" → Task 13 (registry_test.go).
   - "M4b topology test SD-count assertion updates" → Task 13 Step 6.
   - "All existing topology tests either move or are deleted" → Task 13 Step 4.
   - "`infra/main.go` call site updated" → Task 12 Step 2.
   - "No behavioural change" → execution discipline rule 2 + every task's test step.
   - **Not addressed inline**: "Follow-up scope captured: AWS-side `pkg/aws/compute/services.go` does NOT need the same treatment" → this is in the issue's "Out of scope" section, requires no code, no task needed.

**2. Placeholder scan** — searched for "TBD", "fill in", "Similar to Task N", "implement later". Two judgment-call deviations from strict "complete code in every step":
   - Tasks 4, 5, 7, 9 say "copy compute.Options body from gcp.go:<lines> verbatim, substituting *Out → stages.* equivalents" rather than re-pasting nearly-identical 30-line bodies four times. Justified at the top of the plan in the "Execution discipline" section. An agent executing this plan reads the source from a stable file at fixed line ranges — no risk of guessing.
   - Task 13 Step 2 says "Create the other 7 per-service test files… following the M3 template". Same justification: the template is fully concrete in Step 1; subsequent files repeat the pattern with service-specific values. An agent skilled enough to write the M3 test from the template can write the others.

**3. Type consistency** — checked names across tasks: `StageOutputs` (Task 1) is used by every factory signature (Tasks 3–10) and the registry (Task 11) and the call site (Task 12) and the tests (Task 13). `RegistryEntry`, `Walk` names match. `NewM3Metrics`, `NewM6UI`, `NewM4aAnalysis`, `NewM1Assignment`, `NewM2Orchestration`, `NewM2Pipeline`, `NewM7Flags`, `NewM5Management`, `NewCanary` are consistent. `M2PipelinePort` and `M5ManagementPort` exported correctly.

No issues found.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-16-service-registry-refactor.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration with two-stage review

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
