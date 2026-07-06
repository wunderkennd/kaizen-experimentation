<!-- GENERATED from docs/agents/registry/agent-4.md by scripts/gen_agents.py — DO NOT EDIT.
     Edit the registry concept, then run `just gen-agents`. -->
# Agent-4: Statistical Analysis & Bandit Policy (M4a + M4b)

Owns all statistical computation (experimentation-stats) and the bandit policy service (Thompson, LinUCB, Neural, LMAX core).

- **Language**: Rust
- **Ports**: 50053, 50054
- **Owned paths**: `crates/experimentation-stats/`, `crates/experimentation-bandit/`, `crates/experimentation-analysis/`, `crates/experimentation-policy/`
- **Depends on**: agent-1, agent-3, agent-5, agent-6
- **Work queue**: `gh issue list --label "agent-4" --state open` (claim protocol: `scripts/orchestration/README.md`)

Canonical identity & charter: [`docs/agents/registry/agent-4.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-4.md) · Repo context anchor: [`CLAUDE.md`](https://github.com/wunderkennd/kaizen-experimentation/blob/main/CLAUDE.md)

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

- [agent-3](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-3.md): provider metrics, `user_trajectories`, MLRATE predictions.
- [agent-1](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-1.md): SelectArm / GetSlateAssignment response contracts.
- [agent-5](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-5.md): FDR controller delegation; adaptive-N triggers.
- [agent-6](https://github.com/wunderkennd/kaizen-experimentation/blob/main/docs/agents/registry/agent-6.md): result-rendering formats.

## Work tracking

`gh issue list --label "agent-4" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
