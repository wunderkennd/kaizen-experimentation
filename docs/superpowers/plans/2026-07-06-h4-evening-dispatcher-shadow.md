# H4 evening dispatcher — shadow mode, Phase A (#716)

**Status:** Locked.
**Plan-review:** review note on [#716](https://github.com/wunderkennd/kaizen-experimentation/issues/716) (2026-07-06) — premises re-verified against probe #713's live outputs, `claude-code-review.yml`, and the H1 `dispatch.sh`/`ready.sh` interfaces, all read this session.
**Issue:** [#716](https://github.com/wunderkennd/kaizen-experimentation/issues/716) — P1, harness (no sprint cohort), child of Goal #682.
**Blocked by:** — none (probe #713 closed 2026-07-06).

---

## Summary

The H4 amendment (proposal §4, merged #712) retires the multiclaude daemon behind a
GitHub-native **evening dispatcher** on graduated evidence. This plan ships the first
rung: the dispatcher in **shadow mode** — a nightly scheduled workflow that computes
the ready set per sprint cohort with the H2 native `_ready`, applies the nightly cap,
and records what it *would* have dispatched, claiming nothing and launching nothing.

The same PR lands the two artifacts that probe #713 proved must be **registered on
`main` before live mode can exist**: the worker workflow (`claude-worker.yml` —
by-filename `workflow_dispatch` resolves against the default branch) and the H1
executor adapter (`dispatch.d/claude-workflow.sh`). Both are dormant in Phase A.

Trustworthiness constraint: the dispatcher is the piece that will eventually launch
autonomous workers unattended. Every failure mode it can have (duplicate dispatch,
launching blocked work, runaway fan-out) is already governed by an existing mechanism
(H1 claims, H2 edges, the cap) — this plan wires those mechanisms together and adds
**no new authority** until Phase B's double-gate is deliberately opened.

### Non-goals (v1 of #716)

- **No live dispatch.** Phase B flips it, gated on ≥3 clean shadow nights and an
  explicit owner action (repo variable). Nothing in this PR can launch a worker.
- **No multiclaude changes.** `just evening` and the daemon are untouched; retirement
  is Phase C (#682, owner-gated).
- **No shadow-vs-multiclaude scorecard tooling.** Phase B compares by reading the
  Actions history against what multiclaude picked up; automation of that comparison
  is decided then, not built speculatively now.
- **No mobile of the `@claude` comment path.** `dispatch.d/claude-web.sh` stays as
  the human/local adapter; it cannot work from Actions (probe #713 leg 3).

---

## Platform assumptions & probes

All assumptions this design bets on were exercised on this repository before this
plan — no new probes required.

| # | Assumption | Exercised here before? | Probe (task + command) | Verdict |
|---|---|---|---|---|
| PA1 | A workflow holding only the default `GITHUB_TOKEN` + `actions: write` can launch another workflow via `workflow_dispatch` | yes — probe #713 leg 1 (run 28759262585: `triggering_actor: github-actions[bot]`, success) | — | **confirmed** |
| PA2 | By-filename `workflow_dispatch` resolves against the **default branch** — worker workflows must land on `main` before first launch | yes — probe #713 leg 1 (HTTP 404 for branch-only workflow) | — | **confirmed** (design accommodates: worker ships dormant in this PR) |
| PA3 | `@claude` comments posted with `GITHUB_TOKEN` do NOT trigger `claude.yml` — comment-based dispatch is dead from Actions | yes — probe #713 leg 3 (0 runs) | — | **confirmed** (hence the workflow-launch adapter) |
| PA4 | `claude-code-action@v1` runs with an explicit `prompt:` input on a non-interactive trigger using `secrets.CLAUDE_CODE_OAUTH_TOKEN` | yes — `claude-code-review.yml` in production | — | **confirmed** |
| PA5 | Scheduled workflows fire and their run history is durable evidence | yes — `ready-drift.yml` (first run clean 2026-07-06) | — | **confirmed** |
| PA6 | `workflow_dispatch` inputs carry a multi-KB prompt | no — budget documented (~64KB payload) but not exercised at size | none pre-merge; L8 guards with a loud failure at 60,000 chars, and Phase B's first live dispatch is the natural probe | **guarded, verified in Phase B** |

---

## Locks — binding for implementers

| # | Lock | One-line answer | Decided (owner, date) |
|---|---|---|---|
| L1 | Cutover shape | Shadow-first; live is Phase B, gated on **≥3 clean shadow nights** read from the Actions history (ready-drift precedent) plus explicit owner enablement | owner via H4 amendment (#712), 2026-07-05 |
| L2 | Launch mechanism | `gh workflow run claude-worker.yml` with the default `GITHUB_TOKEN` (`actions: write`); **no PAT, no app token** — that keeps H5's credential surface unchanged | owner via H4 amendment + probe #713, 2026-07-06 |
| L3 | Worker | `.github/workflows/claude-worker.yml`: `workflow_dispatch` inputs `{issue, prompt}`, invokes `anthropics/claude-code-action@v1` with `prompt:` + `claude_code_oauth_token` (mirrors `claude.yml` permissions); dormant until Phase B | claude-web session, 2026-07-06 |
| L4 | Live double-gate | Live dispatch requires BOTH the `mode=live` workflow input AND repo variable `EVENING_DISPATCH_LIVE == "1"`; the script additionally requires `--live`. Shadow is the default at every layer | claude-web session, 2026-07-06 |
| L5 | Fan-out control | `DISPATCH_CAP` (default **3**) issues per night across all cohorts; candidates deduped across cohorts and ordered by ascending issue number for determinism | claude-web session, 2026-07-06 |
| L6 | No logic duplication | Readiness comes ONLY from `scripts/orchestration/ready.sh`; dispatch (claims included) ONLY from `dispatch.sh`. The dispatcher composes; it never reimplements | owner via H1/H2 design, standing |
| L7 | Adapter naming | New adapter is `dispatch.d/claude-workflow.sh` (Actions-launched worker). Existing `claude-web.sh` (@claude comment) remains the human/local path. Together they satisfy #682's "claude adapter" child item | claude-web session, 2026-07-06 |
| L8 | Prompt budget | Adapter fails loudly (exit 1, no truncation) if the rendered prompt exceeds **60,000 chars** — under the documented ~64KB `workflow_dispatch` payload ceiling with headroom for the issue input | claude-web session, 2026-07-06 |
| L9 | Shadow output | Shadow writes the would-dispatch report to `$GITHUB_STEP_SUMMARY` (and stdout); **no issue comments in shadow mode** — the Actions run history IS the evidence record | claude-web session, 2026-07-06 |

---

## Cross-phase artifacts

| Artifact | Producer phase / task | Consumer phase / task | Lock # | Status |
|---|---|---|---|---|
| `.github/workflows/claude-worker.yml` registered on `main` | A / A3 | B (first live launch) | L2, L3 | pending |
| `dispatch.d/claude-workflow.sh` adapter | A / A2 | B (via `dispatch.sh <n> claude-workflow`) | L7, L8 | pending |
| `evening_dispatch.sh --live` path (dormant) | A / A1 | B (flipped by double-gate) | L4 | pending |
| Shadow-night Actions run history | A / A4 (nightly from merge) | B gate (≥3 clean nights) | L1, L9 | pending |
| Repo variable `EVENING_DISPATCH_LIVE` | B (owner creates; **not** in this PR) | B live runs | L4 | pending |
| Phase B scorecard vs #682 success metric | B | C (retirement decision) | — | pending |

---

## Phase A — shadow dispatcher + dormant launch chain (this PR, #716)

**Executor:** claude-web (this session).
**Size budget:** ~420 counted lines / 8 counted files (soft gate 400/10 may warn — acceptable; hard gate 900/25 is far off; plan doc is markdown-exempt).

### Task A1: `scripts/orchestration/evening_dispatch.sh`

- [ ] **Step 1:** cohort resolution — args, else all labels matching `sprint-*` on open issues — file: `scripts/orchestration/evening_dispatch.sh`
- [ ] **Step 2:** ready-set per cohort via `ready.sh "$L"` (L6); merge, dedupe by number, sort ascending (L5)
- [ ] **Step 3:** cap to `DISPATCH_CAP` (default 3); report selected + overflow
- [ ] **Step 4:** shadow (default): print report; append to `$GITHUB_STEP_SUMMARY` when set (L9); exit 0
- [ ] **Step 5:** `--live`: for each selected issue run `"$DISPATCH_BIN" <n> claude-workflow` (default `dispatch.sh`, overridable for tests); count dispatched / already-claimed (exit 3) / failed; nonzero exit only on adapter failures

### Task A2: `scripts/orchestration/dispatch.d/claude-workflow.sh`

- [ ] **Step 1:** read prompt from stdin, `$1` = issue; enforce L8 (>60,000 chars → loud exit 1)
- [ ] **Step 2:** `gh workflow run claude-worker.yml -f issue="$1" -f prompt="$PROMPT"` (default branch implied)

### Task A3: `.github/workflows/claude-worker.yml`

- [ ] **Step 1:** `workflow_dispatch` inputs `{issue: required, prompt: required}`; `run-name` carries the issue number
- [ ] **Step 2:** permissions mirroring `claude.yml` (contents/PRs/issues read, id-token write, actions read); checkout; `claude-code-action@v1` with `prompt: ${{ inputs.prompt }}` + OAuth secret (L3)

### Task A4: `.github/workflows/evening-dispatcher.yml`

- [ ] **Step 1:** `schedule: cron "7 4 * * *"` + `workflow_dispatch` inputs `{mode: shadow|live default shadow, cohort: optional}`
- [ ] **Step 2:** permissions `contents: read, issues: write, actions: write`; concurrency group `evening-dispatch`
- [ ] **Step 3:** live only when `mode == 'live'` AND `vars.EVENING_DISPATCH_LIVE == '1'` (L4); otherwise invoke the script in shadow

### Task A5: offline tests + CI wiring

- [ ] **Step 1:** `scripts/orchestration/test_evening_dispatch.sh` — gh-stub pattern (as `test_ready_native.sh`): shadow reports ready set and posts nothing; cap + dedupe + ordering; `--live` calls the (stubbed) `DISPATCH_BIN` with `claude-workflow`; adapter logs the `workflow run` call; adapter rejects an oversize prompt
- [ ] **Step 2:** add the suite + new paths to `.github/workflows/orchestration-tests.yml`

### Task A6: docs

- [ ] **Step 1:** `scripts/orchestration/README.md` — dispatcher section + `claude-workflow` in the adapter list
- [ ] **Step 2:** proposal §4 H4 — one-line Phase A status annotation

---

## Phase B — limited live (separate issue, NOT this PR)

Gate: ≥3 consecutive clean shadow nights in the Actions history AND owner sets
`EVENING_DISPATCH_LIVE=1` (L1/L4). Scope: flip one cohort live with the cap,
collect #682's metrics (duplicate rate 0, acceptance ≥ multiclaude baseline,
review burden/cost ≤ current), PA6 verified on the first real launch. Filed as a
follow-up when the gate opens — a fresh issue keeps one-issue-one-PR intact.

## Phase C — full cutover + retirement (separate, owner-gated)

One full sprint dispatcher-only with multiclaude never invoked → the H4
amendment's Retire checklist (`just evening` retarget, `.multiclaude/` tombstone,
generator drops the view target). Decided on Phase B's scorecard at #682.

---

## Phase F — Convergence (folded into the single Phase-A PR)

### Task F1: Acceptance-criteria mapping

| Issue AC | Test/file location | Cross-phase artifact row |
|---|---|---|
| `evening_dispatch.sh` shadow default, cohort discovery, dedupe, cap, `--live` flag | `test_evening_dispatch.sh` (shadow/cap/dedupe/live cases) | `--live` path row |
| adapter launches worker with issue + prompt; loud fail past budget | `test_evening_dispatch.sh` (adapter cases) | adapter row |
| `claude-worker.yml` registered, dormant | file on `main` after merge; no launches in Phase A | worker row |
| `evening-dispatcher.yml` schedule + double-gated live | workflow file; L4 condition inspectable | shadow-history row |
| offline tests wired | `orchestration-tests.yml` diff | — |
| README + proposal updated | file diffs | — |

### Task F2: regression

`bash scripts/orchestration/test_dispatch.sh && bash scripts/orchestration/test_ready_native.sh && bash scripts/orchestration/test_evening_dispatch.sh && python3 scripts/check_docs.py`

### Task F3: PR

`feat(orchestration): H4 Phase A — evening dispatcher (shadow), worker + adapter (dormant)` — `Closes #716`.

---

## Test plan summary

| Phase | Test files | Count target |
|---|---|---|
| A | `scripts/orchestration/test_evening_dispatch.sh` | ≥10 assertions |

---

## Risks + rollback

| Risk | Severity | Mitigation |
|---|---|---|
| Shadow run mis-reads readiness (native `_ready` bug) | low | shadow writes reports only; `ready-drift.yml` independently checks `_ready` daily against the legacy parser until P3 |
| Worker workflow launched accidentally in Phase A | low | nothing calls it: shadow never dispatches; adapter unreachable except via `dispatch.sh <n> claude-workflow`, which no automation invokes yet; manual `workflow_dispatch` requires a human |
| Prompt exceeds input budget in Phase B | med | L8 loud failure + claim auto-release by `dispatch.sh`; PA6 verified on first live launch |
| Nightly schedule noise | low | L9: no comments; run summary only |

**Rollback for Phase A:** revert the PR — no state, no variables, no launched work.

**Replacement rule (graduated cutover):** the dispatcher ships ALONGSIDE
multiclaude with the shadow record as the drift check; `just evening` retarget and
daemon retirement are separate, later phases gated on the clean window and the
scorecard (L1) — never same-day. Precedent: #680 P1→P3.

---

## Follow-ups

| Item | Trigger | Owner |
|---|---|---|
| **#716.1** — Phase B limited-live issue | ≥3 clean shadow nights in Actions history | owner + claude-web |
| **#716.2** — Phase C retirement checklist | Phase B scorecard meets #682 metric | owner |
| **#716.3** — PA6 prompt-budget verification note on #682 | first live launch | dispatching session |

---

## Branch + PR conventions

- Branch: `claude/harness-workflow-generalize-yywa10` (tolerated harness-session family; attribution rides PR metadata per CLAUDE.md).
- Commits: Conventional Commits, `feat(orchestration):` scope.
- Single PR: `Closes #716`.
