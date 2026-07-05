---
type: PRD
title: <one-line product statement>
status: Draft # Draft | Reviewed | Committed (Goal filed) | Superseded
owner: <who answers questions about this>
goal: "#<Goal issue number, once filed>"
relates: [] # ADRs, RFCs, prior PRDs
timestamp: <YYYY-MM-DDT00:00:00Z>
---

# PRD: <title>

<!--
Generate the first draft with the `to-prd` skill, then interrogate it with
`grill-me` (or `grill-with-docs` against the design doc) until the Open
questions section stops growing. File location:
docs/prds/YYYY-MM-DD-<slug>.md. Exit criterion: a Goal issue exists carrying
the ONE primary metric, and this PRD links it in the frontmatter.
-->

## Problem

<!-- Who hurts, how much, and how do we know? Cite evidence (incidents,
metrics, support themes) — not vibes. -->

## Users

<!-- The distinct people/roles affected, and what each is trying to do.
For this platform that often includes operators, module agents, and
downstream service consumers — not just end users. -->

## Outcome & metric

<!-- Exactly ONE primary success metric with a threshold (the Goal rule from
projects-and-goals.md: "a threshold, not 'done'"). Secondary/guardrail
metrics may be listed, clearly marked as such. -->

- **Primary metric**:
- Guardrails:

## Requirements

<!-- MoSCoW-lite. Each requirement is testable — a reviewer can say
"met / not met" without interpretation. -->

**Must**
-

**Should**
-

**Won't (this iteration)**
-

## Non-goals

<!-- What this deliberately does not attempt — with one line of why, so the
boundary survives re-litigation. -->

## Open questions

<!-- Each with an owner and the stage where it must close (RFC? spec?).
An open question with no owner is a risk, not a question. -->

## Handoffs

<!-- What this PRD feeds: RFC-NNN (cross-boundary), ADR-NNN (decision),
spec (docs/superpowers/specs/), Goal issue #N with sub-issue tree. -->
