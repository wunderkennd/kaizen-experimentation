# Harness Modernization & Generalization Proposal

> **Status:** Proposed · **Scope:** the orchestration harness (dispatch, agents, work graph,
> merge path, knowledge layer) — no product code.
> **Audience:** repo owner + anyone maintaining the multi-tool workflow.
> **Companion docs:** `docs/guides/orchestration-workflow.md` (current model),
> `docs/guides/projects-and-goals.md` (work-tracking migration already in flight).

## TL;DR

The harness works — 204 PRs shipped through it — but it has grown five tool-specific
integrations, four copies of every agent's identity, a markdown-parsed dependency DAG, and a
merge queue that is a *prompt* rather than a platform feature. The systemic bug it produces is
already filed (#521: duplicate dispatch) and already bit us twice in one week (#661/#663,
#664/#665/#666).

The proposal in one sentence: **keep GitHub as the coordination bus, make every executor
pluggable behind one task contract with a claim protocol, collapse agent identity to one
registry, and replace bespoke glue with platform-native primitives** (sub-issue dependencies,
merge queue, rulesets, `claude-code-action`).

| # | Move | Kills | Effort |
| --- | --- | --- | --- |
| H0 | Quick wins: merge/decide #632, refresh stale pins, fix placeholders, archive dead docs, unvendor the 186-file agent library | staleness, 2.6 MB of unwired prompts | hours |
| H1 | Claim protocol + executor-agnostic dispatch (`work-on N executor=X`) | #521 duplicate PRs, tool lock-in | ~1–2 days |
| H2 | Native sub-issue dependencies as the work graph; retire awk/`## Blocked by` parsing | fragile DAG, hardcoded `sprint-I.3` automation, beads sync burden | ~1–2 days |
| H3 | Ruleset + required checks + GitHub merge queue; merge-queue agent becomes a shepherd | prompt-as-infrastructure, un-codified branch protection | ~1 day + settings |
| H4 | Executor consolidation pilot: Claude Code (remote/scheduled/`@claude`) as the reference executor, with ADR-031-style kill criteria. *Amended 2026-07-05:* the pilot vehicle is a GitHub-native **evening dispatcher**; multiclaude retires behind it on evidence (don't vendor the daemon) | 5-tool maintenance surface, local-daemon dependence | 1 sprint, evaluated |

---

## 1. What the harness is today

Five planes, inventoried 2026-07-02:

```
┌────────────────────────────────────────────────────────────────────────┐
│ KNOWLEDGE      CLAUDE.md · docs/guides/* · docs/coordination/*         │
│                agent identity ×4: .multiclaude/agents/ (12) ·          │
│                docs/coordination/prompts/ (7) · docs/onboarding/ (8) · │
│                agents/{worker,merge-queue,pr-shepherd,reviewer}.md     │
├────────────────────────────────────────────────────────────────────────┤
│ WORK GRAPH     GitHub Issues (source of truth) · Projects v2 + Goals   │
│                "## Blocked by" markdown → awk (justfile:_ready,        │
│                auto-promote.yml) · beads mirror (.beads/, 2 sync       │
│                scripts) for Gas Town                                   │
├────────────────────────────────────────────────────────────────────────┤
│ DISPATCH       justfile:705–1255 — morning/evening/interactive/        │
│                autonomous-sprint/work-on/_ready/prime-issue            │
│                5 executors, 5 integration styles:                      │
│                  gt (separate ~/gt checkout) · multiclaude (daemon +   │
│                  tmux) · jules (2 Actions + CLI) · devin (echo a       │
│                  prompt) · gemini (one-liner)                          │
├────────────────────────────────────────────────────────────────────────┤
│ MERGE/VERIFY   ci.yml (620 lines) · pr-title + pr-label-inheritance    │
│                (attribution) · auto-ready · branch-naming (advisory) · │
│                stub-markers · merge-queue = agents/merge-queue.md      │
│                (a persona, not a platform feature)                     │
├────────────────────────────────────────────────────────────────────────┤
│ EXECUTORS      Gas Town · Multiclaude · Jules · Devin · Gemini ·       │
│                Claude Code (solo/web/@claude)                          │
└────────────────────────────────────────────────────────────────────────┘
```

### Credit where due — the harness has been modernizing itself

This proposal continues a trajectory, it doesn't start one:

- Status files → **GitHub Issues** (Phase 5), then Milestones → **Projects v2 Iterations +
  Goals** (#656, `projects-and-goals.md`).
- Branch-name attribution → **PR-metadata attribution** (#671: title lint + label
  inheritance), with harness-generated branches tolerated instead of fought.
- Manual draft flipping → **auto-ready on "done" signal** (#674).
- Repo settings → **settings-as-code** (#670).
- Dependency awareness → `_ready` + `auto-promote.yml` + beads DAG
  (`docs/superpowers/plans/2026-05-05-autonomous-dispatch-loop.md`).
- Cross-tool skills pinned in `skills-lock.json` rather than copy-pasted prompts.

Each of those moves replaced something bespoke with something declarative or
platform-native. H0–H4 below apply the same instinct to the remaining bespoke layers.

---

## 2. Pain points, grounded

**P1 — Dispatch has no claim step (#521).** `_ready` excludes issues with an open *closing
PR*, but a worker holds no visible lease between "dispatched" and "PR opened" — the window in
which a second dispatcher (or a human running `just autonomous-sprint` twice, or Palette
tooling) launches a duplicate. Evidence from one week: ADR-031 got two parallel
implementations (#661/#663); the Audit-Log Palette task got three (#664/#665/#666). Cost:
review time, CI minutes, and a risky "merged-with-red-history" resolution.

**P2 — Agent identity is defined in four places.** `.multiclaude/agents/agent-3-metrics.md`,
`docs/coordination/prompts/agent-3-metrics.md`, `docs/onboarding/agent-3-metrics.md`, and the
`agent-3` label semantics in CLAUDE.md all describe Agent-3. The shared skeleton (ownership →
language/crate/port → responsibilities → standards → work-tracking) is copy-pasted, and the
Work Tracking block repeats near-verbatim ×12 with only the label changing. Drift is not
hypothetical — #590 documented three sources disagreeing about ADR-025's status.

**P3 — Five executors, five integration styles.** Gas Town needs a separate `~/gt` checkout
and tmux; Multiclaude needs a daemon, its own `agents/*.md` personas, and a config currently
tuned only for the infra track (`branch_prefix: "infra-"`, infra-only CI checks — product
workers inherit the wrong defaults); Jules has two Actions plus a CLI recipe pointing at a
placeholder repo (`justfile:706` → `your-org/kaizen`); Devin integration is `@echo` of a
prompt; Gemini is a one-liner. None of them consume a shared task shape beyond "the issue
body", and none of them can be swapped without editing recipes.

**P4 — The dependency DAG is parsed from markdown with awk, thrice.** `justfile:_ready`,
`auto-promote.yml`, and `scripts/beads-sync.sh` each re-implement `## Blocked by` extraction
(`awk '/^## Blocked by/{flag=1…'`). `auto-promote.yml` additionally hardcodes the
`sprint-I.3` label, so every new sprint silently loses auto-promotion. GitHub has since
shipped native sub-issues (already adopted for Goals) and issue dependencies
("blocked by/blocking"), which make all three parsers unnecessary.

**P5 — The merge queue is a prompt.** `agents/merge-queue.md` instructs an agent to poll
`gh pr list`, eyeball five checklist items, and squash-merge. There is no `merge_group`
trigger, no ruleset-required checks codified anywhere (branch protection lives only as
comments in three workflow files), and `.github/settings.yml` carries exactly one setting.
A prompt can drift, hallucinate, or die mid-loop; a platform merge queue cannot.

**P6 — Staleness the harness can't see.** `claude.yml` pins `claude-3-7-sonnet-latest`
(three generations old; open PR **#632** already removes it and adds an automated review
workflow — it has been sitting as "owner decision"). `.claude/settings.json` pins
`claude-opus-4-7[1m]` (one generation old). `sccache-action@v0.0.4` is the lone
patch-pinned action. `docs/coordination/playbook.md` still describes the Phase 0–4
status-file cycle that `github-issues-workflow.md` explicitly replaced. Nothing audits the
harness itself — the Jules weekly maintenance prompt covers `cargo outdated`, not workflow
pins or model IDs.

**P7 — Orchestration logic lives inside a 1255-line justfile.** `_ready`,
`autonomous-sprint`, `work-on`, `prime-issue`, and the sprint→label maps are inline bash
heredocs — unlintable, untestable, and duplicated (the sprint map appears in `evening`,
`beads-sync`, and `autonomous-sprint`). By contrast, the checks that graduated to
`scripts/*.py` (`check_branch_name.py`, `check_stub_markers.py`) are shared cleanly between
`just` and CI.

**P8 — 2.6 MB of unwired vendored prompts.** `.claude/agents/` contains one real harness
agent (`pr-triage.md`) plus a 186-file third-party agent library (marketing, game-dev,
spatial-computing…) committed wholesale, with its own README/LICENSE/scripts. It bloats every
agent's tool-listing, none of it is referenced by the harness, and it's exactly the thing
`skills-lock.json` was invented to avoid vendoring.

---

## 3. Design principles for the next harness

1. **GitHub is the coordination bus.** Issue = task contract; sub-issue edges = DAG; labels =
   routing; PR = output; checks = gate; merge queue = ratchet. Anything that can be a GitHub
   primitive should be, because every executor — present or future — already speaks GitHub.
2. **Executors are pluggable.** A tool earns its place by consuming the task contract and
   honoring the claim protocol, not by having recipes named after it.
3. **Claim before work.** No executor starts without taking a visible, expiring lease on the
   issue. Idempotent dispatch is the fix for #521, not smarter dedup after the fact.
4. **One source per fact.** One agent registry; generated or referenced views elsewhere.
   (Same argument that moved status out of markdown files in Phase 5.)
5. **Prompts are not infrastructure.** Where a platform feature exists (merge queue,
   dependencies, rulesets, scheduled actions), the prompt version is a liability.
6. **The harness audits itself.** Model IDs, action pins, and placeholder strings are lint
   targets like any other code.

---

## 4. The moves

### H0 — Quick wins (hours; no design debate needed)

> **Execution status (2026-07-02, this session):** done — #632 title fixed +
> branch updated for merge, `settings.json` model → `claude-opus-4-8`,
> `justfile` Jules placeholder fixed, `auto-promote.yml` generalized to any
> `sprint-*` label, `playbook.md` archived with a historical banner, Agency
> library unvendored (restore via `just install-agents`), Goal #649 sub-issues
> backfilled. Deferred: the sccache pin bump (verify the current release via
> Jules/dependabot rather than guessing) and the
> `--dangerously-skip-permissions` flip — removing it would stall unattended
> workers on `git push` approval, so it needs the H1 worker-permission profile,
> not a blind config edit.

- **Decide #632.** Recommended: merge. It deletes the stale `claude-3-7-sonnet-latest` pin
  (falling back to the action's current default model) and adds `claude-code-review.yml` for
  automated PR review — which directly addresses the duplicate/attribution review burden.
  If automated review on *every* PR synchronize is too chatty, gate it on `ready_for_review`
  only, but merge the model-pin fix regardless.
- **Refresh remaining pins**: `.claude/settings.json` model → current Opus;
  `mozilla-actions/sccache-action` → current release; consider SHA-pinning the third-party
  actions (`peter-evans/*`, `dorny/*`, `google-labs-code/*`) per supply-chain hygiene.
- **Fix `justfile:706`** — `jules remote new --repo your-org/kaizen` has never pointed at
  this repo. Derive the slug from `gh repo view --json nameWithOwner` instead of hardcoding.
- **Generalize `auto-promote.yml`** — drop the `sprint-I.3` literal; trigger on any
  `sprint-*` label (one-line filter change), until H2 retires the workflow entirely.
- **Archive `docs/coordination/playbook.md`** (Phase 0–4 status-file cycle) with a pointer to
  `orchestration-workflow.md`; mark the Milestones section of `github-issues-workflow.md`
  as historical (its own banner already says the migration happened).
- **Unvendor `.claude/agents/` library**: keep `pr-triage.md` (and any agent actually
  referenced), move the 186-file collection behind its existing plugin
  (`everything-claude-code` is already in `enabledPlugins`) or an entry in `skills-lock.json`.
  One `just install-skills`-style restore path already exists; use it.
- **Revisit `--dangerously-skip-permissions`** in `.multiclaude/config.json` worker args —
  `.claude/settings.json` now expresses a curated allow/deny/ask policy; workers should
  inherit it rather than bypass it.

### H1 — One task contract, one claim protocol, N executors (~1–2 days)

**Task contract** (already 90% true, so codify it): a dispatchable issue has a body with
`## Summary`, `## Acceptance Criteria`, optional `## Blocked by` (until H2), an `agent-N` or
`infra-N` label, and a sprint iteration. `just prime-issue` already upserts the execution
banner; extend it to validate the contract.

**Claim protocol** (the #521 fix):

1. Dispatcher (any tool) atomically adds label `claimed` + a structured comment
   `claim: <executor>/<worker-id> expires <ISO8601>` before starting work.
2. `_ready` (and any other dispatcher) excludes issues labeled `claimed` — in addition to
   the existing open-closing-PR exclusion.
3. Claims expire: a scheduled action (or `just morning`) clears `claimed` labels whose
   comment timestamp is older than the lease (e.g. 24 h) with no linked PR — self-healing
   against dead workers, no daemon required.
4. Opening the PR with `Closes #N` supersedes the claim (existing in-flight logic).

This is deliberately dumb — label + comment — so every executor (multiclaude worker, Gas Town
polecat, Claude Code remote session, Jules, a human) can participate with plain `gh`.

**Executor-agnostic dispatch**: replace tool-named recipes with one façade:

```
just work-on 642                       # default executor (configured)
just work-on 642 executor=multiclaude  # today's behavior
just work-on 642 executor=claude-web   # Claude Code remote session
just work-on 642 executor=jules        # cloud VM
just sprint I.3 executor=multiclaude   # replaces autonomous-sprint
```

Implementation: `scripts/orchestration/dispatch.sh <issue> <executor>` — claim, render the
task prompt from the issue (the same string `work-on` builds today), invoke the executor
adapter (`dispatch.d/multiclaude.sh`, `dispatch.d/claude-web.sh`, …), fall back with claim
release on failure. The justfile keeps thin wrappers; the sprint→label map moves to one
place (`scripts/orchestration/sprints.json` or derived from the Project Iteration field).
This also relocates `_ready` and friends out of justfile heredocs into testable scripts
(P7), with `just`-side names unchanged.

### H2 — GitHub-native work graph (probe-gated; ~2 days work + a drift window)

Adopt **native issue dependencies** (blocked-by/blocking) and **sub-issues** as the only DAG.

This finishes what #656 started. That PR migrated the *reporting* plane (Project #5,
Iteration field, Goal issues, three views) but deliberately left the *automation* plane on
labels/milestones — `projects-and-goals.md` says so explicitly: "the Owner/Iteration fields
are for humans and the Roadmap; the labels are for machines, **until the orchestration layer
speaks GraphQL**." H2 is that "until."

> **Plan v2 (2026-07-05)** — reviewed against the post-H1/H6 codebase. Executed as
> **dispatchable sub-issues** #691 (P0 probe) → #692 (P1) → #693 (P2) → #694 (P3,
> calendar-gated), chained with `## Blocked by` today and converted to native edges by
> P0 itself; #680 is the coordinator (recommended: one worker session claims P0→P1→P2
> sequentially for design coherence; P3 dispatches separately after the drift gate).
> What changed from v1 and why:
>
> 1. **Probe-gated (P0)**: v1 bet the design on an unverified platform API. This repo was
>    burned twice in one day by exactly that — the workflow validator rejected the
>    *documented* `pull_request_review_thread` trigger, and ruleset `evaluate` turned out
>    Enterprise-gated. Sub-issues are proven live here (the #649 backfill); the
>    **dependencies API surface is not** (no `data/features/issue-dependencies.yml` flag
>    exists in github/docs). P0 creates two throwaway issues, exercises the REST edge
>    endpoints, introspects GraphQL for `blockedBy`/`subIssues`/
>    `closedByPullRequestsReferences`, and records design A (one GraphQL query per label
>    cohort) vs design B (GraphQL + batched REST reads) before anything is built.
> 2. **Graduated cutover, not same-day deletion**: `ready.sh` becomes native-first with the
>    body-parse path demoted to deprecated fallback plus a `READY_DRIFT=1` mode diffing the
>    two; the awk parser, the old in-flight grep, and `auto-promote.yml` are deleted only
>    after ≥5 drift-free days or one full sprint (the repo's own beads-first /
>    disabled→active pattern). Where design A holds, in-flight detection upgrades from the
>    `Closes #N` text search to `closedByPullRequestsReferences` — catches manually-linked
>    PRs, ignores mere mentions, and collapses today's N+1 `gh issue view` calls per
>    blocker to ≤2 API calls per `_ready` run.
> 3. **Goal scoping split out**: #649 is backfilled (6 children); #650/#654/#655 still have
>    **zero children** (re-verified 2026-07-05) because the children need *filing*, not
>    linking — that's per-goal product scoping owned by the goal owners, tracked on those
>    Goal issues, no longer on H2's critical path.
> 4. **Decisions, not options**: `auto-promote.yml` is deleted **without replacement**
>    (native UI shows unblocked; H1's dispatch loop computes readiness on demand). The
>    `autonomous-sprint` launcher also drops its `HAS_STRUCTURE` body heuristic and
>    milestone fallback — after the migration, `_ready` is always authoritative.
> 5. **Offline tests required** (H1 precedent): fixture-backed predicate tests wired into
>    `orchestration-tests.yml`; enumerated doc touchpoints (CLAUDE.md sprint/work-tracking
>    sections, orchestration README, guide's transition note) are in scope, and remaining
>    Milestones are closed — finally exiting the #656 dual-system transition.

- **Beads/Gas Town**: beads remains a *projection* for `gt` (`beads-sync.sh` reads edges from
  the API instead of body text — smaller script, same behavior). If H4 retires Gas Town,
  beads and both sync scripts retire with it; until then it stays read-side only.

### H3 — Platform merge path (~1 day + repo settings)

> **Partial execution (2026-07-04, PR #684):** the review-half of the merge path
> landed, generalized per the owner's direction — **reviewer-agnostic**, gating on
> "feedback addressed" rather than on any particular reviewer. Pieces:
> `.github/workflows/review-gate.yml` (red while any review thread is unresolved or a
> standing changes-requested exists; remediation in the failure log per §7 R1),
> `required_conversation_resolution` seeded as the `branches:` block in
> `.github/settings.yml` (that block now owns branch protection — extend it, don't use
> the UI), the PR lifecycle codified in CONTRIBUTING.md §Pull Request Process + a PR
> template checkbox, and post-review conduct added to the H1 dispatch prompts.
> **Second tranche (2026-07-04, same PR): §8 Q2 DECIDED by owner — "gate + queue
> suffices for routine green PRs."** Landed: required-check set in `settings.yml`
> (PR title check, Review gate, schema, rust, go, typescript, hash-parity; linear
> history; `allow_auto_merge`), `automerge.yml` (arms squash auto-merge on ready
> non-risk PRs; refuses `breaking`/`contract-test`/`needs-human-input`/proto-touching
> and requests a human instead), `agents/merge-queue.md` rewritten as the PR-shepherd
> role (never merges directly), CONTRIBUTING §7 graduated review. Remaining for #681:
> confirm the Settings app is installed so the ruleset syncs, and evaluate a native
> merge queue (merge-group testing) once ruleset management is confirmed — auto-merge
> delivers merge-when-green today.
> **Third tranche (2026-07-05): PR-size gate.** The #684 omnibus (20 files, ~1,300
> lines, three tranches) drew three real review findings; its focused follow-ups
> (#689/#690) reviewed clean. Codified per §7 R1 as a check, not a prompt rule:
> `_pr-size.yml` (reusable, fleet-callable) + `pr-size.yml` caller — soft 400
> lines/10 files (warn), hard 900/25 (fail) with lockfiles/generated/markdown
> exempt and an auditable `oversize-approved` label override; the dispatch prompt
> tells workers to slice-and-propose rather than ship omnibuses. Context
> "PR size / check" becomes a required ruleset context in a follow-up PR once
> verified reporting live (verify-then-require, as with the H6 contexts).

- Extend `.github/settings.yml` (settings-as-code is already established by #670) with the
  ruleset: required status checks = `PR title check`, `rust`, `go`, `ts`, `schema`,
  `hash-parity` (branch-naming stays advisory, per CLAUDE.md's stated intent), linear
  history, and **merge queue enabled** on `main`.
- Green agent PRs then merge via the queue. `agents/merge-queue.md` shrinks from "you are the
  ratchet" to a **PR shepherd**: label/triage, requeue on flake, escalate `needs-human-input`
  — judgment work, which is what an agent is actually for. `require_human_review: true`
  stays for `breaking`, `contract-test`, and `proto`-touching PRs via CODEOWNERS or a
  path-filtered required review; routine green PRs stop queuing on a human.
- CI-cost guard: the queue only runs the 620-line `ci.yml` once per merge group — cheaper
  than today's re-run-per-PR-update pattern, and it structurally prevents the
  "merged-with-red-history" incident recorded for #661.

### H4 — Executor consolidation pilot (1 sprint, with kill criteria)

> **Amended 2026-07-05 (owner decision): the multiclaude question is settled as an
> absorption already in progress — finish it, don't vendor it.** Prompted by the owner
> asking whether to incorporate `dlorenc/multiclaude` directly into the harness. The
> audit answer: the harness has already absorbed multiclaude's *coordination plane*,
> capability by capability — task queue/state → Issues + native dependency edges +
> GraphQL `_ready` (H2); assignment → the claim protocol (H1); merge queue + CI gating →
> ruleset required checks + `automerge.yml` + review/size gates (H3/H6); agent personas →
> the OKF registry (canonical; `.multiclaude/agents/*` carry banners pending #682's
> generator); worktree isolation → first-party Claude Code. What remains uniquely
> multiclaude is the **local supervision daemon**: overnight parallel workers on the
> owner's machine, with stall-detection and restart. Decision, honoring §5's
> deprecated-by-evidence-not-by-proposal rule:
>
> - **Do not vendor the daemon.** Process supervision is the fiddly 20%; vendoring means
>   owning its bugs while upstream keeps moving. It also points the wrong way — every
>   recent harness move pushed execution *off* the local machine (`claude.yml`, the
>   workflow-vehicle pattern, remote sessions, `ready-drift.yml`).
> - **Build the replacement as `.github/workflows/evening-dispatcher.yml`**: a scheduled
>   workflow that reads `_ready` for the target cohort, posts claims, and launches one
>   worker per issue. **Claim expiry is the self-healing loop** — a dead worker's claim
>   lapses and the next tick re-dispatches with R2 resume semantics. No supervisor
>   process anywhere; the schedule survives the owner's laptop being off (the
>   session-mortality boundary dissolves).
> - **Multiclaude keeps `just evening` until the dispatcher wins on evidence** — the
>   pilot below stops being a one-shot comparison and becomes the graduated cutover.
>
> **Probes before build** (locked-plan v2 rule; two documented-but-wrong platform
> assumptions burned this repo on 2026-07-04):
>
> 1. **Worker-launch path.** Documented constraint to design around: events created with
>    the default `GITHUB_TOKEN` do not trigger other workflows — a dispatcher comment
>    saying "@claude …" will NOT wake `claude.yml`. Probe, in preference order: invoke
>    `claude-code-action` directly as a dispatcher job step with a `prompt` input;
>    `workflow_dispatch` a worker workflow per issue; app/PAT token for trigger comments
>    (H5 credential territory — least preferred).
> 2. **Wall-clock and parallelism.** One right-sized issue (PR-size-gate sized) per
>    worker inside the 6-hour hosted-job ceiling; matrix fan-out for N workers; public
>    repo → Actions minutes are free, so marginal cost is API spend only.
> 3. Claim writes from Actions `GITHUB_TOKEN` are already proven (the H2 workflow
>    vehicles) — no probe needed.
>
> **Cutover evidence (graduated — same doctrine as H2's drift window):**
>
> - **Shadow**: ≥3 nights of dry-run — the dispatcher logs what it *would* claim and
>   dispatch; compare against what multiclaude actually picked up, diagnosing
>   disagreements the way `READY_DRIFT` mismatches are diagnosed on #680.
> - **Limited live**: the dispatcher owns one cohort for ~a week, multiclaude disabled
>   for that cohort. Metrics unchanged from #682: duplicate-PR rate **0** (claims),
>   acceptance **≥ multiclaude baseline**, review burden and cost **≤ current**.
> - **Full**: one complete overnight sprint driven by the dispatcher with multiclaude
>   never invoked — the acceptance test for retirement.
> - **Retire**: `just evening` retargets to arm/inspect the dispatcher; `.multiclaude/`
>   shrinks to an archival tombstone plus a how-to-resurrect note; #682's generator
>   drops the multiclaude view target; beads and Gas Town stay governed by §8 Q4 —
>   this amendment settles the multiclaude/overnight half only.
> - **Kill** (unchanged in spirit): any metric regresses → multiclaude remains the
>   overnight default and the dispatcher stays a secondary adapter. H1's
>   executor-agnosticism means nothing is lost either way.
>
> **Pre-step, needs access beyond a repo-scoped session**: a review of upstream
> `dlorenc/multiclaude` as it exists today — license check plus a short
> worth-lifting-narrowly-with-attribution list (candidates: stall-detection heuristics,
> worker prompt scaffolding). Upstream may itself have grown GitHub-native features
> since our integration was built; the review should say so either way.
>
> Two knock-ons for #682's child list: the deepagents 10-primitive audit now targets
> the **dispatcher + worker preamble** (what must the replacement cover — planning,
> context offload, HITL) rather than a tool being retired; and the Success bullet
> below reads "becomes optional" — sharpened here to the Retire checklist above.

Run this like ADR-031 — a bounded pilot with explicit success/kill criteria, not a rewrite:

- **Hypothesis**: Claude Code covers three of the five executor roles with zero bespoke
  daemon: interactive (CLI/desktop with native subagents and teams — `.claude/settings.json`
  already sets `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`), autonomous overnight (remote/web
  sessions or scheduled `claude-code-action` runs dispatched per ready issue via the H1
  adapter), and drive-by (`@claude` mentions + `claude-code-review.yml`, both landed by #632).
- **Pilot**: one sprint, dispatch ~10 issues through `executor=claude-web` alongside
  multiclaude. Compare: PR acceptance rate, duplicate rate (should be zero under H1 claims),
  wall-clock, review burden, cost.
- **Success** → multiclaude's daemon/tmux/self-healing surface (and possibly Gas Town + beads)
  becomes optional; the harness keeps Jules for scheduled maintenance (it's two small
  workflow files and free-tier) and Gemini for second opinions (one recipe).
- **Kill** → keep multiclaude as the default `executor=` and lose nothing — H1 made it one
  adapter among several, which is the point.

Either way, agent identity consolidates now (it's executor-independent): **one registry**,
`agents/registry/agent-3.md` with YAML frontmatter (`owner`, `module`, `label`, `paths`,
`port`, `crates`) + prose standards. `.multiclaude/agents/*` and
`docs/coordination/prompts/*` become generated views or thin includes; onboarding docs link
instead of restating. The near-verbatim Work Tracking block ×12 collapses to a template
parameterized by `label`.

### H6 — Ecosystem governance: one harness, many repos (added 2026-07-04)

Context that arrived after H3 shipped: this repo is one of a **fleet** (owner listing,
2026-07-04 — kaizen-experimentation, kaizen-recsys ≈ the ADR-029/RFC-001 personalization
service, kaizen-pipelines, kaizen-rosetta, kaizen-os, kaizen-alchemy, kaizen-accelerator,
kensho-repl), and the owner holds a GitHub **organization** (`wunderkind-ventures`) the
fleet could transfer to. Two structural facts drive the design:

- **Under a user account** (today): no org rulesets, no safe-settings layering, no org-level
  app installs — every native multi-repo governance mechanism is org-gated. What DOES work:
  reusable workflows (public repo → callable fleet-wide) and per-repo rulesets stamped by
  automation.
- **The Probot settings.yml `branches:` block never synced** (app never installed; main was
  live-verified unprotected). Retire it for protection rather than install more third-party
  machinery: native rulesets need no app, are import/exportable JSON, and transfer with a
  repo.

The move, three layers (all landed with this section):

1. **Workflow logic defined once** — `_review-gate.yml`, `_pr-title.yml`, `_automerge.yml`
   are `workflow_call` reusables; this repo and every sibling run ~20-line callers. The
   review-thread-trigger bug (§7 note) gets fixed once, not N times. Check contexts become
   two-segment (`Review gate / gate`, `PR title check / check`).
2. **Protection as data** — `.github/rulesets/main.json` (this repo, one import/API call)
   and `infra/github-governance/` (Pulumi + Go, same stack family as `infra/`): config-driven
   repo list, owner-keyed so a repo migrates user→org by config edit; siblings default
   `enforcement: disabled` until their callers exist (a required check that never reports
   blocks every merge). Auth = fine-grained PAT, Administration:write only — H5's first
   concrete credential.
3. **Org mode encoded, not aspirational** — `orgMode: true` collapses the universal rules
   into ONE organization ruleset targeting `kaizen-*`/`kensho-*` (default enforcement
   `disabled` → `active`; the `evaluate` dry-run status is **Enterprise-only**, so the stack
   never defaults to it — verified 2026-07-04 against github/docs); per-repo rulesets keep
   only repo-specific CI contexts (rulesets aggregate). Migration sequencing, per-repo
   re-pointing checklist (app installs, per-owner PATs, owner-qualified `uses:` refs), and
   the plan gate (org rulesets need **Team+**; everything the harness requires fits Team)
   live in `docs/runbooks/ecosystem-governance.md`.

**Decision needed (owner)**: transfer timing. Recommended order: land governance-as-code
(done) → transfer one low-traffic repo (kensho-repl) as the canary → rest of the fleet →
kaizen-experimentation last (most automation pinned to its identity) → flip `orgMode` once
the org plan supports it.

### H7 — Codify delivery practices: requirements → design → spec → plan (added 2026-07-05)

The knowledge-layer counterpart to H1–H6: requirements gathering, planning, UX design,
architecture, and spec writing are already *practiced* well here — the move is to finish
codifying them with the same mechanism that carried the rest of the harness:
**artifact conventions + skill carriers + advisory lints promoted to required checks once
proven**. Tracking: #699 (four right-sized PRs — this phase practices what it codifies).

Inventory (2026-07-05) — codified in pockets, uneven across the lifecycle:

- **Planning is the strongest pocket and proves the enforcement model**:
  `docs/superpowers/{specs,plans}/` + `locked-plan-template.md` (whose Cross-phase
  artifacts table encodes the ADR-026 P3 orphaned-RPC incident) + `just prime-issue`,
  which *refuses to dispatch an issue that has no plan* and stamps the plan's path/SHA
  into the issue body. An issue without a plan cannot enter the harness — that ratchet
  is the pattern everything else extends.
- **Requirements**: `to-prd`/`grill-me`/`grill-with-docs` skills exist; Goals carry the
  one-metric rule — but PRDs have no stored-artifact convention (requirements evaporate
  into issue bodies).
- **Architecture**: ADRs 001–030 with strong discipline; the RFC precedent (#543–545,
  cross-system boundaries) is uncodified — when-RFC-vs-ADR is unwritten.
- **Specs**: the **Lock** convention (a decision with the burden of justification on the
  challenger) is practiced but unnamed; entry/exit criteria undocumented.
- **UX is the weakest link**: Palette standardizes execution polish, but there is no
  UX-spec stage — UI decisions get made ad-hoc inside plans (the CodeMirror Lock).

The moves (per-PR checklist on #699):

1. **One lifecycle map** — `docs/guides/delivery-lifecycle.md`: idea → PRD → RFC (iff
   the boundary crosses repos/teams) / ADR (iff a decision needs a permanent record) →
   spec (Locks) → locked plan (**plan-review** before blessing — the #680 v1→v2
   exercise, codified) → `prime-issue` → dispatch. Every stage names its artifact,
   carrier (skill/template), and check.
2. **Templates as code** — `docs/templates/` (PRD, RFC, ux-spec) with OKF frontmatter
   (`type`/status/links — the registry pattern's second consumer, composing with #683);
   `locked-plan-template.md` upgraded to this session's plan-quality bar (probe-gated
   platform assumptions, decisions-not-options, executor constraints, phases sized to
   the PR gate, graduated cutover).
3. **Checks, not exhortations** (§7 R1) — `scripts/check_docs.py` extending the
   `check_okf.py` pattern: ADR/spec/plan required sections, Lock format, Cross-phase
   table when multi-phase, one metric per Goal. Advisory first; promotion to required
   only after a clean window (verify-then-require, proven three times in H3/H6).

---

## 5. What NOT to change

- **GitHub Issues as source of truth** and `Closes #N` auto-close — this survived two
  migrations because it's correct.
- **PR-metadata attribution** (title lint + label inheritance) — recently landed, working,
  and it's what makes harness-generated branch names tolerable.
- **Advisory branch naming** — enforcement stays on the PR side, per CLAUDE.md.
- **`skills-lock.json`** and the cross-tool skills approach — this is the pattern H0 extends
  to the vendored agent library.
- **CLAUDE.md as the context anchor** — every tool reads it; keep it the front door.
- **The five-tool portfolio, until H4's data says otherwise.** Consolidation is a pilot
  outcome, not a premise. Multiclaude and Gas Town are carrying real work today; they get
  deprecated by evidence, not by proposal.

## 6. Sequencing

| Phase | Contents | Depends on | Suggested tracking |
| --- | --- | --- | --- |
| H0 | #632 decision · pin refresh · placeholder fixes · doc archival · unvendor library · worker permissions | — | 1 issue, `chore` |
| H1 | claim protocol · `scripts/orchestration/` · `work-on`/`sprint` façade | — | 1 issue (absorbs #521; relates #522) |
| H2 | probe-gated native work graph: dependency edges · GraphQL `_ready` (graduated cutover + drift window) · Iteration-based sprint reads · then delete parsers/auto-promote · slim beads-sync | H1 (claims — satisfied 2026-07-04, #684) | #680 (plan v2, 2026-07-05; Goal-child *scoping* for #650/#654/#655 split out to the Goal issues) |
| H3 | ruleset + merge queue · shepherd role · graduated human review | H0 (#632 for review signal) | 1 issue, `chore` + settings change |
| H4 | Claude-executor pilot + agent registry (registry seeded via #677; *amended 2026-07-05*: evening-dispatcher design + graduated multiclaude retirement) | H1, H3 | #682 (Goal; metric: duplicate rate 0, acceptance ≥ multiclaude baseline, cost ≤ current) |
| H5 | Least-privilege worker credentials + dispatch instrumentation (see §7 R4) | H1 | 1 issue, `chore` — replaces the deferred `--dangerously-skip-permissions` item |
| H6 | Ecosystem governance: reusable workflows · ruleset JSON · Pulumi fleet stamping · org-migration path | H3 (gate + graduated review are what gets fleet-ified) | 1 issue, `chore` + owner decision on wunderkind-ventures transfer timing |
| H7 | Delivery-practice codification: lifecycle map · templates (PRD/RFC/ux-spec, OKF frontmatter) · locked-plan v2 + plan-review · advisory doc-lints | — (extends the `prime-issue` ratchet; composes with #683) | #699, four right-sized PRs |

Each phase is independently shippable and independently revertible; H1+H2 delete more code
than they add.

## 7. Prior art & revisions (2026-07-02)

Survey of [best-of-Agent-Harnesses](https://github.com/RyanAlberts/best-of-Agent-Harnesses)
and its highest-signal links — Anthropic's ["Effective harnesses for long-running
agents"](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents),
OpenAI's ["Harness engineering"](https://openai.com/index/harness-engineering/) (7 engineers,
~1M LOC, ~1,500 agent PRs in 5 months), the [agents.md](https://agents.md) format,
[obra/superpowers](https://github.com/obra/superpowers), and
[deepagents](https://github.com/langchain-ai/deepagents). Five lessons change this proposal;
the rest confirm it.

**R1 — Promote rules from prompts into checks whose error messages carry the fix** (OpenAI).
Their operating rule: when an agent fails, never "try harder" — ask "what capability is
missing, and how do we make it both legible and enforceable," then commit the fix to the
repo. Custom lints inject remediation instructions into agent context via the error message;
"when documentation falls short, we promote the rule into code." *Amends H3:* required
checks are the enforcement substrate for every promotable rule currently living in the ×12
agent prompt files (our `assert_finite!`, golden-file, and title lints are embryonic
versions). This is the general cure for prompt-as-infrastructure.

**R2 — Stop double-work, not just double-dispatch** (Anthropic). Their long-running harness
splits a one-time *initializer* dispatch (environment, feature list, progress file, first
commit) from repeatable *coding* dispatches, each with a mandatory startup ritual: read git
log + progress artifacts → verify baseline green with a smoke test → take exactly one unit
of work → leave a merge-ready clean state with descriptive commits. Task state is structured
JSON that workers may only flip status on ("it is unacceptable to remove or edit tests"),
never rewrite. *Amends H1/H2:* the claim protocol stops duplicate dispatch, but only
externalized, restricted-mutation progress state gives a re-dispatched worker resume
semantics — the actual root fix for our #661/#663-style duplicates. Dispatch adapters gain
an init/resume distinction and a standard startup preamble.

**R3 — Registry as a small table of contents, mechanically kept fresh** (OpenAI +
agents.md + superpowers). OpenAI's "one big AGENTS.md" failed for stated reasons (context
scarcity, everything-important-is-nothing, instant rot, unverifiable blob); the fix is a
~100-line entry point with progressive disclosure into `docs/`, **doc-lints in CI** that
validate freshness/cross-links/structure, and a **recurring doc-gardening agent** that opens
fix-up PRs. *Amends H4:* the agent registry = per-agent frontmatter files (machine identity:
id, module, `owned_paths`, label, port, obligations) + prose charter; `just gen-agents`
renders generated views — module-scoped `AGENTS.md` in each owned directory (nearest-file-wins;
honored natively by Jules, Devin, Codex, Cursor, Copilot), `.multiclaude/agents/*`, onboarding,
prompts, and the CLAUDE.md architecture table — with a CI drift-check that re-runs the
generator and fails on dirty diff. Registry = proto, views = generated stubs: our own
schema-first discipline applied to agent config. (agents.md itself is a *view* format, not a
registry — no identity fields; superpowers proves the one-canonical-source + thin-adapter
portability architecture across 10 harnesses.)

**R4 — Least-privilege progressive disclosure, not `--dangerously-skip-permissions`**
(consensus across sources; "least privilege by default… expand as tasks require"). None of
H1–H4 owned the permission model — our scariest standing risk. *New phase H5 (proposed):*
scoped worker credentials (branch-limited push tokens, per-executor allowlists mirroring
`.claude/settings.json`'s allow/deny/ask tiers), approval gates only at irreversible
boundaries, and instrumentation of every dispatch (tool calls, errors, interventions,
timeouts). Replaces the deferred H0 worker-permissions item with a real design. Tracking
issue to be filed if accepted.

**R5 — Size the merge path for agent throughput; add self-auditing GC** (OpenAI). Their
merge philosophy: minimal blocking gates, short-lived PRs, flakes get follow-up runs rather
than blocking — "corrections are cheap, waiting is expensive." And their entropy control:
golden principles encoded in-repo, background agents that scan for deviations, update a
graded `QUALITY_SCORE.md`, and open under-a-minute automergeable refactor PRs. *Amends H3:*
keep the required-check set small and fast with an explicit flake policy, or the queue
becomes the bottleneck agents route around. *Amends H4/Jules lane:* schedule doc-gardening +
quality-grading agents — our Palette stream is the manual precursor; this also finally gives
the harness the self-auditing loop P6 asked for.

**R6 — Adopt Google's Open Knowledge Format (OKF v0.1) as the registry's source
conventions** *(ADOPTED 2026-07-04 — owner decision on PR #677. The registry bundle is
seeded at `docs/agents/registry/` — 12 concepts, `index.md`, `log.md` — with
`scripts/check_okf.py` + `just check-registry` + an advisory conformance workflow; the
`.multiclaude/agents/` copies carry canonical-source banners until #682's generator
replaces them.)* ([announcement](https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing),
[spec](https://github.com/GoogleCloudPlatform/knowledge-catalog/tree/main/okf), published
2026-06-12). OKF standardizes exactly the shape R3 converged on independently: knowledge as
a directory of markdown files with YAML frontmatter, file path = concept identity, markdown
links form the graph, reserved `index.md` (progressive-disclosure table of contents — R3's
"~100-line entry point", now a standard) and `log.md` (ISO-dated newest-first history —
a standard format for R2's progress artifacts). Only `type` is required; consumers "SHOULD
NOT reject documents with unrecognized fields", so our machine-identity keys (`owned_paths`,
`label`, `port`, `crates`, obligations) ride as conformant extensions. Conformance is three
lintable rules — the `gen-agents` CI drift-check doubles as an OKF conformance check for
free. Bonus: Google ships a self-contained static HTML visualizer (registry → interactive
ownership graph) and their enrichment-agent reference implementation is R3's doc-gardening
agent by another name. Risk: v0.1, weeks old, no third-party adoption yet — acceptable
because the format degrades to plain markdown we would have written anyway; lock-in ≈ 0.
*Amends H4 (registry = an OKF bundle) and R2 (task progress files use `log.md` semantics).*
**Separate product-scale opportunity (HITL, new-ADR-sized, not part of this harness work):**
OKF's headline use case is "your business' meaning of a metric" — ADR-026's
MetricDefinitions plus the M3 `@metric_ref` dependency edges are already a knowledge graph;
an M5 export job rendering each metric as an OKF concept (type, M6 `resource` link, MetricQL
body, lineage links) would make the Kaizen metric catalog consumable by customers' agents
and catalogs (incl. BigQuery Knowledge Catalog, which ingests OKF). Impact M5/M3/M6.

Confirmations worth noting: externalized state in git/GitHub artifacts over session memory
("anything the agent can't access in-context effectively doesn't exist" — validates
Issues-as-spec and requires Gas Town verbal steering be written back to the issue);
executor-agnostic adapters (Manus rewrote its harness 5× in 6 months — don't couple the
queue to any executor); one-issue-one-claim-one-PR granularity; piloting one reference
executor before multiplying roles; tool *subtraction* (Vercel cut 80% of tools, results
improved) as an H4 pilot variable; and the deepagents 10-primitive checklist (planning,
isolated-context subagents, filesystem, context offload, shell sandbox, memory, HITL,
skills, tools, checkpointing) as the rubric for auditing multiclaude's worker loop — which
currently lacks planning, context offload, and HITL. Skipped deliberately: claude-mem-style
local session-memory services (stateful per-machine infra doesn't fit an ephemeral
multi-executor fleet; Issues/PRs/ADRs are our memory) and adopting any framework wholesale.

## 8. Open questions (HITL)

1. **#632** — merge as-is, or gate `claude-code-review.yml` to `ready_for_review` events
   first? (Recommended: merge, then tune triggers.)
2. **Merge queue on a solo-maintainer repo** — comfortable letting green non-`breaking`
   agent PRs merge without human review, or keep `require_human_review` global and use the
   queue purely as the red-history guard?
3. **Claim bot identity** — claims via default `GITHUB_TOKEN`/`gh` as the repo owner, or a
   dedicated machine account so human vs. harness actions stay distinguishable in the audit
   trail?
4. **Gas Town's future** — if H4 succeeds, does interactive steering move to Claude Code
   teams, or is the Mayor/polecat model worth keeping for its tmux-native visibility?
   *(Narrowed 2026-07-05: the H4 amendment settles the multiclaude/overnight half —
   evidence-gated retirement behind the evening dispatcher. This question now covers
   interactive steering only.)*
5. **Devin** — `.devin/skills/` and comment-citations show it's used for review/coverage;
   should it become an H1 adapter, or remain manual-dispatch outside the harness?
