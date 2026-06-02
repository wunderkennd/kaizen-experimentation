---
type: "query"
date: "2026-06-01T17:19:09.367996+00:00"
question: "Why does welch_ttest() connect Ttest to TOST (ADR-027), Sequential mSPRT, Ttest Golden, SRM, CATE, and Stats Bench?"
contributor: "graphify"
source_nodes: ["welch_ttest", "ttest.rs", "tost.rs", "welch_standard_error", "analyze_cate", "compute_welch_se", "msprt_normal", "srm_check", "assert_finite", "bench_welch_ttest"]
---

# Q: Why does welch_ttest() connect Ttest to TOST (ADR-027), Sequential mSPRT, Ttest Golden, SRM, CATE, and Stats Bench?

## Answer

Expanded from original query via vocab: [welch, ttest, tost, sprt, sequential, srm, cate, bench, golden, equivalence, variance, effect]. welch_ttest() at ttest.rs:42 is the architectural floor for mean-comparison: (1) Ttest Golden community 107 validates it to 6 decimal places against R's t.test(); (2) TOST community 35 REIMPLEMENTS the welch standard error at tost.rs:396 as welch_standard_error() instead of calling welch_ttest() - the graph clustered welch_standard_error into community 254 (Ttest) not 35 (TOST), flagging a misfile; (3) CATE community 25 pools subgroups then calls compute_welch_se() per subgroup with B-H correction; test_global_ate_matches_pooled_ttest() asserts CATE matches a single Welch run; (4) Sequential mSPRT community 5 shares the crate root with welch_ttest - mSPRT is the anytime-valid analog using the same variance estimator with a different stopping rule; (5) SRM community 268 gates Welch (randomization check first), sharing the assert_finite() fail-fast invariant; (6) Stats Bench community 250 measures perf with bench_welch_ttest(). The 6 communities aren't independent - they are four refactorings of the same Welch primitive (validated, reimplemented in TOST, wrapped in CATE, generalized in mSPRT) plus two gates (SRM, finite-check) and one bench. Refactor opportunity: move welch_standard_error() from tost.rs:396 into ttest.rs and have TOST call it.

## Source Nodes

- welch_ttest
- ttest.rs
- tost.rs
- welch_standard_error
- analyze_cate
- compute_welch_se
- msprt_normal
- srm_check
- assert_finite
- bench_welch_ttest
- tost_equivalence_test
- ttest_golden.rs
- cate.rs
- sequential.rs