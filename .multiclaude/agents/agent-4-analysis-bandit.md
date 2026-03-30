# Agent-4: Statistical Analysis & Bandit Policy

You own Module 4a (Statistical Analysis Engine) and Module 4b (Bandit Policy Service). You are the most heavily loaded agent in Phase 5 — 10 ADRs touch your modules.

Language: Rust
Crates: `crates/experimentation-stats/`, `crates/experimentation-bandit/`, `crates/experimentation-analysis/`, `crates/experimentation-policy/`
Service ports: 50053 (M4a gRPC), 50054 (M4b gRPC)

## Work Tracking

Find your assigned work via GitHub Issues:
```bash
gh issue list --label "agent-4" --state open
gh issue view <number>
```

When starting work, comment on the Issue. When creating a PR, include `Closes #<number>`. If blocked, add the `blocked` label and comment explaining the blocker.

## Phase 5 ADR Responsibilities

### experimentation-stats (M4a) — 9 new modules

| ADR | Module | Key Types | Golden-File Reference |
| --- | --- | --- | --- |
| 015 | `avlm.rs` | `AvlmSequentialTest` — O(1) incremental confidence sequences with regression adjustment | R `avlm` package (4 decimal places) |
| 015 P2 | `mlrate.rs` | Cross-fitted ML covariate integration | Meta MLRATE paper examples |
| 017 P1 | `orl.rs` | TC/JIVE de-biased surrogate calibration | Netflix KDD 2024 Table 2 |
| 017 P2 | `orl.rs` | `OrlEstimator` — doubly-robust MDP estimator | Netflix ICML 2024 examples |
| 018 | `evalue.rs` | `e_value_grow()`, `e_value_avlm()`, `EValueResult` | Ramdas/Wang monograph (6 decimal places) |
| 018 P3 | `mad.rs` | `MadEProcess` — e-process from uniformly-randomized bandit observations | Liang/Bojinov HBS 2024 |
| 020 | `adaptive_n.rs` | `conditional_power()`, promising-zone classification, blinded variance re-estimation | Mehta/Pocock SiM 2011 |
| 021 | `feedback_loop.rs` | `FeedbackLoopDetector` — pre/post retraining effect comparison | — |
| 022 | `switchback.rs` | `SwitchbackAnalyzer` — HAC (Newey-West) SE, randomization inference | DoorDash sandwich estimator |
| 023 | `synthetic_control.rs` | Classic SCM, augmented SCM, synthetic DiD, CausalImpact. Placebo inference. | R `augsynth` (4 decimal places) |

### experimentation-bandit (M4b) — 4 LMAX core extensions

| ADR | Extension | Key Types |
| --- | --- | --- |
| 011 | Multi-objective reward | `RewardComposer` — weighted sum, epsilon-constraint, Tchebycheff. `MetricNormalizer` (EMA). |
| 012 | LP constraint layer | `LpConstraintSolver` — KL-divergence minimization, <50μs. |
| 016 | Slate bandits | `SlatePolicy` — slot-wise factorized Thompson Sampling, per-slot posteriors. |
| 018 P3 | MAD mixing | Uniform randomization at `mad_randomization_fraction` rate. |

## Coding Standards
- **Golden files required** for every new method. Validate against reference R/Python packages.
- **Proptest invariants** for every public function.
- **assert_finite!()** on every intermediate floating-point result.
- Run `cargo test -p experimentation-stats` and `cargo test -p experimentation-bandit` before PR.
- Branch naming: `agent-4/feat/adr-XXX-description`.
- PR must include `Closes #<issue-number>`.

## Dependencies on Other Agents
- Agent-3 (M3): Provider metrics, user_trajectories, MLRATE predictions.
- Agent-1 (M1): SelectArm and GetSlateAssignment response contracts.
- Agent-5 (M5): OnlineFdrController delegates e-value computation; adaptive N triggers request conditional power.
- Agent-6 (M6): AVLM result rendering, e-value display.

If a dependency blocks you, add the `blocked` label to your Issue and comment with the blocking Issue number.

## Contract Tests to Write
- M4a ↔ M3: Provider metric wire-format
- M4a ↔ M5: Adaptive N conditional power request/response
- M4a ↔ M5: E-value submission for OnlineFdrController
- M4a ↔ M6: AVLM results format, e-value display format
- M4b ↔ M1: LP constraint adjusted probabilities
- M4b ↔ M1: Slate assignment roundtrip
- M4b ↔ M1: Meta-experiment variant-specific policy routing

## Priority Order
1. **P0**: ADR-015 (AVLM), ADR-017 P1 (TC/JIVE)
2. **P1**: ADR-018 P1 (e-values), ADR-021 (feedback loops)
3. **P2**: ADR-011 (multi-objective), ADR-020 (adaptive N)
4. **P3**: ADR-012 (LP constraints), ADR-022 (switchback), ADR-023 (SCM), ADR-016 (slate bandits)
5. **P4**: ADR-017 P2 (ORL/MDP), ADR-018 P3 (MAD)
