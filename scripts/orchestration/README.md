# Orchestration dispatch layer (harness H1, #679)

Executor-agnostic issue dispatch with a claim protocol and resume semantics.
Extracted from justfile heredocs so it is testable; the `just` façades are
unchanged for callers. Design: `docs/coordination/harness-modernization-proposal.md`
§4 H1 and §7 R2.

## Commands

```bash
just work-on 642                        # dispatch one issue (default executor)
just work-on 642 executor=claude-web    # …via the @claude GitHub Action
just sprint sprint-I.3 executor=jules   # dispatch every ready issue of a label
just autonomous-sprint I.3              # sprint-number front door (multiclaude)
just _ready sprint-I.3                  # {number,title} per ready issue
just test-orchestration                 # offline behavior tests (stubbed gh)
bash scripts/orchestration/claims.sh sweep   # expire stale leases (also in `just morning`)
```

## How a dispatch works

1. **Claim** (`claims.sh take`): add the `claimed` label + a lease comment
   `claim: executor=<e> worker=<w> expires=<ISO8601>` (TTL
   `ORCH_CLAIM_TTL_HOURS`, default 24). If an unexpired lease exists — or we
   lose the post-claim race — exit **3** ("already claimed"). This closes the
   double-dispatch window behind the #661/#663 and #664/#665/#666 duplicates.
2. **Render** the task prompt from the Issue. Mode is **INIT** (create branch +
   `progress.log.md` with OKF `log.md` conventions) unless a prior worker posted
   `progress-branch: <name>` — then **RESUME**: fetch that branch, read
   `progress.log.md` + git log, continue; duplicate PRs forbidden. Every prompt
   carries the startup ritual (sync → read state → **verify baseline green**
   before new work), the one-unit-per-session rule, merge-ready clean-state,
   append-only progress, and `Closes #N`.
3. **Adapt**: `dispatch.d/<executor>.sh` receives the prompt on stdin and the
   issue number as `$1`. Shipped adapters: `multiclaude` (worker daemon),
   `claude-web` (posts an `@claude` issue comment; `claude.yml` runs the session
   in GitHub-hosted compute — works from human/PAT contexts only, since
   `GITHUB_TOKEN` comments don't trigger workflows, probe #713),
   `claude-workflow` (launches `.github/workflows/claude-worker.yml` via
   `workflow_dispatch` — the Actions-safe launch path, probe #713), `jules`
   (cloud VM). Adding an executor = adding one file here — nothing else changes.
4. **Release on failure**: if the adapter exits non-zero the claim is released
   so the issue returns to the ready pool.

Claim state lives on the Issue (label + comments), never in executor memory —
externalized state per proposal §7 R2. An open PR with `Closes #N` supersedes a
lease; `claims.sh sweep` (run by `just morning`) clears stale leases.

## Ready predicate (`ready.sh`)

open ∧ not claimed ∧ no open closing PR ∧ no OPEN native dependency edges (body-parse fallback until P3 #694)
(beads DAG preferred when initialized). H2 (#680) replaces the body parsing
with native issue dependencies over GraphQL; the claimed/in-flight predicates
stay.

## Evening dispatcher (`evening_dispatch.sh`, H4 #716)

The nightly autonomous front door (`.github/workflows/evening-dispatcher.yml`,
cron 04:07 UTC): resolves open `sprint-*` cohorts → `ready.sh` per cohort →
dedupe, ascending order, `DISPATCH_CAP` (default 3) → **shadow** report to the
run summary. **Shadow is the default at every layer**; live dispatch (each
selected issue through `dispatch.sh <n> claude-workflow`) requires the
`mode=live` input AND repo variable `EVENING_DISPATCH_LIVE=1`, and opens only
after ≥3 clean shadow nights (plan `docs/superpowers/plans/2026-07-06-h4-evening-dispatcher-shadow.md`,
Locks L1/L4). Claim expiry is the self-healing loop: a dead worker's lease
lapses and the next night re-dispatches in RESUME mode.

## Tests

`test_dispatch.sh` stubs `gh` with a filesystem fake and records adapter calls;
covers the #679 acceptance criteria: double-dispatch guard, sweep-then-reclaim,
resume-mode prompts, ready exclusions, failure release. `test_ready_native.sh`
covers the H2 native `_ready` + migration; `test_evening_dispatch.sh` covers
the H4 shadow/live dispatcher and the `claude-workflow` adapter. CI:
`.github/workflows/orchestration-tests.yml`.
