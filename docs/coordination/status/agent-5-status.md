# Agent-5 Status — Phase 5

**Module**: M5 Management
**Last updated**: 2026-03-24

## Current Sprint

Sprint: 5.3
Focus: ADR-025 Phase 3 — M5 Rust port statistical integration
Branch: work/lively-penguin
PR: pending

## In Progress

- [x] ADR-025 Phase 3 — `experimentation-management` Rust crate, Phase 5 statistical integration (this PR)

## Completed (Phase 5)

- [x] ADR-020 Adaptive Sample Size (PR #227, merged)
  - `crates/experimentation-stats/src/adaptive_n.rs`: blinded_pooled_variance, conditional_power, zone_classify, gst_reallocate_spending, required_n_for_power, run_interim_analysis
  - `sql/migrations/008_adaptive_sample_size_audit.sql`
  - Go M5 scheduler in `services/management/internal/adaptive/`

- [x] ADR-025 Phase 3 — M5 Rust port, Phase 5 statistical integration (this PR)
  - **`crates/experimentation-stats/src/portfolio.rs`** (new): `optimal_alpha()` (Bonferroni), `annualized_impact()` — 12 unit tests
  - **`crates/experimentation-management/`** (new crate):
    - `src/fdr.rs`: `OnlineFdrController` — wraps `e_value_grow` + `e_value_avlm`, sqlx state persistence in `fdr_controller_state` + `fdr_controller_audit`
    - `src/portfolio.rs`: `enrich_portfolio_allocation()` — wraps `optimal_alpha`, `annualized_impact`, `conditional_power`
    - `src/adaptive_n.rs`: `run_adaptive_interim()` — wraps `conditional_power` + `zone_classify`, zone→extension decision
    - `tests/phase3_integration.rs`: 16 cross-module integration tests, all pure (no DB)
  - **`sql/migrations/009_fdr_controller.sql`** (new): FDR state + audit tables
  - All 38 tests pass (22 unit + 16 integration, 3 DB tests `#[ignore]`)
  - `cargo test -p experimentation-management`: **ok**
  - Workspace Cargo.toml updated to include the new crate

## Decision trigger (ADR-025)

The threshold of ≥3 ADRs committed is met:
- ADR-018 (E-Values / OnlineFdrController) — **committed**
- ADR-019 (Portfolio Optimization) — **committed**
- ADR-020 (Adaptive Sample Size) — **committed** (PR #227)

This PR implements ADR-025 Phase 3 and unblocks the Agent-4 dependency: M5 no longer
needs the `ConditionalPowerClient` gRPC interface. All statistical calls are direct
function calls to `experimentation-stats`.

## Blocked

None — no M4a gRPC dependency for Phase 3 statistical calls.

## Next Up

- ADR-025 Phase 4: port contract tests (11 M5-M6 + 10 M1-M5 wire-format tests)
- ADR-013 META experiment type validation in `experimentation-management::lifecycle`
- ADR-022 Switchback config validation
