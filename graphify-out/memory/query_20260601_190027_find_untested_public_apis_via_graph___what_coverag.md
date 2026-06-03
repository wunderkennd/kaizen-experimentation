---
type: "query"
date: "2026-06-01T19:00:27.323346+00:00"
question: "Find untested public APIs via graph - what coverage gaps exist?"
contributor: "graphify"
source_nodes: ["feature_keys", "randomization_fraction", "impression_fractions", "with_reward_composer", "sigmoid", "Thompson.sample", "fail_fast.rs", "assert_finite"]
---

# Q: Find untested public APIs via graph - what coverage gaps exist?

## Answer

Graph signal alone overcounts: 149 production pub-fn nodes had 0 test-file edges, but most are tested in-file under #[cfg(test)] mod tests blocks (graph blind spot for same-file unit tests). After cross-checking with grep against in-file test blocks AND sibling test files, only 49 pub fns truly have zero direct test calls. experimentation-stats has only 3, all in dead fail_fast.rs (already filed #584) — CLAUDE.md proptest+golden requirement is effectively 100% compliant once #584 lands. Real signal landed in experimentation-bandit: 4 verified dead pub fns - LinUcbPolicy.feature_keys (redundant accessor; consumers carry their own field), MadEProcess.randomization_fraction (redundant; MadConfig field is already pub), ConstraintSolver.impression_fractions (orphan), ThompsonSamplingPolicy.with_reward_composer (builder, referenced only in rustdoc). Filed #588. KEY LIMITATION DISCOVERED: graph + grep cannot see transitive coverage (e.g., Thompson.sample is exercised through select_arm which has 4 tests; sigmoid is called by policy/core.rs which has tests). True coverage analysis requires cargo-llvm-cov, not the graph. Negative result: this thread bounded the coverage cleanup to a tiny consolidated PR rather than a workspace audit.

## Source Nodes

- feature_keys
- randomization_fraction
- impression_fractions
- with_reward_composer
- sigmoid
- Thompson.sample
- fail_fast.rs
- assert_finite