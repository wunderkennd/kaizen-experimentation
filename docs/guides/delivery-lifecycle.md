# Delivery Lifecycle

How an idea becomes merged code in this repo — every stage, the artifact it
produces, the carrier that produces it, and the check that guards it. This is
the spine H7 (#699) hangs the practice conventions on; the stages themselves
predate it (this document names what the repo already does well and fills the
gaps).

**The ratchet principle** (proven by `just prime-issue`): each stage's gate is
*advisory until it earns required*. `prime-issue` already refuses to dispatch
an issue that has no plan — an issue without a plan cannot enter the harness.
Every other gate below extends that same ratchet one stage earlier, and gets
promoted (advisory → required) only after a clean window, the same
verify-then-require discipline the merge path used for its checks.

## The map

| Stage | Question it answers | Artifact (where) | Carrier | Check |
| --- | --- | --- | --- | --- |
| **Requirements** | What outcome, for whom, measured how? | PRD → `docs/prds/YYYY-MM-DD-<slug>.md`; outcome lands as a **Goal** issue (one metric) | `to-prd` skill, interrogated by `grill-me` / `grill-with-docs`; `docs/templates/prd-template.md` | Goal carries exactly ONE success metric (`projects-and-goals.md` rule; linted by `check_docs.py` once it lands) |
| **Cross-boundary design** | Do other repos/teams/services need to weigh in? | RFC → an issue titled `RFC-NNN: <title>` (precedent: #543–#545); the accepted decision graduates into an ADR | `docs/templates/rfc-template.md` (issue body) | Decision recorded with owner + date before dependent work dispatches |
| **Architecture decision** | What did we decide, and why is it permanent? | ADR → `docs/adrs/NNN-<slug>.md` | `documentation-and-adrs` skill; imitate 001–030 | Status/Deciders/Impact present (`check_docs.py`, advisory) |
| **UX design** (M6-touching work) | What states, flows, and a11y acceptance before build? | UX spec → `docs/superpowers/specs/` alongside the technical spec | `docs/templates/ux-spec-template.md` (lands in H7 PR-4) | States enumeration incl. empty/loading/error (Palette standards) |
| **Spec** | What exactly are we building; which decisions are **Locked**? | Spec → `docs/superpowers/specs/YYYY-MM-DD-<slug>.md` | Imitate the corpus; Locks per the convention below | Every artifact named in a Lock or stub appears in the plan's Cross-phase table (spec-review blocker, per the locked-plan template) |
| **Plan** | Who builds what, in which order, sized how? | Plan → `docs/superpowers/plans/YYYY-MM-DD-<slug>.md` | `docs/superpowers/templates/locked-plan-template.md` | **plan-review** (below), then `just prime-issue <N>` stamps the plan into the issue |
| **Dispatch → merge** | — | Issues (+ claims), PRs | H1 dispatch (`scripts/orchestration/`) | The required-check set: PR title / Review gate / PR size / CI |

**When RFC vs ADR**: an **RFC is a conversation** — use it when the boundary
crosses repos, teams, or external consumers and you are *requesting comment*
before deciding (RFC-001 #543 is the precedent: the Personalization service's
event-emission boundary). An **ADR is a record** — use it when the decision is
this repo's to make and needs a permanent, numbered rationale. An accepted RFC
usually *concludes in* an ADR; an ADR never needs an RFC when the blast radius
is internal.

**The Lock convention** (named here; practiced since ADR-026): a **Lock** is a
decision written into a spec or plan that shifts the burden of justification
to whoever wants to change it ("The Lock is CodeMirror 6 … the burden of
justification falls on Monaco"). Locks exist so dispatched workers do not
re-litigate settled questions mid-implementation. Challenging a Lock is
allowed — by reopening it with the spec's owner, never by silently building
something else.

## Plan quality bar (enforced by plan-review)

Every plan must clear the bar the #680 v1→v2 review established:

1. **Probe-gated platform assumptions** — any capability the design bets on
   that has not been exercised on this infrastructure gets a step-0 probe with
   exact commands and a decision matrix. (This repo was burned twice in one
   day by documented-but-rejected platform behavior: the
   `pull_request_review_thread` trigger; ruleset `evaluate` on non-Enterprise.)
2. **Decisions, not options** — "do X or Y" is a spec gap; the plan records
   the decision and its owner/date.
3. **Executor constraints stated** — which phases need `gh api`/local auth vs
   run anywhere (claude-web has no `gh`); one issue = one worker session = one
   PR.
4. **Phases sized to the PR gate** — soft 400 lines / 10 files; a phase that
   can't fit is two phases.
5. **Graduated cutover for replacements** — new path ships alongside the old
   with a drift check; deletions gate on a clean window (never same-day).
6. **Cross-phase artifacts table** — mandatory for multi-phase plans (see the
   locked-plan template's rationale; ADR-026 P3's orphaned RPC is the
   incident it encodes).

Run [plan-review](./plan-review.md) before `prime-issue`; record the review
as a comment on the plan's issue (v1 → v2 diffs are the deliverable, as on
#680).

## Where each carrier lives

- Skills: pinned in `skills-lock.json`, restored by `just install-skills`
  (`to-prd`, `grill-me`, `grill-with-docs`, `to-issues`,
  `documentation-and-adrs`, `tdd`, `triage`, …).
- Templates: `docs/templates/` (PRD, RFC; ux-spec arrives in H7 PR-4) and
  `docs/superpowers/templates/` (locked plan).
- Checks: `scripts/check_docs.py` (H7 PR-3, advisory) alongside
  `scripts/check_okf.py`; the merge-path checks are already required.

All lifecycle artifacts carry OKF-style frontmatter (`type`, plus status and
links) so the knowledge layer stays machine-indexable — same pattern as
`docs/agents/registry/` and the planned metric catalog (#683).
