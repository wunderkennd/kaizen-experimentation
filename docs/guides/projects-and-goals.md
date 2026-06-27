# GitHub Projects & Goals

This guide defines how the kaizen-experimentation repo tracks work after adopting
**GitHub Projects (v2)**. It supersedes the Milestone-as-Sprint model documented in
[github-issues-workflow.md](./github-issues-workflow.md) — see
[Migration from Milestones](#migration-from-milestones) below.

> **TL;DR**
> - **Sprint** moves from a Milestone to the Project's native **Iteration** field.
> - **Goal** is a new top-level concept: a typed parent Issue with one success metric and a tree of native sub-issues.
> - Reporting labels (`P0..P4`, `cluster-a..f`) become Project **fields**.
> - `agent-N` and `sprint-N` **labels stay** — orchestration tooling reads them over the REST API.

## The four axes

Work is described along four orthogonal axes. None is a "size" of another — each
answers a different question.

| Concept | Question it answers | GitHub primitive | Time-bound? |
| --- | --- | --- | --- |
| **Goal** | *What outcome are we causing, and how do we know we hit it?* | Parent **Issue** (type `Goal`) + sub-issues | No (closes when metric met) |
| **Sprint** | *When does this batch of work happen?* | Project **Iteration** field | Yes (2-week cadence) |
| **Issue** | *What unit of work needs doing?* | **Issue** | No (open/closed) |
| **ADR** | *What decision did we make, and why?* | `docs/adrs/NNN-*.md` | No (immutable record) |

A Goal **cuts across** the other three: it spans multiple iterations, may reference
several ADRs, and owns many issues. That cross-cutting property is the tell that it
is a distinct top layer, not a synonym for a big issue or a small sprint.

```
GOAL  (outcome + metric)         "ADR-026 Custom Metrics GA — 3 metric types live,
  │                               operators self-serve, <5% definition error rate"
  ├─ sub-issue #552              (native GitHub sub-issue → drives the progress bar)
  ├─ sub-issue #555
  ├─ sub-issue #435
  └─ sub-issue #436
        each leaf: Iteration field = "Sprint 5.6", Owner = agent-3, Closes #N on merge
```

## Goal granularity

**One Goal per ADR, or per named non-ADR initiative. Never per-issue; never one mega-goal.**

A work item qualifies as a Goal **only if all four hold**:

1. It has exactly **one measurable success metric** (a threshold, not "done").
2. It spans **≥ 1 iteration**.
3. It owns **≥ 2 child issues**.
4. It maps to an **ADR number** *or* a **named initiative**.

If a piece of work is a single issue, it is an Issue — not a Goal. If it is "all of
Phase 6", it is too coarse to measure — split it per ADR.

### Seed goals (current state, 2026-06)

Filed via `scripts/projects/seed-goals.sh` (active goals first, then the rest).

| Goal | Issue | Source | Success metric (example — refine on creation) |
| --- | --- | --- | --- |
| Infrastructure GA (Pulumi/AWS) | [#647](https://github.com/wunderkennd/kaizen-experimentation/issues/647) | Sprints I.0–I.3 | All 9 Kaizen services deployed; mock suite green; observability wired |
| ADR-031 ConnectRPC Pilot | [#648](https://github.com/wunderkennd/kaizen-experimentation/issues/648) | ADR-031 | M1 RPCs over Connect; JSON shim retired; pilot meets success/kill criteria |
| ADR-026 Custom Metrics Layer → GA | [#649](https://github.com/wunderkennd/kaizen-experimentation/issues/649) | ADR-026 | 3 Tier-1 metric types GA; operators self-serve; <5% definition-validation error rate |
| ADR-027 TOST Equivalence Testing → GA | [#650](https://github.com/wunderkennd/kaizen-experimentation/issues/650) | ADR-027 | TOST exposed in M4a/M5/M6; golden files match R `TOSTER` to 6 dp |
| ADR-028 M4b Shadow Inference | [#651](https://github.com/wunderkennd/kaizen-experimentation/issues/651) | ADR-028 | Shadow core promotes policies with zero prod-traffic exposure regressions |
| ADR-029 Cross-Modal Score Calibration | [#652](https://github.com/wunderkennd/kaizen-experimentation/issues/652) | ADR-029 | Unified NEV scale across ≥3 modalities; calibration error within target band |
| ADR-030 Shadow Experiment Mode | [#653](https://github.com/wunderkennd/kaizen-experimentation/issues/653) | ADR-030 | Candidate variants run on prod traffic with 0 user-facing exposure incidents |
| QoE Observability GA | [#654](https://github.com/wunderkennd/kaizen-experimentation/issues/654) | EBVS + HeartbeatSessionizer | Server-side QoE aggregation live; EBVS first-class on `PlaybackMetrics` |
| Palette / M6 Design-System Standardization | [#655](https://github.com/wunderkennd/kaizen-experimentation/issues/655) | Palette polish | Search, empty states, filter-clear, CopyButton standardized across M6; a11y pass |

> The two **active** goals (live open work) are filed first; the remaining seven are
> filed in a second batch. ADR-031 is a ninth goal that spun up after the original
> eight were drafted.

## Project field schema

Configure these on the Project. Fields beat labels for *reporting* dimensions — they
are sortable, groupable, and do not pollute the global label namespace. We promote
reporting labels to fields and keep only the labels orchestration tooling reads.

| Field | Type | Replaces | Notes |
| --- | --- | --- | --- |
| **Status** | Single-select | (new) | `Backlog → Ready → In Progress → In Review → Blocked → Done` |
| **Iteration** | Iteration | **Milestones** | 2-week cadence; "Sprint 5.6", "Sprint I.2" become iterations |
| **Goal** | Single-select | (new) | Mirror of the parent-issue title, for table grouping when sub-issue view is not used |
| **Owner** | Single-select | mirrors `agent-N` / `infra-N` label | Field is for humans/Roadmap; the **label stays** for automation (see caveat) |
| **Priority** | Single-select | `P0..P4` labels | Drop the labels once migrated |
| **Cluster** | Single-select | `cluster-a..f` labels | Drop the labels once migrated |
| **ADR** | Text | (new) | e.g. `ADR-026`; filter "all work for an ADR" across iterations |
| **Estimate** | Number | (new) | Optional; enables burn-up charts in Insights |

## Views

Three saved views are "same data, three lenses" — no duplication:

| View | Layout | Grouped by | Used for |
| --- | --- | --- | --- |
| **Board** | Board | `Status` | Daily execution (Gas Town / Multiclaude operational view) |
| **Roadmap** | Roadmap | `Goal`, laid on `Iteration` | The outcome view — goals across time |
| **By Agent** | Table | `Owner` | Per-agent load balancing |

## Creating a Goal

1. Open a new Issue using the **Goal** template (`.github/ISSUE_TEMPLATE/goal.md`).
2. Fill the **Success Metric** — a threshold, not a checklist.
3. Add the child issues as **native sub-issues** (Issue sidebar → *Create sub-issue* /
   *Add existing*). The parent's progress bar is driven by sub-issue closure.
4. Add the Goal issue to the Project; set the `Goal` field on each child to match.
5. Leaf issues keep using `Closes #N` in their PRs — unchanged.

## Migration from Milestones

The repo already runs a parallel `sprint-N` **label** system (`beads-sync`,
`sprint-status`, `evening` all key off labels). So automation keeps working if we keep
those labels and move only the human/reporting sprint axis to the Iteration field.

**Blast radius** (everything that reads Milestones today):

| Consumer | Today | After migration |
| --- | --- | --- |
| `just morning` | `gh api repos/:owner/:repo/milestones` → active milestone | Reads current Iteration from the Project (GraphQL), falls back to active `sprint-N` label |
| `just agent-work` | prints `.milestone.title` | prints `sprint-N` label (REST-visible) |
| `docs/guides/github-issues-workflow.md` | "Milestones = Sprints" | Banner points here |

> **The caveat that bites automation:** Project v2 fields and Iterations are reachable
> **only via the GraphQL API** — they are invisible to REST `gh issue list --milestone`
> and to label-based hooks. That is why `agent-N` and `sprint-N` stay as labels: the
> Owner/Iteration *fields* are for humans and the Roadmap; the *labels* are for machines,
> until the orchestration layer speaks GraphQL.

### Procedure

> **UI steps the API can't do** (the bootstrap script prints these): GitHub seeds a
> default **Status** field with `Todo|In Progress|Done` — reconcile it to
> `Backlog, Ready, In Progress, In Review, Blocked, Done`. The **Iteration** field and
> the three **Views** are also UI-only (single-select option editing, iteration fields,
> and view creation are not exposed by the Projects-v2 API).

```bash
# 1. Create the Project + fields (idempotent; dry-run by default). Single-select fields
#    whose options drift are reported as WARN, not silently skipped.
./scripts/projects/bootstrap-project.sh --owner <org-or-user> --apply

# 2. Migrate existing Milestones → Iterations and move their issues (dry-run by default)
./scripts/projects/migrate-milestones-to-iterations.sh --owner <org-or-user> --project <number> --apply

# 3. Verify, then close (do not delete) the old Milestones so history is preserved
```

Run a **transition sprint with both systems live** before removing Milestone reads
from the justfile. Do not delete Milestones — close them; their `due_on` dates seed the
iteration boundaries.

## See also

- [github-issues-workflow.md](./github-issues-workflow.md) — Issue body format, labels, `Closes #N`
- [orchestration-workflow.md](./orchestration-workflow.md) — Gas Town / Multiclaude dispatch
- `.github/ISSUE_TEMPLATE/goal.md` — the Goal issue template
- `scripts/projects/` — bootstrap + migration scripts
