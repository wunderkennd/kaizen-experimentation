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
| H4 | Executor consolidation pilot: Claude Code (remote/scheduled/`@claude`) as the reference executor, with ADR-031-style kill criteria | 5-tool maintenance surface, local-daemon dependence | 1 sprint, evaluated |

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

### H2 — GitHub-native work graph (~1–2 days, then delete code)

Adopt **native issue dependencies** (blocked-by/blocking) and **sub-issues** as the only DAG.

This finishes what #656 started. That PR migrated the *reporting* plane (Project #5,
Iteration field, Goal issues, three views) but deliberately left the *automation* plane on
labels/milestones — `projects-and-goals.md` says so explicitly: "the Owner/Iteration fields
are for humans and the Roadmap; the labels are for machines, **until the orchestration layer
speaks GraphQL**." H2 is that "until." Live state as of 2026-07-02: Goals #647 and #648 have
proper sub-issue trees (5 children each; #648 shows 1/5 done), but **#649, #650, #654, #655
have zero children linked** despite child issues existing (#599–#602, #554, #558 for #649
alone), and `just morning` still reads Milestones over REST (`justfile:758`) — the
"transition sprint with both systems live" was never exited.

- Backfill the Goal↔sub-issue linkage for #649/#650/#654/#655 (scripted, minutes — the
  same `sub_issue` API `seed-goals.sh` already uses).
- Migrate `## Blocked by` sections to real dependency edges (one-time script; keep the body
  section as human-readable narrative if desired, but tooling stops parsing it).
- `_ready` becomes a single GraphQL query: open, unclaimed, no open closing PR, zero open
  blocking issues. Delete the awk parser and the `IN_FLIGHT` grep.
- Swap `just morning`'s milestone read for the Project Iteration (GraphQL) with the
  `sprint-N`-label fallback the migration guide already specifies, then close the remaining
  Milestones — exiting the dual-system transition state.
- Delete `auto-promote.yml` — GitHub surfaces "unblocked" natively in issue timelines and
  Project views; if the dispatch-nudge comment is still wanted, one generic workflow on
  `issues:closed` can query dependents of *any* label.
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
> Remaining for #681: required-status-check selection, merge queue, and the graduated
> human-review decision (§8 Q2).

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
| H2 | native dependencies · GraphQL `_ready` · delete parsers/auto-promote · slim beads-sync | H1 (claims) | 1 issue (absorbs the Goal-linkage P0 from status 2026-07-02) |
| H3 | ruleset + merge queue · shepherd role · graduated human review | H0 (#632 for review signal) | 1 issue, `chore` + settings change |
| H4 | Claude-executor pilot + agent registry | H1, H3 | 1 Goal (it has a metric: duplicate rate 0, acceptance ≥ multiclaude baseline, cost ≤ current) |
| H5 | Least-privilege worker credentials + dispatch instrumentation (see §7 R4) | H1 | 1 issue, `chore` — replaces the deferred `--dangerously-skip-permissions` item |

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
5. **Devin** — `.devin/skills/` and comment-citations show it's used for review/coverage;
   should it become an H1 adapter, or remain manual-dispatch outside the harness?
