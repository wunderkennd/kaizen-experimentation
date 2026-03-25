# Merge Queue Run — 2026-03-25

## Summary

Processed 43 open PRs. Reviewed all 37 with green CI, merged 8, flagged 9 for fixes, approved 19 pending rebase.

## Merged (8 PRs)

| PR | Title | Priority |
|----|-------|----------|
| #238 | docs(agent-7): ADR-024 verification pass — all 26 tests green | P0 |
| #237 | docs(experimentation-stats): ADR-015 AVLM decision record + adaptive_n fix | P0 |
| #235 | docs(agent-4): ADR-017 TC/JIVE Phase 1 verification pass | P0 |
| #241 | feat(experimentation-stats): complete ADR-021 feedback loop API | P1 |
| #231 | feat(m5,fdr): ADR-018 Phase 2 e-LOND OnlineFdrController | P1 |
| #260 | feat(contract-tests): Phase 5 cross-module contract tests (ADR-016/019/021) | P1 |
| #247 | feat(m3): ADR-014 provider-side metrics — SQL templates, DDL, freshness | P1 |
| #246 | feat(m2): ADR-021 composite dedup key (model_id+window) + M2→M3 contracts | P1 |

## NEEDS_WORK — Review Comments Posted (9 PRs)

| PR | Blocking Issue |
|----|----------------|
| #269 | Incomplete Table 2 golden-file coverage (Scenario C missing); proptest assertion weakening hides contract violation |
| #266 | File naming convention (PascalCase vs kebab-case); AVLM description shown for non-AVLM methods |
| #264 | Emit() silently discards Kafka publish errors; training request fires before metric validation |
| #230 | Missing DoorDash sandwich estimator golden files (required: 4 decimal places) |
| #263 | Missing assert_finite!() and proptest; duplicates PR #245 |
| #242 | Missing proptest invariants for slate bandit public functions |
| #256 | experimentation-bandit and rand in [dependencies] instead of [dev-dependencies] |
| #274 | Stray adaptive_n.rs change; pnpm-lock.yaml from scratch; conflicts with #232 |
| #232 | Superseded by #274; palette date wrong |

## Approved — Needs Rebase (19 PRs)

All passed code review but have merge conflicts from cascading merges. Authors need to rebase onto main.

#262, #243, #239, #261, #245, #240, #255, #270, #268, #267, #257, #248, #253, #252, #250, #258, #259, #244, #229

## Skipped (6 PRs)

| PR | Reason |
|----|--------|
| #272 | RED CI — claude-review failure (human review required) |
| #271 | RED CI — claude-review failure (human review required) |
| #273 | CI status OTHER/PENDING |
| #265 | CI status OTHER/PENDING |
| #249 | CI status OTHER/PENDING |
| #254 | No CI checks reported |

## Recommendations

1. **Priority rebases**: #262 (P0 ADR-015), #243 (ADR-023), #245 (ADR-012) should be rebased first
2. **Close #232** in favor of #274 (superset implementation)
3. **Close #263** in favor of #245 (better standards compliance)
4. **#236** (ADR-024 design record) — approved but has merge conflicts, needs rebase
