# progress.log — #501 M4b GCE chaos test

Session log for issue #501 (worker `claude-web-501-20260706`, charter
`docs/agents/registry/infra-4.md`). OKF `log.md` conventions: `## YYYY-MM-DD`
headings, newest entries first, append-only.

## 2026-07-06

- **Workflow-permission probe result**: confirmed — the push of
  `.github/workflows/weekly-chaos-gcp.yml` was rejected (`refusing to allow a
  GitHub App to create or update workflow ... without 'workflows'
  permission`). Fallback executed: the workflow is staged verbatim at
  `docs/ci/weekly-chaos-gcp.yml` with a one-line maintainer install step
  (`git mv` into `.github/workflows/`), referenced from the runbook, the PR,
  and the issue. The offending commit was amended away (`git reset` was not
  in the allowed tools; `git rm --cached` + `git commit --amend` was).
- **Deliverables committed**: `scripts/chaos_kill_m4b_gce.sh` (chaos harness:
  `delete`/`panic` kill → probe until serving → outage `< RECOVERY_SLA_MS`
  assertion → RocksDB survival + stateful-disk-reattach verification, with
  `--runs N` for consecutive-run acceptance evidence), `just chaos-policy-gce`
  recipe, `docs/runbooks/m4b-gce-chaos.md`, and weekly CI wiring
  (`weekly-chaos-gcp.yml`, Sunday 03:00 UTC — pushed as a separate commit in
  case this executor's GitHub App lacks `workflows` permission; on rejection
  the YAML lands via the runbook/PR for a maintainer to apply). Live 3-run
  validation is acceptance follow-through: the M4b endpoint sits on a private
  subnet unreachable from GitHub-hosted runners, so the CI job is gated off
  behind `vars.GCP_CHAOS_ENABLED` until a VPC-attached runner or forwarder
  path exists — noted on the issue.
- **Baseline (startup ritual step 3)**: `just test-infra` could not execute in
  this session — `just`/`go test` are not in the runner's allowed tools
  (dispatcher may widen `--allowedTools`). Recorded per ritual. Mitigation:
  the branch is cut from green main (`e4ec72a`), and this change touches no
  Go/Rust code — nothing in `test-infra`'s coverage (`infra/pkg/...`,
  `infra/test/...`) is modified (bash + justfile + markdown + workflow only).
  `shellcheck`/`bash -n` were likewise unavailable; the script was authored
  against the `chaos_kill_policy.sh` conventions and reviewed manually.
- **Branch (startup ritual step 2 deviation)**: the dispatch asked for
  `infra-4/test/m4b-chaos-recovery`; the claude-web executor cannot rename its
  harness-generated ref, so work rides on `claude/issue-501-20260706-0215` —
  the tolerated `claude/<slug>` family per CLAUDE.md branch-naming, with
  attribution carried by PR metadata (Conventional-Commit title + `infra-4`
  label inherited from #501).
- **Session plan (MODE: INIT)**: read charter, spec
  (`docs/superpowers/specs/2026-04-20-multi-cloud-gcp-aws-design.md`, Compute
  Model → M4b), #487 module (`infra/pkg/gcp/compute/m4b.go`), and the
  `weekly-chaos.yml` + `scripts/chaos_kill_policy.sh` precedents → build the
  GCE chaos harness against the #487 topology (`kaizen-<env>-m4b-mig`,
  deterministic instance `kaizen-<env>-m4b-0`, static IP
  `kaizen-<env>-m4b-ip`:50054, stateful disk device `m4b-data`) mirroring the
  local script's phase/report conventions → justfile recipe → runbook →
  weekly CI wiring following the `ci.yml` Workload Identity pattern → PR with
  `Closes #501`.
