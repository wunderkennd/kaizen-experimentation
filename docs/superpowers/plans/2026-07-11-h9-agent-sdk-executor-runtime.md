# H9 — Formalize the executor runtime on the Claude Agent SDK (issue: to file)

**Status:** Design lock — RFC for review (v1 draft).
**Plan-review:** pending — not yet run. Next step per `docs/guides/delivery-lifecycle.md`: plan-review v1→v2, review note on the H9 tracking issue, then `just prime-issue`. Do NOT dispatch from this document before that.
**Issue:** to be filed — H9 tracking issue (sibling of #682/#720, proposal §6 row H9) on plan acceptance; this line then gains the `Issue: #N` reference `prime-issue` greps for.
**Blocked by:** — none hard. Phase E (pilot) composes with H4 Phase B but does not gate on it.

---

## Summary

The harness has consolidated onto GitHub-native primitives (H1–H3, H6) with Claude Code
as the reference executor (H4). But the Claude lanes themselves still run on a
**prompt-string boundary**: `dispatch.sh` renders prose, `claude-code-action@v1`
executes it verbatim, and every law the session must honor — claim discipline, branch
rules, PR metadata, the size gate, "one unit of work" — rides as exhortation inside
that prose. That is the prompt-as-infrastructure pattern §3.5 of the proposal retired
everywhere else, still live at the execution plane. Grep confirms the gap: **nothing in
this repo references the Claude Agent SDK today** — the programmatic engine under
Claude Code (typed options, lifecycle hooks that can deny tool calls with remediation,
programmatic agent definitions, budget caps, schema-validated structured outputs,
session resume) is entirely unused.

The evidence that this is the next bottleneck is already on Goal #682. The first pilot
batch (2026-07-06, four issues) found: *"workers deliver branches + reports but the App
cannot open PRs, push `.github/workflows/**`, or remove the `claimed` label, and the
runner allowlist blocked `just test-infra`"* — and the comparison scorecard is being
hand-collected because workers emit no machine-readable outcome. Meanwhile the evening
dispatcher has six consecutive successful nightly shadow runs (2026-07-06 → 2026-07-11),
so H4 Phase B (live dispatch) is about to put real traffic on this boundary.

**H9 in one sentence:** build a typed harness runtime (`harness/`, TypeScript, pinned
`@anthropic-ai/claude-agent-sdk`) that the Claude worker lane invokes instead of a bare
`prompt:` input — it assembles session options from the OKF registry (single source,
read live), enforces harness law in-session via SDK hooks (R1: deny with the fix in the
error message), caps spend per dispatch (`maxBudgetUsd`), and ends every run with a
versioned structured outcome record — which is exactly the per-lane instrumentation
H8 Phase 0 (#720) and the #682 scorecard need.

**Composes, never replaces** (the H4-L6 doctrine): claims, readiness, and dispatch stay
`scripts/orchestration/` bash + `gh`. The runtime is the execution plane only — it slots
in as one new adapter (`dispatch.d/claude-sdk.sh`) plus one worker workflow honoring the
existing normative contracts (`{issue, prompt}` inputs, prompt on stdin / issue as `$1`,
60k-char guard, registered dormant on `main`), shadow-piloted against the bare-action
lane and graduated on the #682 scorecard.

### Non-goals (v1 of H9)

- **No coordination-plane rewrite.** `claims.sh`, `ready.sh`, `dispatch.sh`,
  `evening_dispatch.sh` are proven and stay bash; the runtime never computes readiness
  or takes claims (the dispatcher claimed before the adapter ran).
- **No migration of `claude.yml` (@claude) or `claude-code-review.yml`.** The
  interactive and review lanes stay on `claude-code-action@v1` as-is; only the
  *dispatched worker* lane gets the runtime. Cutover of other lanes is a post-scorecard
  follow-up, not this plan.
- **No multiclaude changes.** Retirement stays governed by H4 Phase B/C evidence (#682).
- **No routing optimizer.** H8 Phase 1+ (`routing.yml`, adaptive selection) stays
  design-stage; H9 ships only the outcome *record* that makes H8's evidence exist.
- **No Agent Teams / Managed Agents adoption.** Teams is interactive-steering territory
  (proposal §8 Q4; `docs/design/agent-teams-vs-multiclaude-evaluation.md`); Managed
  Agents is Anthropic's hosted product — the harness stays self-hosted on Actions.
- **No product code, no proto, no stats.** Harness tooling only.
- **Not the full H5.** H9 designs the Claude-lane worker credential and permission
  profile (its first concrete instance); fleet-wide credential design remains H5.

---

## Platform assumptions & probes

| # | Assumption | Exercised here before? | Probe (task + command) | Verdict |
|---|---|---|---|---|
| PA1 | The Agent SDK runs headless on an ubuntu runner authenticated by `CLAUDE_CODE_OAUTH_TOKEN` (the secret `claude.yml`/`claude-worker.yml` already use) | no — the action wraps the CLI; the SDK has never run here | A1: workflow-vehicle PR runs `node probe.mjs` with the secret; asserts a session completes | **open — Phase A gates everything** |
| PA2 | `PreToolUse` hooks can deny a tool call and surface a remediation message; `settingSources: ["project"]` loads `.claude/settings.json` permissions | no (documented: code.claude.com/docs/en/agent-sdk/hooks.md) | A1: probe asserts a scripted deny fires and a settings.json deny rule is honored | **open** |
| PA3 | Result messages carry `total_cost_usd` + `usage`; `structuredOutput` (JSON Schema) returns validated JSON; `maxBudgetUsd` halts a runaway session | no (documented: …/agent-sdk/structured-outputs) | A1: probe asserts all three on a trivial task | **open** |
| PA4 | `workflow_dispatch` worker with `{issue, prompt}` inputs launches via default `GITHUB_TOKEN` + `actions: write`, and must be registered on `main` first | yes — probe #713, `claude-worker.yml` (H4 Phase A) | — | **confirmed** |
| PA5 | The claude-code-action GitHub App token cannot open PRs, push `.github/workflows/**`, or remove the `claimed` label | yes — #682 pilot batch findings, 2026-07-06 | — | **confirmed (design constraint D2 below)** |
| PA6 | Events created with the default `GITHUB_TOKEN` do not trigger other workflows — a PR opened with it would never run the required checks (PR title / Review gate / PR size / CI), making it unmergeable | yes — probe #713 leg 3 established the class; documented platform behavior | — | **confirmed (why L4 uses a PAT, not `GITHUB_TOKEN`)** |
| PA7 | A fine-grained PAT scoped to this repo (Contents + Pull requests + Issues write, Workflows withheld) can push branches, open PRs that DO trigger required checks, and remove labels | no at PR-open granularity — precedent exists (H6 governance stack runs on a fine-grained PAT, Administration:write) | A2: probe leg with a throwaway branch/PR/label round-trip, then close + delete | **open** |
| PA8 | Adapter prompt ≤ 60,000 chars fits `workflow_dispatch` payload | guarded by H4 L8 in `claude-workflow.sh`; same guard reused | — | **confirmed (inherited)** |

Doctrine reminder (plan quality bar #1): PA1–PA3 and PA7 are exactly the
documented-but-unexercised class that burned this repo twice on 2026-07-04. Phase A
runs them before any runtime code is written; a decision matrix lands in the probe
report comment.

---

## Locks — binding for implementers

| # | Lock | One-line answer | Decided (owner, date) |
|---|---|---|---|
| L1 | Scope boundary | The runtime formalizes the **execution plane only**; claims/ready/dispatch stay `scripts/orchestration/` bash — the runtime composes, never reimplements (mirrors H4 L6) | claude session, 2026-07-11 (v1 draft; ratify at plan-review) |
| L2 | Language + home | **TypeScript**, new top-level package `harness/` (Node ≥ 18, layout precedent: `infra/` is a self-contained Go module). Chosen over Python SDK because the policy layer is the point: TS carries the full documented hook surface (`SessionStart`/`SessionEnd`, `canUseTool`, `PostToolBatch` are TS-only) and it is `claude-code-action`'s own ecosystem. Burden of justification to switch: show the Python SDK covering the L6/L7 hook set. Google ADK (Go) was evaluated 2026-07-12 and rejected *for this lane* — see "Alternatives considered" below; its crossover trigger is recorded there | claude session, 2026-07-11 — **requires owner ratification**: ships with a one-line CLAUDE.md scope note ("TypeScript is UI only" governs product modules; `harness/` is harness tooling, still zero statistical computation) |
| L3 | Dependency pinning | `@anthropic-ai/claude-agent-sdk` pinned **exact** (0.3.x current as of 2026-07); bumps ride the Jules weekly-maintenance lane like other pins; `package-lock.json` committed (lockfiles are PR-size-exempt) | claude session, 2026-07-11 |
| L4 | Credentials | Anthropic auth = existing `CLAUDE_CODE_OAUTH_TOKEN` secret (no new vendor credential; Agent SDK subscription credits apply). GitHub writes = **new fine-grained PAT `KAIZEN_WORKER_TOKEN`** — this repo only; Contents, Pull requests, Issues: write; **Workflows, Administration: withheld** — because the App token can't complete the worker contract (PA5) and `GITHUB_TOKEN` PRs can't trigger the merge gates (PA6). This is H5's first designed worker credential (precedent: H6 governance PAT) | claude session, 2026-07-11 — **owner action + HITL**: owner mints the PAT at Phase D; kill = stay on App token and the lane keeps hand-opened PRs |
| L5 | Session config posture | `settingSources: ["project"]` (inherit `.claude/settings.json` allow/deny/ask tiers + pinned skills) + `permissionMode: "dontAsk"` + explicit `allowedTools` overlay from the registry concept. **`bypassPermissions` is banned in this lane** — retires the H0-deferred `--dangerously-skip-permissions` question for Claude lanes. `model` and `effort` are **per-dispatch knobs** (lane config/env, H8 level 2): intra-lane model tiering (Haiku for mechanical task classes, Opus/Fable tiers for hard ones) and Bedrock/Vertex-hosted variants are configuration, not architecture | claude session, 2026-07-11 (model-knob clause added 2026-07-12) |
| L6 | Policy hook set (v1) | In-session, deny-with-remediation (R1): (a) deny `git push` to `main`/default; (b) deny writes under `.github/workflows/**` (PA5 made them un-pushable anyway — fail early, loudly); (c) at PR-create: Conventional-Commit title lint + `Closes #N`/`Refs #N` presence (same regexes as `_pr-title.yml`); (d) warn at soft size gate 400/10 before PR-create. Hook messages carry the fix verbatim | claude session, 2026-07-11 |
| L7 | Outcome record | Every run ends with `harness.outcome.v1` JSON (schema in `harness/schemas/outcome.v1.json`): issue, lane, **vendor**, agent label, model, result ∈ {pr_opened, branch_pushed, blocked, budget_exceeded, failed}, pr/branch refs, `total_cost_usd`, usage tokens, turns, wall-clock, session id, gate-hit list. **Vendor-neutral by requirement**: `lane`/`vendor`/`model` are free identifiers, no Claude-specific fields; non-SDK lanes may emit the same record from cheaper sources (e.g. a workflow post-step parsing the action log) — this record is H8's cross-lane currency. Emitted via SDK `structuredOutput` + wrapper fields; posted as an issue comment (fenced `json` block under a `harness-outcome:` marker) and `$GITHUB_STEP_SUMMARY`. This is the #682 scorecard row and the H8 Phase 0 seed schema, machine-produced | claude session, 2026-07-11 (vendor-neutrality requirement added 2026-07-12) |
| L8 | Identity source | The runtime reads `docs/agents/registry/<id>.md` (OKF frontmatter + charter) **live at run time** — agent label on the issue → concept → `systemPrompt` append (charter + standards), `allowedTools` overlay, `owned_paths` advisory context. No codegen step, no third view to drift; `gen_agents.py` views stay untouched for other consumers | claude session, 2026-07-11 |
| L9 | Lane naming + contracts | New lane `claude-sdk`: `dispatch.d/claude-sdk.sh` honors the adapter contract (prompt on stdin, issue `$1`, 60k guard — H4 L8 inherited); `.github/workflows/claude-sdk-worker.yml` honors the worker-workflow contract (`workflow_dispatch` inputs exactly `{issue, prompt}`, registered dormant on `main` per PA4). Registry `executors:` lists gain `claude-sdk` only where piloted | claude session, 2026-07-11 |
| L10 | Graduated cutover | `claude-workflow` (bare action) stays the workflow-lane default until the pilot scorecard shows parity-or-better on the #682 metrics (duplicate rate 0, acceptance ≥ baseline, review burden and cost ≤ current); owner flips the default; the bare lane is deleted only after a clean window — never same-day (precedent: #680 P1→P3, H4 shadow) | claude session, 2026-07-11 |
| L11 | Prompt contract unchanged | The runtime consumes the `dispatch.sh`-rendered prompt verbatim as the user turn; registry charter/policy layer *around* it, never forking the render. Slimming the prompt's prose ritual (parts the hooks now enforce) is a cross-lane follow-up — multiclaude still needs the prose | claude session, 2026-07-11 |

---

## Multi-vendor posture (how H9 composes with H8)

Owner direction (2026-07-12): the harness must enable multiple models and model
vendors. H9's answer, consistent with H8's three routing levels (#720):

- **Vendor plurality lives at the lane layer, not inside a universal runtime.** The
  vendor-neutral surface is the contracts, all GitHub-native: the task contract +
  claim protocol (H1), the adapter contract (prompt on stdin, issue `$1`), the
  worker-workflow contract (`{issue, prompt}`), the merge gates (vendor-blind law,
  per R1), registry `executors:` ∩ `routing.yml` (H8 Phase 1), and the L7 outcome
  record. The harness already runs three vendors through these contracts today —
  `claude-web`/`claude-workflow` (Anthropic), `jules` (Google), the `gemini-review`
  one-liner — with zero shared runtime.
- **Runtimes are deliberately per-lane and vendor-native.** Packaged coding agents
  are not interchangeable chat models: each vendor's agent is post-trained around its
  own tool harness, so a universal abstraction runtime runs every vendor
  off-distribution and absorbs every vendor's API churn (the §7 Manus lesson).
  In-session hooks (L6) are defense-in-depth and cost-efficiency; a lane without
  them is degraded-but-safe because the platform gates remain the enforcement
  boundary.
- **Per-vendor lane recipe** (how vendor N joins): packaged executor first (its
  official action — `jules-action`/`run-gemini-cli` for Google, Codex's action for
  OpenAI) behind a `dispatch.d/` adapter + worker workflow (~50 lines); it earns
  traffic through the same H4 scorecard; it gets a vendor-native programmatic
  runtime only when its traffic justifies L6/L7-grade control — for a Google lane
  that engine would be ADK-Go (see Alternatives considered below).
- **Multiple models within a lane is configuration, not architecture** — L5's
  `model`/`effort` knobs now, `routing.yml` task-class → (lane, model) at H8 Phase 1.

## Alternatives considered — Google ADK in Go (evaluated 2026-07-12)

Examined as a challenge to L2 at owner request; facts verified against
`github.com/google/adk-go` releases, `pkg.go.dev/google.golang.org/adk/v2`, and the
`google/adk-docs` source (fetched 2026-07-12).

**What it is**: Google's agent-orchestration runtime for Go — launched Nov 2025,
v1.0 GA Mar 2026, **v2.0.0 GA 2026-06-30** (`google.golang.org/adk/v2`, Apache-2.0,
Go ≥ 1.25). Verified capable where it matters mechanically: `BeforeToolCallback`
can block or rewrite tool calls (the L6 mechanism), 12-hook plugin system, workflow
graphs with HITL pause/resume, MCP toolset on the official go-sdk, A2A both
directions, and `skilltoolset` speaks the same open `SKILL.md` format as our pinned
skills. Its Go-ness also fits this repo better than TypeScript on the language axis
(no CLAUDE.md scope note needed).

**Why rejected for the Claude worker lane** (the burden L2 sets was examined, not met):

1. **Wrong category for a coding worker** — ADK supplies the agent loop but none of
   the domain layer: no file/edit/shell/git toolset in Go, no code executor, no
   coding-agent docs; the toolset and its safety tiers (what `.claude/settings.json`
   + Claude Code give us) become owned code — the "fiddly 20%" H4 refused to vendor.
2. **No Anthropic path from Go** — Claude adapters exist for Java (native) and
   Python (LiteLLM) but not Go; we'd hand-roll `model.LLM` over `anthropic-sdk-go`
   or adopt unbacked community libs, switch to `ANTHROPIC_API_KEY`/Vertex
   credentials (surrendering the existing OAuth secret + subscription Agent SDK
   credits, expanding the H5 surface L4 avoids), and run Claude *outside* the
   Claude-Code tool harness it is post-trained around — an unmeasured worker-quality
   regression the scorecard would pay to discover.
3. **Missing harness features in the Go column**: no `maxBudgetUsd`-equivalent
   (Go's batch `RunConfig` lacks even `max_llm_calls`), token usage but no dollar
   cost, eval framework Python-only, no documented CI/Actions pattern.
4. **Churn + evidence**: one breaking rewrite (1.x→2.0) within eight months of
   launch, vs. the GA-stable first-party SDK for the model we run; and the Claude
   lane holds all live evidence here (probe #713, PA4, six shadow nights, pilot
   batches) while an ADK lane would be a seventh executor starting at zero —
   against H4's consolidation direction.
5. **Even Google routes this use case elsewhere** — its packaged coding-agent
   answers are `jules-action` and `run-gemini-cli` (both already in this harness's
   orbit); ADK has no coding-agent story and neither Google agent is ADK-built.

**Recorded crossover trigger** (so this is a decision, not a dismissal): ADK-Go
becomes the recommended engine when a **programmatic Gemini lane** is warranted
(H8 Phase 1, or a second-vendor author lane whose packaged action can't provide
L6/L7-grade control); if **two or more non-Claude author lanes** ever need such
control, a shared ADK-Go runtime for those lanes beats N bespoke shims — the
Claude lane stays SDK-native regardless. Follow-ups carries the spike row.

---

## Cross-phase artifacts

| Artifact | Producer phase / task | Consumer phase / task | Lock # | Status |
|---|---|---|---|---|
| Probe report + decision matrix (comment on H9 issue) | A / A2 | B–E (design confirmations); plan-review v2 | PA1–PA3, PA7 | pending |
| `harness/` package (registry loader, options assembly, runner) | B / B1–B3 | C, D | L2, L5, L8 | pending |
| `harness/schemas/outcome.v1.json` | B / B4 | C (emitter), E (scorecard), H8 Phase 0 (#720) | L7 | pending |
| Policy hook modules + unit tests | C / C1–C2 | D (wired), E (gate-hit telemetry) | L6 | pending |
| `dispatch.d/claude-sdk.sh` adapter | D / D1 | E (via `dispatch.sh <n> claude-sdk`) | L9 | pending |
| `.github/workflows/claude-sdk-worker.yml` (dormant on `main`) | D / D2 | E (first live launch) | L4, L9 | pending |
| Repo secret `KAIZEN_WORKER_TOKEN` (owner-minted) | D (owner; **not in any PR**) | E live runs | L4 | pending |
| CLAUDE.md scope note (TS rule) + Critical Rules pointer | B / B6 | standing | L2 | pending |
| Pilot scorecard (outcome records vs bare lane) on H9 issue + #682 | E / E2 | cutover decision (owner), H8 Phase 1 | L7, L10 | pending |
| Proposal §4 H9 status annotations | each phase's final task | standing record | — | pending |

---

## Phase A — probes: SDK on this repo's rails (1 issue, 1 PR)

**Executor:** claude-web (remote session) or owner-local — this phase pushes a workflow
file on a vehicle branch, which the claude-workflow lane cannot do (PA5).
**Size budget:** ~150 counted lines / 4 counted files (probe script + vehicle workflow +
report; well under soft gate).

### Task A1: SDK probe (PA1–PA3)

- [ ] **Step 1:** vehicle branch with `harness-probe/` — `package.json` pinning
  `@anthropic-ai/claude-agent-sdk`, `probe.mjs` running a trivial task
  (`maxBudgetUsd: 0.50`, `maxTurns: 4`, `permissionMode: "dontAsk"`,
  `settingSources: ["project"]`, one scripted `PreToolUse` deny, one
  `structuredOutput` schema) — file: `harness-probe/probe.mjs`
- [ ] **Step 2:** `pull_request`-triggered probe workflow scoped to the vehicle branch
  (workflow-vehicle pattern — runs from the PR without touching `main`), secret
  `CLAUDE_CODE_OAUTH_TOKEN`
- [ ] **Step 3:** assert: session completes on OAuth auth; deny fired + remediation
  visible; settings.json deny honored; `total_cost_usd`/`usage` present;
  structured output validates; budget halt observed (second run, `maxBudgetUsd: 0.01`)

### Task A2: GitHub-writes probe (PA7) + report

- [ ] **Step 1:** with an owner-minted **throwaway** fine-grained PAT (same scopes as
  L4): push branch → open PR → confirm required checks trigger → remove a label →
  close + delete; record each result
- [ ] **Step 2:** post the probe report + decision matrix (proceed / adapt / kill per
  PA) as a comment on the H9 issue; annotate this plan's PA table verdicts; close the
  vehicle PR unmerged

**Kill criteria (Phase A):** PA1 fails (no headless OAuth path) → stop; H9 falls back
to a settings/hooks-only formalization *inside* `claude-code-action` (shell hooks in
`.claude/settings.json`) and this plan returns to review. PA7 fails → L4 reverts to
App token + hand-opened PRs, and the outcome record ships anyway (it needs no PAT).

## Phase B — runtime core (1 issue, 1 PR)

**Executor:** any lane (offline; no live API in tests).
**Size budget:** ~380 counted lines / 9 counted files (`package-lock.json` and docs
exempt).

### Task B1: package skeleton

- [ ] **Step 1:** `harness/package.json` (private, pinned SDK per L3, Node ≥ 18 engines,
  `npm test` = node test runner), `tsconfig.json`, `src/`, `test/` — no new CI runner
  deps beyond Node already used by `ui/`

### Task B2: registry loader (L8)

- [ ] **Step 1:** `src/registry.ts` — parse OKF frontmatter + charter from
  `docs/agents/registry/<id>.md`; resolve agent id from issue labels
  (`agent-N`/`infra-N`); fail loudly with remediation when no/ambiguous label
- [ ] **Step 2:** fixture tests against a copied real concept (`agent-3.md` shape:
  `type`, `label`, `executors`, `language`, `owned_paths`, `depends_on`)

### Task B3: options assembly + runner (L5)

- [ ] **Step 1:** `src/options.ts` — compose `query()` options: systemPrompt append
  (charter + standards + work-tracking pointer), `settingSources: ["project"]`,
  `permissionMode: "dontAsk"`, `allowedTools` overlay, `model`/`effort` from lane
  config, `maxBudgetUsd`/`maxTurns` defaults (overridable via env, mirroring
  `ORCH_*` knob style)
- [ ] **Step 2:** `src/worker.ts` — CLI entry: `--issue N` + prompt on stdin; drives the
  session; INIT/RESUME passthrough (the dispatch prompt already carries mode per H1 R2)

### Task B4: outcome schema (L7)

- [ ] **Step 1:** `harness/schemas/outcome.v1.json` + `src/outcome.ts` (emit from SDK
  result message + wrapper timings); schema-validation test

### Task B5: CI wiring

- [ ] **Step 1:** `harness-tests` job (or extend `orchestration-tests.yml` paths) —
  offline only, `permissions: contents: read`

### Task B6: docs

- [ ] **Step 1:** CLAUDE.md — one-line TS scope note (L2, owner-ratified at
  plan-review) + `harness/` row in Key File Locations; `scripts/orchestration/README.md`
  cross-pointer

## Phase C — policy hooks + outcome posting (1 issue, 1 PR)

**Executor:** any lane.
**Size budget:** ~350 counted lines / 8 counted files.

### Task C1: hooks (L6)

- [ ] **Step 1:** `src/hooks/` — push-to-main deny; workflow-path write deny;
  PR-create gate (title regex identical to `_pr-title.yml`'s set, `Closes/Refs #N`
  check); soft-size warning. Every deny message states the fix (R1)
- [ ] **Step 2:** unit tests: synthetic `PreToolUse` inputs → expected
  decision/remediation (pure functions; no API)

### Task C2: lifecycle integration

- [ ] **Step 1:** post `harness-outcome:` comment + `$GITHUB_STEP_SUMMARY` at end of
  run (success, blocked, or budget-exceeded); `progress-branch:` breadcrumb after first
  push (R2 resume contract, same marker `dispatch.sh` reads)
- [ ] **Step 2:** clean-failure claim release via existing
  `claims.sh release <issue> <worker-id>` semantics (comment marker); crash case stays
  TTL-sweep self-healing — no new machinery

## Phase D — lane wiring (1 issue, 1 PR)

**Executor:** claude-web or owner-local (touches `.github/workflows/**`; PA5).
**Size budget:** ~200 counted lines / 6 counted files.

### Task D1: adapter

- [ ] **Step 1:** `dispatch.d/claude-sdk.sh` — stdin prompt, `$1` issue, 60k guard
  (L9); `gh workflow run claude-sdk-worker.yml -f issue -f prompt`
- [ ] **Step 2:** extend `test_evening_dispatch.sh`/`test_dispatch.sh` gh-stub cases
  for the new adapter; `scripts/orchestration/README.md` adapter list + contracts

### Task D2: worker workflow (dormant)

- [ ] **Step 1:** `.github/workflows/claude-sdk-worker.yml` — `workflow_dispatch`
  `{issue, prompt}`; `timeout-minutes: 120`; checkout; `npm ci` in `harness/`; run
  `src/worker.ts` with `CLAUDE_CODE_OAUTH_TOKEN` + `KAIZEN_WORKER_TOKEN` (git/gh env);
  `permissions:` minimal (`contents: read`, `issues: write` for the outcome comment
  fallback); registered on `main`, launched by nothing until Phase E
- [ ] **Step 2:** owner action (no PR): mint `KAIZEN_WORKER_TOKEN` per L4

## Phase E — pilot + cutover decision (1 issue; PR only for config/doc deltas)

**Executor:** owner + dispatching session (live dispatches; owner gates).
**Size budget:** ~80 counted lines / 4 counted files (docs + registry `executors:`
additions + proposal annotation).

- [ ] **Step 1:** add `claude-sdk` to `executors:` on 2–3 registry concepts (pilot
  cohort); dispatch ~4 right-sized issues via `just work-on N executor=claude-sdk`
  (mirrors the 2026-07-06 batch)
- [ ] **Step 2:** collect outcome records; assemble the scorecard vs the bare
  `claude-workflow` lane on the #682 metrics; post to the H9 issue + #682
- [ ] **Step 3:** owner decision per L10: flip workflow-lane default / keep piloting /
  kill. On flip: bare lane enters its deletion clean-window; `evening_dispatch.sh`
  live path needs **no change** (it calls `dispatch.sh`, which resolves the default
  executor)
- [ ] **Step 4:** proposal §4 H9 status annotation + §6 row update

## Phase F — Convergence (folded into Phase E's PR)

### Task F1: Acceptance-criteria mapping

| Issue AC (H9 tracking issue) | Test/file location | Cross-phase artifact row |
|---|---|---|
| SDK proven on repo rails (auth, hooks, cost, structured output, budget halt) | Phase A probe assertions + report comment | probe report row |
| Runtime assembles options from registry live | `harness/test/registry.test.ts`, `options.test.ts` | `harness/` package row |
| Harness law enforced in-session with remediation | `harness/test/hooks.test.ts` | policy hooks row |
| Every run emits `harness.outcome.v1` | schema validation test + pilot comments | outcome schema row |
| Lane honors both normative contracts, dormant until pilot | adapter stub tests; workflow file on `main`; no launches pre-E | adapter + worker rows |
| Scorecard vs bare lane on #682 metrics | Phase E scorecard comment | pilot scorecard row |

### Task F2: regression

`cd harness && npm test && cd .. && bash scripts/orchestration/test_dispatch.sh && bash scripts/orchestration/test_ready_native.sh && bash scripts/orchestration/test_evening_dispatch.sh && python3 scripts/check_docs.py . && python3 scripts/check_okf.py docs/agents/registry`

### Task F3: PRs

One per phase (dispatchability rule): `chore(harness): H9 Phase A — SDK probes (vehicle, not-for-merge)` · `feat(harness): H9 Phase B — runtime core` · `feat(harness): H9 Phase C — policy hooks + outcome records` · `feat(orchestration): H9 Phase D — claude-sdk lane (dormant)` · `docs: H9 Phase E — pilot scorecard + cutover`. Each `Refs`/`Closes` the phase's issue.

---

## Test plan summary

| Phase | Test files | Count target |
|---|---|---|
| A | probe assertions (live, vehicle PR) | ≥6 assertions across PA1–PA3, PA7 |
| B | `harness/test/{registry,options,outcome}.test.ts` | ≥12 assertions |
| C | `harness/test/hooks.test.ts` + lifecycle tests | ≥10 assertions |
| D | `test_dispatch.sh` + `test_evening_dispatch.sh` extensions | ≥4 new cases |
| E | live pilot (scorecard is the artifact) | 4 dispatches |

---

## Risks + rollback

| Risk | Severity | Mitigation |
|---|---|---|
| SDK version churn breaks the runtime | med | exact pin (L3); bumps via Jules weekly lane with `harness` tests as the gate |
| `total_cost_usd` is a client-side estimate | low | record usage tokens alongside; scorecard compares like-with-like across lanes |
| PAT is a standing credential (H5's scariest class) | med | fine-grained, single-repo, three write scopes, Workflows/Admin withheld (L4); rotation note in the governance runbook; kill switch = delete the secret (lane fails loudly, claims TTL-expire) |
| Model-driven session with real write creds | med | defense in depth: `dontAsk` + settings.json deny tier + L6 hooks + ruleset-protected `main` + required checks on every PR |
| Runner allowlist blocks tools mid-session (#682 finding) | low | outcome record captures the gate-hit structurally; `blocked` result routes the issue back instead of half-done work |
| Two Claude workflow lanes during pilot confuse dispatch | low | default unchanged until L10 flip; `executors:` affinity limits `claude-sdk` to the pilot cohort |
| TS-rule scope objection at review | low | L2 ships the CLAUDE.md clarification for owner ratification; fallback = Python SDK accepted with its documented hook gaps (burden per L2) |

**Rollback:** Phases A–D are additive and dormant — revert the PR(s); no state, no
launched work (same posture as H4 Phase A). Phase E rollback = point the default lane
back to `claude-workflow` (one config line) and close the pilot cohort's `executors:`
entries.

**Replacement rule (graduated cutover):** the SDK lane ships ALONGSIDE the bare-action
lane; outcome records are the drift check; default-flip and bare-lane deletion are
separate, later, owner-gated steps on a clean window — never same-day.

---

## Follow-ups

| Item | Trigger | Owner |
|---|---|---|
| File H9 tracking issue + stamp this plan (`prime-issue`) | plan-review v1→v2 clean | owner + reviewing session |
| Slim `dispatch.sh` prompt prose that hooks now enforce (L11) | Phase C merged + one clean pilot | claude session |
| Outcome records for non-Claude lanes (multiclaude/jules parity for H8) | H8 Phase 0 kickoff (#720) | H8 plan |
| Evaluate `claude.yml`/`claude-code-review.yml` migration onto the runtime | L10 cutover complete | owner |
| Session-resume via SDK `resume`/`forkSession` for R2 re-dispatch | first `blocked`/crash pilot case | claude session |
| Registry `executors:` full rollout of `claude-sdk` | scorecard parity (L10) | owner |
| ADK-Go spike: engine for a programmatic `gemini-workflow` lane (see Alternatives considered) | H8 Phase 1 (`routing.yml`), or a second-vendor author lane clearing the packaged-action ceiling | owner + H8 plan (#720) |

---

## Branch + PR conventions

- This plan's PR: branch `claude/formalize-harness-sdk-styde2` (tolerated
  harness-session family; attribution rides PR metadata), title
  `docs: H9 draft plan — formalize the executor runtime on the Claude Agent SDK`.
- Implementation phases: Conventional Commits per Task F3; one issue = one worker
  session = one PR; markdown/lockfiles size-exempt, all phases sized under the soft
  gate.

## References

- Proposal: `docs/coordination/harness-modernization-proposal.md` (§3 principles, §4
  H4 amendment + Phase A note, §7 R1/R2/R3/R6)
- Contracts: `scripts/orchestration/README.md` (adapter + worker-workflow contracts)
- Precedent plan: `docs/superpowers/plans/2026-07-06-h4-evening-dispatcher-shadow.md`
- Prior art in-repo: `docs/design/agent-teams-vs-multiclaude-evaluation.md`
- SDK (verified 2026-07-11): overview `code.claude.com/docs/en/agent-sdk/overview.md`;
  hooks `…/agent-sdk/hooks.md`; permissions `…/agent-sdk/permissions.md`; structured
  outputs `…/agent-sdk/structured-outputs`; sessions `…/agent-sdk/sessions.md`;
  TS package `@anthropic-ai/claude-agent-sdk` (npm), Python `claude-agent-sdk` (PyPI);
  GitHub integration `code.claude.com/docs/en/github-actions`
- ADK-Go evaluation sources (verified 2026-07-12): `github.com/google/adk-go`
  (releases, `agent/llmagent` callback semantics), `pkg.go.dev/google.golang.org/adk/v2`
  (module surface: model/tool/session/plugin packages), `github.com/google/adk-docs`
  (models/anthropic, runconfig, evaluate, a2a, skills, deploy pages),
  `github.com/google-labs-code/jules-action`, `github.com/google/agents-cli`
