# Ecosystem Governance Runbook (H6)

How PR-lifecycle governance — the Review gate, the PR-title lint, graduated
auto-merge, and branch-protection rulesets — is defined once in this repo and
applied across the whole Kaizen fleet. Design record: proposal §4 H6
(`docs/coordination/harness-modernization-proposal.md`).

## The fleet

Owner-supplied listing (2026-07-04) of `~/…/` on the primary workstation,
worktree checkouts deduped. **Only kaizen-experimentation is verified to exist
on GitHub**; confirm each sibling's GitHub presence before onboarding it.

| Repo | Role (inferred — correct me) | Governance state |
| --- | --- | --- |
| `kaizen-experimentation` | This platform. Harness + governance home | Active (ruleset `main`) |
| `kaizen-recsys` | Personalization service (ADR-029 / RFC-001 #543 consumer) | Listed, disabled |
| `kaizen-pipelines` | Data pipelines | Listed, disabled |
| `kaizen-rosetta` | Translation/codegen layer | Listed, disabled |
| `kaizen-os` | Platform umbrella / tooling | Listed, disabled |
| `kaizen-alchemy` | (also checked out as `alchemy-clean`) | Listed, disabled |
| `kaizen-accelerator` | Starter kit / perf | Listed, disabled |
| `kensho-repl` | Interactive analysis REPL | Listed, disabled |

"Listed, disabled" = present in `infra/github-governance/Pulumi.governance.yaml`
with `enforcement: disabled` — the ruleset exists as a no-op until the repo is
onboarded (a required check that never reports would block every merge).

## The three governance layers

| Layer | Lives at | Applied by |
| --- | --- | --- |
| **Workflow logic** (Review gate, PR-title lint, auto-merge) | `.github/workflows/_review-gate.yml`, `_pr-title.yml`, `_automerge.yml` (reusable, `workflow_call`) | Each repo adds ~20-line **caller** workflows |
| **Branch protection** (required checks, thread resolution, linear history) | `.github/rulesets/main.json` (this repo's copy) + `infra/github-governance/` (fleet) | `pulumi up` with an admin PAT, or one-time import/`gh api` per repo |
| **Repo toggles** (allow auto-merge, delete branch on merge) | Documented here; `settings.yml` keeps the intent record | One-time: Settings → General per repo (not ruleset material) |

Check-run contexts from reusable workflows are two-segment —
**`<caller job name> / <callee job name>`** — so the required contexts are
`Review gate / gate` and `PR title check / check`. Repo-local CI jobs
(schema/rust/go/…) keep single-segment names.

## Onboard a sibling repo

1. **Confirm the repo exists on GitHub** under the expected owner.
2. **Add the caller workflows** in that repo (`.github/workflows/`):

   ```yaml
   # review-gate.yml
   name: Review gate
   on:
     pull_request:
       types: [opened, reopened, ready_for_review, synchronize]
     pull_request_review:
       types: [submitted, edited, dismissed]
     pull_request_review_comment:
       types: [created, edited, deleted]
   permissions: { contents: read, pull-requests: read }
   concurrency:
     group: review-gate-${{ github.event.pull_request.number }}
     cancel-in-progress: true
   jobs:
     gate:
       name: Review gate
       if: '!github.event.pull_request.draft'
       permissions: { contents: read, pull-requests: read }
       uses: wunderkennd/kaizen-experimentation/.github/workflows/_review-gate.yml@main
   ```

   ```yaml
   # pr-title.yml
   name: PR title check
   on:
     pull_request:
       types: [opened, edited, reopened, synchronize]
   permissions: { contents: read }
   jobs:
     check:
       name: PR title check
       uses: wunderkennd/kaizen-experimentation/.github/workflows/_pr-title.yml@main
   ```

   ```yaml
   # automerge.yml — optional third caller; set inputs to the repo's risk model
   name: Auto-merge routine PRs
   on:
     pull_request:
       types: [opened, reopened, ready_for_review, synchronize, labeled, unlabeled]
   permissions: { contents: write, pull-requests: write }
   concurrency:
     group: automerge-${{ github.event.pull_request.number }}
     cancel-in-progress: true
   jobs:
     automerge:
       name: auto-merge
       if: '!github.event.pull_request.draft'
       permissions: { contents: write, pull-requests: write }
       uses: wunderkennd/kaizen-experimentation/.github/workflows/_automerge.yml@main
       with:
         risk-labels: "breaking,needs-human-input"
         risky-path-prefixes: ""   # e.g. "proto/,migrations/"
   ```

   Notes: the `@main` refs are **owner-qualified** — update them if the
   workflows' home repo transfers to the org. Cross-repo `workflow_call`
   works because kaizen-experimentation is public.
3. **Open a PR in the sibling with the callers and verify** the two checks
   report with the expected two-segment names.
4. **Enable protection**: in `Pulumi.governance.yaml`, set the repo's
   `enforcement: active` and add its own CI contexts to `requiredChecks`,
   then apply (below). Or one-off: import `.github/rulesets/main.json` in
   that repo's UI and edit the contexts.
5. **Flip repo toggles** once: Settings → General → *Allow auto-merge* ✓,
   *Automatically delete head branches* ✓.

## Apply the fleet stack

```bash
cd infra/github-governance
pulumi stack select governance   # create on first use: pulumi stack init governance
GITHUB_TOKEN=<admin-PAT> pulumi preview   # drift check — read-only
GITHUB_TOKEN=<admin-PAT> pulumi up
```

- **PAT scoping (H5)**: fine-grained PAT, *only* the governed repos,
  permission **Administration: read+write** (rulesets) — nothing else.
  Fine-grained PATs are **per resource owner**: during the user→org
  migration window you either run the stack twice with per-owner tokens
  (comment out the other owner's entries) or use a classic PAT that spans
  both. Store nowhere; export for the one command.
- The default `GITHUB_TOKEN` of Actions **cannot** manage rulesets — this
  stack is run by a human (or a dedicated runner with the PAT as a secret,
  once H5 lands).
- `pulumi preview` against a repo that doesn't exist 404s — that's the
  existence check for unverified siblings.

## Org migration (wunderkind-ventures)

Sequencing that keeps automation alive through the move:

1. **Before any transfer**: land this governance layer; it's owner-keyed, so
   moves are config edits.
2. **Transfer a low-traffic repo first** (e.g. `kensho-repl`) to shake out
   integration breakage. Update its `owner:` in `Pulumi.governance.yaml`.
   Rulesets, issues, PRs, and settings transfer with a repo; git redirects
   old-URL fetches/pushes.
3. **Re-point what redirects don't cover**, per transferred repo:
   - GitHub App installations (Devin, Claude, Settings if used) are
     per-owner — reinstall/re-grant under the org.
   - Fine-grained PATs are per-owner — mint org-scoped replacements
     (multiclaude/Jules/Devin credentials).
   - Actions `uses:` owner-qualified refs in *other* repos' callers.
   - Claude Code web session scopes / integrations pinned to the old
     `owner/repo`.
4. **kaizen-experimentation last** — it has the most automation wired to its
   identity (this session included).
5. **After the whole fleet is over** and the org plan supports org rulesets
   (**Team suffices** — verified 2026-07-04 against the github/docs source):
   flip `orgMode: true` — the universal rules collapse into ONE org ruleset;
   per-repo entries keep only repo-specific CI contexts. Start it `disabled`,
   flip `active` once callers are fleet-wide — the `evaluate` dry-run status
   (Rule Insights) is **Enterprise-only** and a Team org rejects it, which is
   why the stack defaults `orgEnforcement` to `disabled`. Also set the
   `fallback-reviewer` input on automerge callers (an org can't be requested
   as a reviewer the way a user-owner can).

   Plan gating summary (2026-07-04): repo rulesets — free on public repos,
   Pro/Team for private; org rulesets — Team+; `evaluate` mode and the
   audit-log API — Enterprise. Everything this harness *requires* fits Team.

## Troubleshooting

- **A required check never reports on a sibling PR** → the caller workflow is
  missing/misnamed there, or the context in the ruleset doesn't match the
  live check-run name. Compare against the PR's Checks tab verbatim.
- **`Invalid workflow file … Unexpected value 'pull_request_review_thread'`**
  → that trigger is rejected by this GitHub's validator (see #681 finding,
  2026-07-04); the grace window in `_review-gate.yml` is the substitute.
  Don't re-add it to callers.
- **Auto-merge enable fails soft** → *Allow auto-merge* toggle off, or the
  repo has no ruleset requiring checks yet (GitHub refuses to arm auto-merge
  with nothing to wait for).
- **Pulumi state after a transfer** → the ruleset resource address embeds the
  old owner; `pulumi state rename` (or delete + re-import) after editing the
  config. `pulumi preview` shows exactly what it thinks changed — read it
  before `up`.
