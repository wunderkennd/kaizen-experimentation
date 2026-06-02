# Locked-plan template

Use this skeleton for every multi-phase ADR implementation plan landed under
`docs/superpowers/plans/`. The section order is canonical; the
**Cross-phase artifacts** table is mandatory whenever a plan has more than one
phase. Filename convention: `YYYY-MM-DD-<adr-or-issue>-<slug>.md`.

---

## What the template captures (and why)

Phase-N plans dispatch one worker per phase. Workers see only their slice and
trust the plan to surface anything that crosses slice boundaries. When an
artifact's producer and consumer live in different phases, three things
typically happen:

1. The producer phase ships without producing the artifact (the consumer was
   "not really part of my work").
2. The consumer phase ships with a stub or a "tracked in #NNN" reference that
   never gets followed up.
3. The convergence (F) phase doesn't open because all individual phases
   "shipped," and the issue auto-closes from a stray `Closes #N`.

ADR-026 Phase 3 hit exactly this trail: `MigrateMetricDefinition` was
mentioned in the Phase A migrator's `apply` stub help text and in the L7 Lock
prose, but no phase explicitly owned producing the RPC. Phase C shipped the
deprecation surface alone, Phase A's `apply` stayed unimplemented on `main`,
and #437 closed administratively when the last slice merged. The
Cross-phase artifacts table prevents this by making cross-slice dependencies
discoverable from a single grep.

`★ Insight ─────────────────────────────────────`
The table is not just documentation — it's the single source of truth that
spec reviewers grep against. A Lock or stub-help-text that names an artifact
which doesn't appear in the table is treated as a spec-review blocker.
`─────────────────────────────────────────────────`

---

## Skeleton (copy from the line below)

```markdown
# <ADR-N Phase X / Issue title> (#<issue>)

**Status:** <Design lock — RFC for review | Locked | Executing | Shipped>.
**Issue:** [#N](https://github.com/<org>/<repo>/issues/N) — <priority>, <sprint>, <cluster>, owners <agent-X, agent-Y>.
**Blocked by:** <upstream PRs/issues with strikethrough once merged>.

---

## Summary

<2–4 paragraphs. What ships, why now, what trustworthiness/correctness/safety
constraint motivates the design.>

### Non-goals (v1 of #N)

- <Bullet list of explicit non-goals. Forces the plan to declare its boundary
  so reviewers don't expand scope mid-flight.>

---

## Locks — binding for implementers

Locks freeze the cross-cutting design decisions. **Locks are normative — copy
verbatim, do not drift.** If a Lock seems wrong, BLOCK and escalate via an
issue comment rather than overriding in implementation.

| # | Lock | One-line answer |
|---|---|---|
| L1 | <Topic> | <Answer> |
| L2 | <Topic> | <Answer> |
| ... | | |

<Per-lock detail sections follow if more nuance is needed.>

---

## Cross-phase artifacts

Every artifact named in a Lock body, in a stub help-text, in a runbook
reference, or in a per-phase task list that crosses a phase boundary MUST
appear in this table. Spec reviewers grep this table when reviewing each phase
PR; missing rows are a blocker.

| Artifact | Producer phase / task | Consumer phase / task | Lock # | Status |
|---|---|---|---|---|
| <proto RPC, file path, flag key, table name, ...> | Phase A / A2 | Phase C / C3 | L7 | <pending / produced / consumed / verified> |
| ... | | | | |

**Authoring rules:**

1. **Producer is always upstream.** If Phase A's worker can't ship without
   the artifact, and Phase C produces it, you have a dependency cycle — split
   the plan so the producer phase comes first.
2. **No "implicit" artifacts.** If a Lock body says "Phase B writes to
   `delta.metric_summaries`," that's an artifact: add a row.
3. **The convergence (F) phase verifies every row reaches `verified`.** F1's
   acceptance-criteria mapping cross-references this table; a row stuck at
   `pending` blocks issue closure.
4. **Stub-marker comments in code must reference a row.** A
   `Status::unimplemented("apply — see plan's cross-phase row 'MigrateMetricDefinition'")`
   ties source-code TODOs back to the plan; the [stub-markers CI workflow](../../../.github/workflows/stub-markers.yml)
   enforces the comment format.

---

## Phase A — <name>

<Per-task breakdown with checkbox steps. Each task names the file(s) it
touches and the test(s) it owns.>

### Task A1: <subject>

- [ ] **Step 1:** <action> — file: `<path>`
- [ ] **Step 2:** <action>

### Task A2: <subject>

...

---

## Phase B — <name>

...

---

## Phase F — Convergence

### Task F1: Acceptance-criteria mapping

<Table mapping each AC bullet from the parent issue to the test/file location
that verifies it. Required.>

| Issue AC | Test/file location | Cross-phase artifact row |
|---|---|---|
| <Issue's AC bullet, verbatim> | `<path>:<line>` or `<test name>` | <row from Cross-phase artifacts, if any> |

### Task F2: Full-suite regression

```
cargo test --workspace
go test ./...
cd ui && npm test
<plus any parity / golden gates>
```

### Task F3: Final commit + PR

`<conventional commit message>` — push, open PR with `Closes #N`, link the
corpus, include the AC mapping table, link the Cross-phase artifacts table
with every row at `verified`.

---

## Test plan summary

| Phase | Test files | Count target |
|---|---|---|
| A | <paths> | <count> |
| ... | | |

---

## Risks + rollback

| Risk | Severity | Mitigation |
|---|---|---|
| ... | | |

**Rollback for any phase:** <one-line per phase>

---

## Follow-ups

| Item | Trigger | Owner |
|---|---|---|
| **#N.1** — <follow-up subject> | <condition that activates it> | <agent> |

---

## Branch + PR conventions

- Branches: `agent-N/<verb>/<adr-XXX-slug>` per CLAUDE.md and
  [`.github/branch-naming.yml`](../../../.github/branch-naming.yml). Verbs:
  `feat`, `fix`, `port`, `design`, `chore`, `refactor`, `docs`, `test`,
  `perf`.
- Commits: Conventional Commits with crate/module scope.
- PR `Closes #N` on the convergence (F) PR only; intermediate phase PRs use
  `Refs #N`.
```

---

## Worked examples

- **Done right (will be):** the next ADR plan after this template lands.
- **Cautionary tale:** [`docs/superpowers/plans/2026-05-30-adr-026-phase-3-custom-migration.md`](../plans/2026-05-30-adr-026-phase-3-custom-migration.md)
  shipped without a Cross-phase artifacts table; the missing
  `MigrateMetricDefinition` RPC went un-owned and #437 closed
  administratively. The Phase 3 completion sweep retroactively adds the
  table to the plan as part of the convergence work.
