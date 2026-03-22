# Agent-4: Statistical Analysis & Bandit Policy

You own Module 4a (Statistical Analysis Engine) and Module 4b (Bandit Policy Service). You are the most heavily loaded agent in Phase 5 — 10 ADRs touch your modules. All statistical computation and bandit algorithms live in your crates.

Language: Rust
Crates: `crates/experimentation-stats/`, `crates/experimentation-bandit/`, `crates/experimentation-analysis/`, `crates/experimentation-policy/`
Service ports: 50053 (M4a gRPC), 50054 (M4b gRPC)

## Phase 5 ADR Responsibilities

### experimentation-stats (M4a) — 9 new modules

| ADR | Module | Key Types | Golden-File Reference |
| --- | --- | --- | --- |
| 015 | `avlm.rs` | `AvlmSequentialTest` — O(1) incremental confidence sequences with regression adjustment | R `avlm` package (4 decimal places) |
| 015 P2 | `mlrate.rs` | Cross-fitted ML covariate integration (M3 trains model, M4a uses predictions) | Meta MLRATE paper examples |
| 017 P1 | `orl.rs` | TC/JIVE de-biased surrogate calibration. `SurrogateCalibrator` with cross-fold IV estimation | Netflix KDD 2024 Table 2 |
| 017 P2 | `orl.rs` | `OrlEstimator` — doubly-robust MDP estimator (Q-function + density ratio) | Netflix ICML 2024 examples |
| 018 | `evalue.rs` | `e_value_grow()`, `e_value_avlm()`, `EValueResult` | Ramdas/Wang monograph (6 decimal places) |
| 018 P3 | `mad.rs` | `MadEProcess` — e-process from uniformly-randomized bandit observations | Liang/Bojinov HBS 2024 |
| 020 | `adaptive_n.rs` | `conditional_power()`, promising-zone classification, blinded variance re-estimation | Mehta/Pocock SiM 2011 |
| 021 | `feedback_loop.rs` | `FeedbackLoopDetector` — pre/post retraining effect comparison, bias-corrected estimate | — |
| 022 | `switchback.rs` | `SwitchbackAnalyzer` — HAC (Newey-West) SE, randomization inference, carryover test | DoorDash sandwich estimator |
| 023 | `synthetic_control.rs` | Classic SCM, augmented SCM, synthetic DiD, CausalImpact. Placebo inference. | R `augsynth` (4 decimal places) |

### experimentation-bandit (M4b) — 4 LMAX core extensions

| ADR | Extension | Key Types |
| --- | --- | --- |
| 011 | Multi-objective reward | `RewardComposer` — weighted sum, epsilon-constraint (Lagrangian), Tchebycheff. `MetricNormalizer` (EMA). All on LMAX Thread 3. |
| 012 | LP constraint layer | KL-divergence minimization over constraint polytope. `LpConstraintSolver`. Population-level running counts with EMA decay. <50μs for general linear. Log adjusted **q** as `assignment_probability`. |
| 016 | Slate bandits | `SlatePolicy` — slot-wise factorized Thompson Sampling (default), GeMS VAE (behind `gpu` flag). Per-slot posteriors in `PolicyState`. Three reward attribution models. |
| 018 P3 | MAD mixing | When `mad_randomization_fraction > 0`, mix uniform selection at rate ε. Flag observations as bandit vs. uniform component. |

## Coding Standards
- **Golden files required** for every new method. Validate against reference R/Python packages.
- **Proptest invariants** for every public function:
  - `avlm.rs`: CS covers true parameter at rate ≥ (1-α) over 10K sims
  - `evalue.rs`: e-values non-negative; E[e] ≤ 1 under null
  - `adaptive_n.rs`: Type I error ≤ α after blinded re-estimation
  - `synthetic_control.rs`: donor weights non-negative, sum to 1
  - `switchback.rs`: HAC SE ≥ naive SE
  - LP constraints: q satisfies all constraints; KL(q||p) minimal
  - Multi-objective: normalized rewards converge to mean≈0, var≈1
- **assert_finite!()** on every intermediate floating-point result.
- Run `cargo test -p experimentation-stats` and `cargo test -p experimentation-bandit` before PR.
- Write status to `docs/coordination/status/agent-4-status.md`.

## Dependencies on Other Agents
- Agent-Proto: All new proto types must land before implementation.
- Agent-3 (M3): Provider metrics, user_trajectories, MLRATE predictions — coordinate on Delta Lake schemas.
- Agent-1 (M1): SelectArm and GetSlateAssignment response contracts.
- Agent-5 (M5): OnlineFdrController delegates e-value computation to M4a; adaptive N triggers request conditional power from M4a.
- Agent-6 (M6): AVLM confidence sequence rendering, e-value display, switchback/SCM results tabs.

## Contract Tests to Write
- M4a ↔ M3: Provider metric wire-format
- M4a ↔ M5: Adaptive N conditional power request/response
- M4a ↔ M5: E-value submission for OnlineFdrController
- M4a ↔ M6: AVLM results format, e-value display format
- M4b ↔ M1: LP constraint adjusted probabilities
- M4b ↔ M1: Slate assignment roundtrip
- M4b ↔ M1: Meta-experiment variant-specific policy routing

## Priority Order (within Agent-4 work)
1. **P0**: ADR-015 (AVLM) — #1 ROI item
2. **P0**: ADR-017 P1 (TC/JIVE) — corrects theoretical error
3. **P1**: ADR-018 P1 (e-values alongside p-values)
4. **P1**: ADR-021 (feedback loop detection)
5. **P2**: ADR-011 (multi-objective reward on LMAX)
6. **P2**: ADR-020 (adaptive N)
7. **P3**: ADR-012 (LP constraints)
8. **P3**: ADR-022 (switchback HAC)
9. **P3**: ADR-023 (synthetic control)
10. **P3**: ADR-016 (slate bandits)
11. **P4**: ADR-017 P2 (full ORL/MDP)
12. **P4**: ADR-018 P3 (MAD e-processes)
