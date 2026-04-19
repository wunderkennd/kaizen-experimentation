# ADR-004: Group Sequential Tests Alongside mSPRT

**Status**: Accepted
**Date**: 2026-03-03

---

## Context
The platform already supports mSPRT (always-valid inference with arbitrary peeking). Spotify Confidence offers both always-valid approaches and group sequential tests (GSTs), reasoning that GSTs are more powerful when teams can pre-commit to a fixed analysis schedule. Our platform serves teams with different monitoring patterns.

## Decision
Offer both sequential testing methods in M4a, selected per-experiment via `SequentialTestConfig`:

- **mSPRT**: For teams that check dashboards continuously and want to stop experiments at any time. Lower power (pays for arbitrary peeking flexibility).
- **GST with O'Brien-Fleming spending**: For teams with a fixed weekly review cadence (e.g., "we'll look every Monday for 4 weeks"). Alpha concentrated at later looks — conservative early stopping, maximum power at final look.
- **GST with Pocock spending**: For teams wanting equal stopping probability at each look. Moderate power.

## Alternatives Considered
- **mSPRT only**: Simpler, but wastes statistical power for teams that can commit to a schedule. A typical GST with 4 looks recovers ~15-20% of the power lost to always-valid correction.
- **GST only**: Would force teams to pre-commit to a schedule, which is impractical for exploratory experiments or teams that don't have regular review cadences.
- **Bayesian stopping rules**: Already supported as a separate analysis mode. Bayesian stopping does not control frequentist Type I error, which many organizations require for regulatory or governance reasons.

## Consequences
- M4a must implement spending functions (O'Brien-Fleming, Pocock) and track alpha expenditure across looks.
- M5 must validate that `planned_looks >= 2` for GST experiments.
- M6 UI must render distinct visualizations: confidence sequences for mSPRT, boundary-crossing plots for GST.
- GST boundaries validated against R's gsDesign package to 4 decimal places in CI.
