# ADR-005: Component State Machine with Transitional States

**Status**: Accepted
**Date**: 2026-03-03

---

## Context
NautilusTrader defines both stable and transitional states for all components, preventing race conditions during state changes. Our experiment lifecycle has a gap: between "start experiment" and "experiment is running," several asynchronous validations must complete (config validation, bandit warm-up, metric availability check, segment power check). Similarly, between "conclude experiment" and "results available," final analysis must complete. Without transitional states, clients can observe inconsistent states.

## Decision
Add two transitional states to the experiment lifecycle:

- **STARTING** (between DRAFT and RUNNING): M5 orchestrates validation. M1 Assignment Service MUST NOT serve assignments for experiments in STARTING state. M6 UI shows a validation checklist. If validation fails, experiment returns to DRAFT with error details.
- **CONCLUDING** (between RUNNING and CONCLUDED): M4a runs final analysis, computes surrogate projections, generates IPW estimates. M6 UI shows a progress indicator. Result API queries return 503 (Service Unavailable) during CONCLUDING to prevent partial result consumption.

## Alternatives Considered
- **Boolean flags (is_validated, is_analysis_complete)**: Spreads state management across multiple fields. Easy to create inconsistent combinations. Single enum state is authoritative.
- **Async job tracking (separate jobs table)**: Adds complexity. The experiment state already serves as the coordination point — adding a separate job tracking system creates two sources of truth.
- **Immediate transitions (DRAFT→RUNNING, RUNNING→CONCLUDED)**: Current approach. Race condition: M1 could serve assignments before validation completes. PM could query results before analysis finishes.

## Consequences
- All services must check experiment state before acting. M1 filters STARTING experiments from assignment serving. M4a only runs analysis on RUNNING or CONCLUDING experiments.
- State transitions are atomic (PostgreSQL UPDATE with state precondition check).
- Audit trail records every state transition with timestamps and actor identity.
