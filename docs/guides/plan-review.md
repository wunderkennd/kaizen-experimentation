# Plan Review

The named step between "a plan exists" and "`just prime-issue` blesses it."
Codifies the #680 v1→v2 exercise (2026-07-05) — the highest-leverage hour of
the H2 replan — as a repeatable procedure. Part of H7 (#699); the quality bar
itself lives in [`delivery-lifecycle.md`](./delivery-lifecycle.md) and is
baked into the
[locked-plan template v2](../superpowers/templates/locked-plan-template.md).

## When

- Before `prime-issue` stamps the plan into its issue (always).
- Again if the plan sat unexecuted across a sprint boundary, or if any
  live-state claim it rests on may have drifted (the #680 v1 plan predated
  H1/H6 landing — half its premises were stale).

## Who

Anyone — the owner, a module agent, or a dispatched worker (the review is
itself a fine one-session unit of work). The reviewer must NOT be the plan's
author when the plan crosses module boundaries.

## The procedure

1. **Re-verify every live-state claim against the source, not memory.**
   Issue states, sub-issue trees, file paths and line numbers, "X is still
   true" premises — one API call or grep each. Write down what drifted.
   (#680 v1 cited a closed blocker and three Goals whose children were never
   filed.)
2. **Walk the quality bar** (delivery-lifecycle.md § Plan quality bar):
   probe-gated platform assumptions · decisions-not-options · executor
   constraints stated · phases sized to the PR gate · graduated cutover for
   replacements · Cross-phase artifacts table when multi-phase.
3. **Check template conformance** — the v2 skeleton's mandatory sections
   exist and aren't vestigial (an empty probes section on a plan full of
   unexercised platform bets is the review's #1 catch).
4. **Check dispatchability** — does each phase fit one worker session and one
   PR; does a calendar gate live *between* issues rather than inside one
   (claim leases are 24h; `Closes #N` fires on the first merge); is anything
   conflated that separate owners should hold (product scoping vs plumbing)?
5. **Produce v2, not comments.** Edit the plan; summarize the delta as a
   numbered review note **on the plan's issue** (what changed and why — the
   #680 note is the exemplar). The plan's `**Plan-review:**` line links that
   note.
6. **Re-run `just prime-issue <N>`** so the issue banner picks up the new
   plan SHA.

## Outputs

- The revised plan (v2) on `main`.
- The review note on the issue — numbered findings, each with the fix taken.
- Updated `**Plan-review:**` link in the plan's status block.

A review that finds nothing states that explicitly on the issue ("plan-review
vN: clean — premises re-verified <date>") — silence is indistinguishable from
"nobody looked."
