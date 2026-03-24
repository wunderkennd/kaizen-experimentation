//! Experiment Management Service — Rust port (ADR-025 Phase 3).
//!
//! Phase 3 adds direct `experimentation-stats` integration for three ADRs:
//!
//! - [`fdr`]: `OnlineFdrController` (ADR-018) — sequential FDR control via
//!   e-values.  State persisted in PostgreSQL.  Called on experiment conclusion.
//!
//! - [`portfolio`]: Portfolio allocation enrichment (ADR-019) — optimal alpha
//!   recommendation and annualized impact estimation for the portfolio dashboard.
//!
//! - [`adaptive_n`]: Adaptive sample size trigger (ADR-020) — scheduled interim
//!   analysis that classifies the experiment into Favorable/Promising/Futile and
//!   extends it when in the Promising zone.
//!
//! All statistical computation is direct function calls to `experimentation-stats`
//! — no gRPC RPC, no FFI.  This is the architectural payoff of ADR-025.

pub mod adaptive_n;
pub mod fdr;
pub mod portfolio;
