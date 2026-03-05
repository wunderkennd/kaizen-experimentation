//! Statistical analysis engine.
//!
//! All statistical computation for the platform lives here.
//! M4a (Analysis Service) is a thin gRPC shell around this crate.
//!
//! # Design principles
//! - **Fail-fast**: NaN, Infinity, overflow → immediate panic with context.
//! - **Golden tests**: Every method validated against R/scipy to 6 decimal places.
//! - **Property-based**: proptest invariants for all public functions.
//!
//! # Modules
//! - `ttest` — Welch's t-test, z-test for proportions
//! - `srm` — Sample Ratio Mismatch chi-squared test
//! - `cuped` — CUPED variance reduction
//! - `delta_method` — Delta method for ratio metrics
//! - `bootstrap` — BCa bootstrap confidence intervals
//! - `sequential` — mSPRT and Group Sequential Tests
//! - `multiple_comparison` — Holm-Bonferroni, Benjamini-Hochberg
//! - `novelty` — Exponential decay fitting for novelty effects
//! - `interference` — Jensen-Shannon divergence, Jaccard, Gini
//! - `clustering` — Clustered standard errors for session-level experiments

pub mod ttest;
pub mod srm;
pub mod cuped;
// Stubs — implement in Phase 1/2:
// pub mod delta_method;
// pub mod bootstrap;
// pub mod sequential;
// pub mod multiple_comparison;
// pub mod novelty;
// pub mod interference;
// pub mod clustering;
