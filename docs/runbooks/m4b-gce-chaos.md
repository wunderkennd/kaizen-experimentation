# Runbook: M4b GCE Chaos Test (kill instance, verify recovery < 10s)

Operates `scripts/chaos_kill_m4b_gce.sh` — the Phase 4 GCP validation of M4b's
crash-recovery invariant (issue #501; spec:
`docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md`, Compute
Model → M4b Policy Service). It is the cloud-level analog of the process-level
`scripts/chaos_kill_policy.sh`.

## What it verifies

M4b on GCP (provisioned by `infra/pkg/gcp/compute/m4b.go`, #487) is a single
stateful VM in a zonal MIG (`kaizen-<env>-m4b-mig`, instance
`kaizen-<env>-m4b-0`) with RocksDB on a stateful persistent disk (device
`m4b-data`) and a stateful internal IP (`kaizen-<env>-m4b-ip`, port 50054).
The test forcefully terminates the instance and asserts:

1. The MIG recreates the instance (instance id changes in `delete` mode).
2. The persistent disk reattaches (device `m4b-data` present post-recovery).
3. RocksDB state survives — a bandit experiment created *before* the kill
   still resolves afterwards (`SelectArm` returns an arm, not `NotFound`;
   `GetPolicySnapshot` still answers).
4. The outage window (first failed probe → first serving probe) is
   **< 10s** (`RECOVERY_SLA_MS`). M4b is gRPC — "returns 200" is implemented
   as TCP connect + a well-formed `SelectArm` gRPC response.

Probes poll every 0.2s with a 1s connect timeout, so measured outage is
biased upward by at most ~1.4s: a pass is conservative evidence for the SLO.

## Prerequisites

| Requirement | Detail |
| --- | --- |
| gcloud auth | `roles/compute.instanceAdmin.v1` on the target project (instance delete/describe/list, MIG describe/wait). `--kill-mode panic` additionally needs `roles/iap.tunnelResourceAccessor` + OS Login. |
| grpcurl | On PATH. M4b serves gRPC reflection, so no proto files are needed. |
| Network reach | The endpoint is a **private-subnet IP** (no public IP). Run from inside the VPC (VPC-attached runner / bastion VM), over VPN/Interconnect, or pass `--endpoint localhost:50054` behind a forwarder. An IAP TCP tunnel (`gcloud compute start-iap-tunnel`) works for a *functional* check but dies with the instance and its re-establishment inflates the measured outage — do not use it for SLA measurement. |
| Deployed stack | `pulumi up --stack gcp-<env>` including the M4b slice, with the M4b runtime serving on 50054. The boot image / startup automation must launch the runtime on a fresh instance — first-boot mounting alone (the #487 startup script) is not enough for recovery to complete. |

The script **aborts before killing anything** if the endpoint is not serving,
if the MIG lacks its autohealing health check, or if the stateful-disk policy
for `m4b-data` is missing. Targeting `--env prod` requires `--allow-prod`.

## Usage

```bash
just chaos-policy-gce --dry-run                 # preflight + plan, kills nothing
just chaos-policy-gce --project <ID> --env dev  # single kill/recover cycle
just chaos-policy-gce --runs 3                  # issue #501 acceptance evidence
just chaos-policy-gce --kill-mode panic         # sysrq kernel panic instead of delete
```

Key flags (see `--help` for all): `--env` (default `dev`), `--project`,
`--zone` (discovered from the instance when omitted), `--endpoint`
(discovered from the static IP when omitted), `--experiment-id` (reuse
existing state instead of creating a chaos experiment), `--recovery-sla`
(default 10000 ms), `--recovery-timeout` (default 600 s wait before giving
up — the SLA assertion is separate from the wait).

Each run without `--experiment-id` creates one cold-start bandit experiment
with `content_id` prefixed `chaos-gce-`; these accumulate and can be cleaned
up by the usual M4b retention path.

## CI wiring

The weekly job `weekly-chaos-gcp.yml` (Sunday 03:00 UTC, one hour after the
local-stack `weekly-chaos.yml`) runs `scripts/chaos_kill_m4b_gce.sh --runs 3`
with GCP OIDC auth, following the `ci.yml` Workload Identity pattern.

> **Install step (one-time)**: the workflow is staged at
> [`docs/ci/weekly-chaos-gcp.yml`](../ci/weekly-chaos-gcp.yml) because the
> #501 worker's GitHub App lacks the `workflows` permission. A maintainer
> lands it with:
> `git mv docs/ci/weekly-chaos-gcp.yml .github/workflows/weekly-chaos-gcp.yml`
> (drop the `STAGED WORKFLOW` comment header while at it).

It is gated on repository variables so it degrades to a no-op warning until
configured:

| Variable | Meaning |
| --- | --- |
| `GCP_WORKLOAD_IDENTITY_PROVIDER` | Existing WIF provider (shared with `ci.yml`). |
| `GCP_CHAOS_SA` | Service account with the IAM roles above. |
| `GCP_PROJECT_ID` | Target project. |
| `GCP_CHAOS_ENABLED` | Set to `true` only once the runner can reach the private endpoint (self-hosted VPC runner, or a firewall + forwarder path). GitHub-hosted runners cannot reach the private subnet, so the job stays off by default. |

## Recording acceptance evidence (issue #501)

Run `just chaos-policy-gce --runs 3` from a network-attached host and paste
the `CHAOS TEST REPORT` block into issue #501. Acceptance requires all three
runs `Outage(ms) <= 10000` with `State = true` and `Recreated = yes`.

## Triage

| Symptom | Likely cause |
| --- | --- |
| Abort: "not serving pre-chaos" | No network path to the private IP, or the M4b runtime is not running. Nothing was killed. |
| Recovery timeout, instance recreated | Boot image / startup automation does not launch the M4b runtime; check serial console: `gcloud compute instances get-serial-port-output kaizen-<env>-m4b-0`. |
| Outage > SLA but recovers | Measure components: MIG repair latency (`gcloud compute operations list --filter='operationType=repair'`), boot time, RocksDB warm-load. Compare with the AWS ASG path (~3-5s) in the spec table. |
| `State = false` (experiment lost) | Disk not reattached (check per-instance config: `gcloud compute instance-groups managed instance-configs list-instances ...`), or the runtime pointed at the wrong RocksDB path (`/data/rocksdb`). |
| `Recreated = no` after delete | The delete failed or the probe flapped without a real kill — see `kill_<run>.log` semantics in the script and the MIG's recent actions. |
