# ADR-009: Automated Bucket Reuse with Cooldown Period

**Status**: Accepted
**Date**: 2026-03-03

---

## Context
Spotify builds bucket reuse directly into their platform to prevent traffic exhaustion at high experiment volume. Without bucket reuse, a platform running 100+ experiments per quarter on a 10,000-bucket layer would exhaust its hash space within 2-3 quarters. Manual bucket management is operationally burdensome.

## Decision
When an experiment transitions to CONCLUDED, its hash-space allocation is automatically returned to the layer's available pool after a configurable cooldown period (default 24 hours):

1. Experiment concludes → M5 sets `released_at = NOW()` and `reusable_after = NOW() + cooldown` on the LayerAllocation.
2. M1 stops serving assignments for concluded experiments (config update streamed from M5).
3. After cooldown, the allocation is eligible for new experiments. M5 validates no overlap with active or cooling-down allocations.
4. The cooldown prevents late-arriving exposure events (from mobile clients with delayed event delivery) from being associated with the wrong experiment.

## Alternatives Considered
- **No automatic reuse (manual)**: Requires a platform admin to manually free allocations. Doesn't scale beyond ~50 experiments.
- **Immediate reuse (no cooldown)**: Risk of attribution errors from late-arriving events. A mobile client that was offline for hours could submit an exposure event for a concluded experiment, and if the bucket has already been reused, the event would be incorrectly attributed to the new experiment.
- **Longer cooldown (7 days)**: Safer but wastes bucket capacity. 24 hours covers the vast majority of late-arriving events (mobile clients sync within hours of coming online).

## Consequences
- M5 must track allocation lifecycle timestamps and reject premature reuse.
- M1 config snapshot must include allocation status (active, cooling, available).
- Layer utilization dashboards should show active, cooling, and available bucket counts.
