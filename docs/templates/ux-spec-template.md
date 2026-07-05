---
type: UX-Spec
title: <surface / flow name>
status: Draft # Draft | Locked | Implemented
owner: <agent-6 or feature owner>
relates: [] # PRD, technical spec, issue, ADR
timestamp: <YYYY-MM-DDT00:00:00Z>
---

# UX Spec: <title>

<!--
For M6-touching work: fill this BEFORE the technical spec's UI decisions get
made ad-hoc inside a plan (the CodeMirror Lock was decided mid-plan — this
stage exists so the next one is decided here, with states and a11y up front).
Lands beside the technical spec: docs/superpowers/specs/YYYY-MM-DD-<slug>-ux.md.
The states table is the review focus — Palette's whole polish stream exists
because these were designed late.
-->

## Purpose & user

<!-- One paragraph: who is on this screen and what they came to do. Link the
PRD's user section rather than restating it. -->

## Entry points

<!-- Every way a user arrives (nav, deep link, redirect, empty-dashboard CTA).
Each entry point appears in the flows below. -->

## States — mandatory enumeration

<!-- Every view/component this spec introduces or changes, against the five
canonical states. "n/a" must be argued, not assumed. Treatments reference
Palette standards (standardized search, empty states, filter clearing,
CopyButton) and shadcn/ui components — reuse before new. -->

| View / component | Empty | Loading | Error | Partial | Filled |
| --- | --- | --- | --- | --- | --- |
| <component> | <treatment or n/a + why> | | | | |

## Flows

<!-- Happy path plus every failure path a state above implies. Numbered steps;
each step names the state it lands in. Mermaid optional. -->

## Component inventory

<!-- What's reused (shadcn/ui, existing M6 components, Palette patterns) vs
genuinely new. New components carry one line of justification — the burden
of justification is on new (same posture as Locks). -->

| Component | Reuse / new | Source or justification |
| --- | --- | --- |

## Accessibility acceptance

<!-- Checkable before merge — these become test/review items, not vibes. -->

- [ ] Full keyboard path: <entry → primary action → exit>
- [ ] Focus order and visible focus states across every state above
- [ ] Labels/roles for interactive elements (screen-reader pass)
- [ ] Color is never the only signal; contrast meets Palette baseline

## Instrumentation

<!-- Events this surface emits — names, triggers, properties. This is an
experimentation platform; a surface that emits nothing cannot be measured
against the PRD's metric. -->

| Event | Trigger | Properties |
| --- | --- | --- |

## Locks

<!-- UI decisions frozen for implementers (library, layout system, pattern),
one per row, decided with owner + date — same convention as plan Locks. -->

| # | Lock | One-line answer | Decided (owner, date) |
| --- | --- | --- | --- |

## Handoffs

<!-- What this feeds: the technical spec's UI sections, the plan's M6 phase,
webapp-testing coverage (Playwright flows mirror the Flows section). -->
