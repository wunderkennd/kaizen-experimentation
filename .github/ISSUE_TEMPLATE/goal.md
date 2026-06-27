---
name: "🎯 Goal"
about: "An outcome with a measurable success metric, spanning ≥1 iteration and ≥2 child issues. One Goal per ADR or named initiative."
title: "Goal: <ADR-NNN or initiative name> — <outcome>"
labels: ["goal"]
---

<!--
A Goal is an OUTCOME, not a task. Before filing, confirm all four hold:
  1. Exactly one measurable success metric (a threshold, not "done").
  2. Spans ≥ 1 iteration.
  3. Owns ≥ 2 child issues.
  4. Maps to an ADR number OR a named initiative.
If it is a single unit of work, file a regular Issue instead.
See docs/guides/projects-and-goals.md.
-->

## Outcome

<!-- One or two sentences: what becomes true in the world when this Goal is met. -->

## Success metric

<!--
A single, measurable threshold. NOT a checklist of work.
Good:  "Operators define custom metrics with <5% validation-error rate over 30 days."
Good:  "TOST golden files match R `TOSTER` (tsum_TOST) to 6 decimal places."
Bad:   "All three metric types implemented."  ← that's work, not an outcome.
-->

**Metric:**
**Target / threshold:**
**Measured by:**

## Source

- **ADR:** <!-- ADR-NNN, or "none (initiative)" -->
- **Cluster:** <!-- cluster-a .. cluster-g, or n/a -->
- **Primary modules:** <!-- e.g. M5, M3, M4a -->

## Child issues

<!--
Add these as NATIVE sub-issues via the Issue sidebar (Create sub-issue / Add existing)
so the parent progress bar tracks closure. List them here for readability too.
-->

- [ ] #
- [ ] #

## Project fields to set

When adding to the Project, set:

- **Goal** field → this issue's title
- **Iteration** → the iteration where the metric is expected to be met
- **Owner**, **Priority**, **ADR** → as applicable

## Out of scope

<!-- What this Goal explicitly does NOT cover, to keep the metric honest. -->
