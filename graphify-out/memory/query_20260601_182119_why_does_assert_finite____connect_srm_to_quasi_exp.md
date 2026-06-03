---
type: "query"
date: "2026-06-01T18:21:19.449458+00:00"
question: "Why does assert_finite!() connect Srm to Quasi-Experimental, TOST, Sequential mSPRT, Orl, Ttest?"
contributor: "graphify"
source_nodes: ["assert_finite", "assert_finite_fail_fast_invariant", "fail_fast.rs", "experimentation-core/src/error.rs", "srm.rs", "tost.rs", "ttest.rs", "sequential.rs", "cate.rs", "orl.rs"]
---

# Q: Why does assert_finite!() connect Srm to Quasi-Experimental, TOST, Sequential mSPRT, Orl, Ttest?

## Answer

Expanded via vocab: [assert, finite, invariant, nan, inf, panic, srm, tost, sprt, orl, ttest, synthetic]. The assert_finite!() concept node bridges those communities because every statistical method in experimentation-stats imports it from experimentation-core::error. 545 call sites across 8 crates: 427 in stats, 47 in analysis, 38 in bandit, 18 in management, 7 in ingest, 4 in assignment, 3 in policy, 1 in core. Sits at high betweenness because it's the architectural floor under every numeric computation. KEY GRAPH FINDING: 8 separate nodes were extracted for 'assert_finite' from different files - investigating revealed crates/experimentation-stats/src/fail_fast.rs has its OWN pub fn assert_finite (plus assert_probability, assert_non_empty) with a different panic message. This file is DEAD CODE: 'mod fail_fast' is never declared in lib.rs, so cargo never compiles it. The graph isolated this orphan file in its own community (290) because the AST scanner emitted nodes but no use-edges formed. Same pattern as welch_standard_error() in tost.rs:396, but the latter is alive (TOST actively calls its own copy) while fail_fast.rs is dead. Both findings: primitives escaped their canonical home.

## Source Nodes

- assert_finite
- assert_finite_fail_fast_invariant
- fail_fast.rs
- experimentation-core/src/error.rs
- srm.rs
- tost.rs
- ttest.rs
- sequential.rs
- cate.rs
- orl.rs
- welch_standard_error