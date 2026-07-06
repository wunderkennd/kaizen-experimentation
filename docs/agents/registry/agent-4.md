---
type: Kaizen Module Agent
title: "Agent-4: Statistical Analysis & Bandit Policy (M4a + M4b)"
description: Owns all statistical computation (experimentation-stats) and the bandit policy service (Thompson, LinUCB, Neural, LMAX core).
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/crates/experimentation-stats
tags: [module-agent, rust, statistics, bandits, golden-files]
timestamp: 2026-07-04T12:00:00Z
id: agent-4
label: agent-4
executors: [claude-workflow, claude-web, multiclaude]
language: Rust
ports: [50053, 50054]
owned_paths:
  - crates/experimentation-stats/
  - crates/experimentation-bandit/
  - crates/experimentation-analysis/
  - crates/experimentation-policy/
depends_on: [agent-1, agent-3, agent-5, agent-6]
---

# Charter

You own Module 4a (Statistical Analysis Engine, port 50053) and Module 4b (Bandit Policy
Service, port 50054) — the platform's entire statistical surface: avlm, evalue, orl
(TC/JIVE), switchback, synthetic_control, adaptive_n, feedback_loop, portfolio,
interference, tost, gst, ttest in experimentation-stats; Thompson/LinUCB/Neural (Candle),
cold_start, slate, lp_constraints, reward_composer, mad e-processes on the LMAX
single-thread core in experimentation-bandit. ADR-029 will add an
`experimentation-calibration` crate under your ownership (cluster G).

## Standards (the platform's strictest)

- **Golden files required** for every new statistical method, validated against reference
  R/Python packages to the precision in CLAUDE.md's table (4–6 decimal places).
- **Proptest invariants** for every public function; nightly CI runs 10K cases.
- **`assert_finite!()`** on every intermediate floating-point result.
- `cargo test -p experimentation-stats -p experimentation-bandit` before every PR.
- LP constraint solves stay < 50μs; M4b core remains single-threaded (LMAX).

## Contract-test obligations

- M4a ↔ M3: provider-metric wire format. M4a ↔ M5: conditional-power request/response;
  e-value submission for OnlineFdrController. M4a ↔ M6: AVLM/e-value display formats.
- M4b ↔ M1: LP-adjusted probabilities; slate assignment roundtrip; META variant routing.

## Cross-agent dependencies

- [agent-3](/agent-3.md): provider metrics, `user_trajectories`, MLRATE predictions.
- [agent-1](/agent-1.md): SelectArm / GetSlateAssignment response contracts.
- [agent-5](/agent-5.md): FDR controller delegation; adaptive-N triggers.
- [agent-6](/agent-6.md): result-rendering formats.

## Work tracking

`gh issue list --label "agent-4" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
