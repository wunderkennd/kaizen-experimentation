# Artifact Registry Credentials — Rotation Playbook

**Status:** Active (Phase 1 onwards)
**Owner:** Platform / SRE rotation
**Last reviewed:** 2026-05-07
**Scope:** GCP Artifact Registry repos owned by Pulumi `pkg/gcp/cicd` and pushed to by GitHub Actions.

This runbook covers the credentials that move container images from CI into
Artifact Registry. It does **not** cover Cloud Run runtime SAs (those have
their own runbook once Phase 1 compute lands).

> Companion: `pkg/gcp/cicd/artifact_registry.go` for repo provisioning;
> `.github/workflows/ci.yml` (job: `docker`) for the push pipeline.

---

## Identity model

We use **GitHub OIDC → GCP Workload Identity Federation (WIF)** — no
long-lived service-account keys are stored in GitHub secrets. The chain:

```
GitHub Actions runner
  ↓ (signs an OIDC JWT for this repo + branch)
GCP Workload Identity Pool ("github-actions")
  ↓ (attribute mapping: repository, ref)
Workload Identity Provider ("github-actions-oidc")
  ↓ (impersonation grant via roles/iam.workloadIdentityUser)
Service account: kaizen-ci-push@<project>.iam.gserviceaccount.com
  ↓ (per-repo IAM binding from pkg/gcp/cicd)
roles/artifactregistry.writer  on each Kaizen AR repo
```

The push SA holds **only** writer role per repository — not project-wide. The
binding is created by `pkg/gcp/cicd.NewArtifactRegistryRepositories` when
`Config.PushPrincipal` is set. Read access for Cloud Run runtime SAs is a
separate, also-per-repo binding.

### Why no service-account JSON keys

Static keys don't appear in any rotation surface here. The only place they
*could* exist is the legacy bootstrap account, and that account is
suspended after first apply (see "Bootstrap" below). If you find a JSON key
in a CI secret, treat it as an incident and rotate immediately.

---

## Routine rotation cadence

| Surface | Cadence | How |
| --- | --- | --- |
| Workload Identity Provider attribute conditions | On repo rename / branch policy change | Update `attribute_condition` on the provider, then re-run `pulumi up` |
| Push SA email / project | Never (one per project, lifetime of the project) | If forced, see "Service account replacement" below |
| Bootstrap JSON key (if it exists) | 90 days max, ideally never used after first apply | `gcloud iam service-accounts keys delete <KEY_ID> --iam-account=<EMAIL>` |
| `vars.AWS_DEPLOY_ROLE_ARN` / `vars.GCP_CI_PUSH_SA` GitHub repo variables | When the underlying identity changes | GitHub UI: *Settings → Secrets and variables → Actions → Variables* |

The OIDC-based path has **no symmetric secret to rotate** — that's the whole
point of using WIF instead of stored keys. If a workflow run succeeds today
with the current OIDC config, no action is required tomorrow.

---

## Required GitHub repo configuration

The `docker` job in `.github/workflows/ci.yml` reads three GitHub Actions
**variables** (not secrets — variables are non-sensitive and safe to expose):

| Variable | Example value | Purpose |
| --- | --- | --- |
| `GCP_WORKLOAD_IDENTITY_PROVIDER` | `projects/123456789/locations/global/workloadIdentityPools/github-actions/providers/github-actions-oidc` | Target of OIDC token exchange |
| `GCP_CI_PUSH_SA` | `kaizen-ci-push@kaizen-experimentation-dev.iam.gserviceaccount.com` | SA the runner impersonates |
| `GCP_PROJECT_ID` | `kaizen-experimentation-dev` | Used to build AR image refs |
| `GCP_AR_LOCATION` (optional) | `us` | AR multi-region; defaults to `us` |
| `AWS_DEPLOY_ROLE_ARN` | `arn:aws:iam::123456789012:role/kaizen-ci-push` | AWS OIDC role |
| `AWS_REGION` (optional) | `us-east-1` | ECR region |

When any of `GCP_WORKLOAD_IDENTITY_PROVIDER` / `GCP_CI_PUSH_SA` /
`GCP_PROJECT_ID` are missing, the workflow logs a warning and pushes only to
the registries it can authenticate to. **The cross-registry digest-parity
step is skipped** when only one registry is configured.

---

## Bootstrap (one-time, per project)

Before the WIF path can authenticate, the underlying GCP resources must
exist. This is the only step that requires a human with `roles/owner`.

```bash
PROJECT_ID=kaizen-experimentation-dev
PROJECT_NUMBER=$(gcloud projects describe "${PROJECT_ID}" --format='value(projectNumber)')
GH_OWNER=<github-org>
GH_REPO=<github-repo>     # e.g. kaizen-experimentation

# 1. Enable required APIs.
gcloud services enable \
  artifactregistry.googleapis.com \
  iamcredentials.googleapis.com \
  --project="${PROJECT_ID}"

# 2. Create the push service account.
gcloud iam service-accounts create kaizen-ci-push \
  --project="${PROJECT_ID}" \
  --display-name="Kaizen CI image push"

# 3. Create the Workload Identity Pool + provider for GitHub Actions OIDC.
gcloud iam workload-identity-pools create github-actions \
  --project="${PROJECT_ID}" \
  --location=global \
  --display-name="GitHub Actions"

gcloud iam workload-identity-pools providers create-oidc github-actions-oidc \
  --project="${PROJECT_ID}" \
  --location=global \
  --workload-identity-pool=github-actions \
  --display-name="GitHub OIDC" \
  --issuer-uri="https://token.actions.githubusercontent.com" \
  --attribute-mapping="google.subject=assertion.sub,attribute.repository=assertion.repository,attribute.ref=assertion.ref" \
  --attribute-condition="assertion.repository=='${GH_OWNER}/${GH_REPO}' && assertion.ref=='refs/heads/main'"

# 4. Allow the push SA to be impersonated from this repo's main branch only.
gcloud iam service-accounts add-iam-policy-binding \
  "kaizen-ci-push@${PROJECT_ID}.iam.gserviceaccount.com" \
  --project="${PROJECT_ID}" \
  --role="roles/iam.workloadIdentityUser" \
  --member="principalSet://iam.googleapis.com/projects/${PROJECT_NUMBER}/locations/global/workloadIdentityPools/github-actions/attribute.repository/${GH_OWNER}/${GH_REPO}"

# 5. Now run Pulumi to create the AR repos and bind the SA as writer per repo.
cd infra
pulumi config set kaizen-experimentation:gcpCiPushPrincipal \
  "serviceAccount:kaizen-ci-push@${PROJECT_ID}.iam.gserviceaccount.com" \
  --stack gcp-dev
pulumi up --stack gcp-dev
```

The `attribute-condition` above pins impersonation to **main branch of one
repo**. Branch protection is part of the trust boundary — without it, anyone
who can push a workflow file on a feature branch can push to AR. Keep this
condition strict.

After bootstrap, set the three GitHub Actions variables documented above
(GCP_WORKLOAD_IDENTITY_PROVIDER, GCP_CI_PUSH_SA, GCP_PROJECT_ID).

---

## Emergency revoke (suspect compromise)

If you have any reason to believe the push SA has been compromised — a
suspicious image in AR, an unexpected workflow run, an alert from
Chronicle/SCC — execute the following **in this order** to stop the bleed
without breaking unrelated tenants:

```bash
PROJECT_ID=kaizen-experimentation-dev

# 1. Suspend the push SA. This blocks ALL operations using it, including
#    in-flight CI jobs. Effect is immediate.
gcloud iam service-accounts disable \
  "kaizen-ci-push@${PROJECT_ID}.iam.gserviceaccount.com" \
  --project="${PROJECT_ID}"

# 2. Audit recent activity. Look for image pushes outside main-branch SHAs.
gcloud logging read \
  "protoPayload.serviceName=artifactregistry.googleapis.com AND \
   protoPayload.authenticationInfo.principalEmail=kaizen-ci-push@${PROJECT_ID}.iam.gserviceaccount.com AND \
   protoPayload.methodName:UploadArtifact" \
  --project="${PROJECT_ID}" \
  --limit=200 \
  --format='table(timestamp, protoPayload.resourceName, protoPayload.requestMetadata.callerIp)'

# 3. If the WIF binding is implicated (someone bypassed branch protection),
#    revoke at the pool level. This kills ALL OIDC auth from GitHub until a
#    new attribute-condition is set.
gcloud iam workload-identity-pools providers update-oidc github-actions-oidc \
  --project="${PROJECT_ID}" \
  --location=global \
  --workload-identity-pool=github-actions \
  --attribute-condition="false"   # block everything

# 4. Once the IR is complete, re-enable with a tightened condition.
```

Recovery from step 3 requires re-running the bootstrap step 4 with whatever
new condition the IR concludes is appropriate. Do **not** re-enable with the
old condition without first auditing branch-protection rules and the
attribute-mapping.

---

## Service-account replacement (rare)

If for any reason the push SA must be replaced (compromise without ability
to verify rotation, project migration, etc.):

1. Create the new SA: `kaizen-ci-push-v2@<project>.iam.gserviceaccount.com`.
2. Bind it to the WIF pool (step 4 of bootstrap, with the new SA email).
3. Update the GitHub Actions variable `GCP_CI_PUSH_SA` to the new email.
4. Update `kaizen-experimentation:gcpCiPushPrincipal` in `Pulumi.gcp-dev.yaml`
   (and any other GCP stack files), then `pulumi up` — this revokes the old
   SA's per-repo writer binding and grants the new SA.
5. After two consecutive successful CI runs on `main`, suspend then delete
   the old SA:
   ```bash
   gcloud iam service-accounts disable kaizen-ci-push@... --project=...
   # wait 24h to be sure no in-flight runs use it
   gcloud iam service-accounts delete kaizen-ci-push@... --project=...
   ```

The order matters: rotate **principal** before revoking the old binding, so
there's never a window where CI cannot push. Pulumi's apply is the
authoritative cutover.

---

## What NOT to do

- **Do not** use `roles/artifactregistry.admin` for the CI push SA. Writer
  is sufficient. Admin can delete repos and policies — not needed for
  pushing tags.
- **Do not** grant the SA at the project level (`gcloud projects add-iam-policy-binding`).
  Pulumi grants per-repo so the blast radius of a compromised SA is bounded
  to the Kaizen AR repos and not, say, any other project AR repos that
  happen to live alongside.
- **Do not** create static JSON keys for this SA. If you need them
  temporarily for local debugging, use short-lived credentials instead:
  ```bash
  gcloud auth print-access-token \
    --impersonate-service-account=kaizen-ci-push@${PROJECT_ID}.iam.gserviceaccount.com
  ```
- **Do not** loosen the WIF attribute-condition to `attribute.repository=='*'`
  or remove the `ref==refs/heads/main` clause. Both are part of the trust
  boundary.

---

## Related

- Pulumi module: `infra/pkg/gcp/cicd/artifact_registry.go`
- CI job: `.github/workflows/ci.yml` → `docker`
- Stack config: `infra/Pulumi.gcp-dev.yaml`
- Spec: `docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md` (Container Image Strategy)
- Issue: #482
