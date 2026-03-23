# Agent-1 Status — Phase 5

**Module**: M1 Assignment
**Last updated**: 2026-03-23

## Current Sprint

Sprint: 5.0
Focus: ADR-022 Switchback assignment (complete), ADR-016 GetSlateAssignment (next), ADR-013 META routing
Branch: work/calm-lion

## In Progress

- [ ] ADR-016 GetSlateAssignment — slate bandit forwarding to M4b
  - Blocked by: none (M4b SlatePolicy exists)
  - ETA: next sprint

## Completed (Phase 5)

- [x] **ADR-022 Switchback assignment** — 2026-03-23
  - Three designs: SIMPLE_ALTERNATING, REGULAR_BALANCED, RANDOMIZED
  - Time-based assignment: block_index = floor(unix_secs / block_duration_secs)
  - Washout period exclusion (leading edge of each block)
  - Block index returned in `GetAssignmentResponse.block_index` for M2 exposure events
  - `ExposureEvent.switchback_block_index` proto field added for M4a
  - M5 STARTING validation in `switchback::validate_config`: planned_cycles >= 4, block_duration >= 1h
  - 29 tests (19 unit + 10 integration), all green
  - PR: feat(m1): ADR-022 switchback assignment

## Blocked

_None._

## Next Up

- ADR-016 GetSlateAssignment — delegate to M4b SlatePolicy, return ordered slate with slot probabilities
- ADR-013 META experiment routing — hash-based routing to variant-specific reward objectives

## Notes for Other Agents

- **M2 (Agent-2)**: `GetAssignmentResponse.block_index` (proto field 6) is now populated for SWITCHBACK
  experiments. Include it in `ExposureEvent.switchback_block_index` (proto field 12) for M4a analysis.
- **M4a (Agent-4)**: `ExposureEvent.switchback_block_index` is added. Use it to partition observations
  by block for within-switchback analysis per ADR-022.
- **M5 (Agent-5)**: `SwitchbackConfig` validation constraints are enforced in M1 at request time:
  `planned_cycles >= 4`, `block_duration_secs >= 3600`, `washout_period_secs < block_duration_secs`.
  M5 should enforce the same gates during the STARTING→RUNNING transition.
