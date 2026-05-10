# Issue #511 — Post-Processing Branch Type-Gates

- **Date:** 2026-05-07
- **Status:** Approved
- **Author:** Kenneth Sylvain + Claude Code
- **Implements:** [Issue #511](https://github.com/wunderkennd/kaizen-experimentation/issues/511) (option a — type-gate; option b deferred to future ADR)
- **Builds on:** [PR #510](https://github.com/wunderkennd/kaizen-experimentation/pull/510) (issue #504, M3 MetricConfig wiring)

## Context

PR #510 wired ADR-026 Phase 1 metric types (`FILTERED_MEAN`, `COMPOSITE`, `WINDOWED_COUNT`) end-to-end through `services/metrics/internal/jobs/standard.go`. **Side effect:** these types are now reachable from the four post-processing branches in `Run`:

1. **CUPED covariate** (lines ~162–206) — fires when `m.CupedCovariateMetricID != "" && exp.StartedAt != ""`.
2. **MLRATE cross-fit** (lines ~208–223) — fires when `exp.MLRATEEnabled && len(m.MLRATEFeatureEventTypes) > 0 && m.MLRATEModelURI != ""`.
3. **Session-level** (lines ~226–258) — fires when `exp.SessionLevel && !m.IsQoEMetric`.
4. **Lifecycle** (lines ~261–293) — fires when `exp.LifecycleStratificationEnabled && !m.IsQoEMetric`.

All four branches reuse `params` and call SQL templates that **assume the legacy MEAN-style column convention** — `me.value` for the metric value and `me.event_type = '{{.SourceEventType}}'` for the row filter. The new types break this assumption:

| Type | Mismatch |
| --- | --- |
| `FILTERED_MEAN` | uses `me.{{.ValueColumn}}`, not `me.value` — column may not exist |
| `COMPOSITE` | reads from `delta.metric_summaries`, not `delta.metric_events` — `me.event_type = 'n/a'` matches zero rows |
| `WINDOWED_COUNT` | counts rows, doesn't average values — `AVG()` against COUNT semantics is meaningless |

**Today's risk is bounded** — no current seed file or staging environment combines `session_level: true` / `lifecycle_stratification_enabled: true` / `cuped_covariate_metric_id` / `mlrate_feature_event_types` with a new-type metric. But the moment one is, production data is silently wrong.

## Decision

**Adopt option (a) from issue #511: type-gate the four post-processing branches with an `isLegacyStyle` predicate, emit a single-line `slog.Info` skip message for non-legacy types, and continue the loop.**

### Rationale

Option (b) — proper per-type post-processing — is the right long-term shape but requires designing 12 new SQL templates (3 new types × 4 branches) plus the M5 validation that determines which combinations are legal. That's an ADR-scale exercise; gating buys time at the cost of one helper function and four if/else wraps.

The type-gate has the right failure mode: a real-world misconfiguration (someone enables `session_level: true` on an experiment whose secondary metric is COMPOSITE) emits a clear `slog.Info` instead of producing silently-wrong rows. On-call engineers can grep for the skip message and route the request to whoever owns option (b).

## Architecture

### 1. `isLegacyStyle` helper

Add to `services/metrics/internal/jobs/standard.go` near the existing `toSparkOperands` helper (file-private, lowercase first letter):

```go
// isLegacyStyle reports whether a MetricType uses the legacy MEAN-style
// column convention (me.value, me.event_type = '{{.SourceEventType}}')
// that the post-processing templates (cuped_covariate, session_level_mean,
// lifecycle_mean, mlrate_*) assume.
//
// ADR-026 Phase 1 types (FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT) use
// different column conventions and must skip post-processing until those
// templates grow per-type variants (issue #511 option b).
func isLegacyStyle(metricType string) bool {
    switch strings.ToUpper(metricType) {
    case "MEAN", "PROPORTION", "COUNT", "RATIO", "PERCENTILE", "CUSTOM":
        return true
    }
    return false
}
```

### 2. Four post-processing gates

For each of the four branches, wrap the existing body in a nested if/else. Skip path emits a one-line `slog.Info` and falls through (no `continue` — other branches still get a chance):

```go
// Pattern applied to all 4 branches:
if <existing-trigger-condition> {
    if !isLegacyStyle(m.Type) {
        slog.Info("skipping <branch-name>: legacy column convention not supported for this metric type",
            "metric_id", m.MetricID,
            "type", m.Type,
        )
    } else {
        // existing branch body unchanged
    }
}
```

Branch-specific skip messages:

| Branch | Skip message |
| --- | --- |
| CUPED covariate | `"skipping CUPED covariate: legacy column convention not supported for this metric type"` |
| MLRATE cross-fit | `"skipping MLRATE cross-fit: legacy column convention not supported for this metric type"` |
| Session-level | `"skipping session-level metric: legacy column convention not supported for this metric type"` |
| Lifecycle | `"skipping lifecycle metric: legacy column convention not supported for this metric type"` |

### Design choices

| Choice | Decision | Why |
| --- | --- | --- |
| Helper location | Package-level unexported function in `standard.go` | Testable in isolation; matches `toSparkOperands` placement |
| Helper input type | `string` (not a typed enum) | Matches the existing `MetricConfig.Type` field shape and the renderer's `RenderForType(metricType string, ...)` signature |
| Case sensitivity | `strings.ToUpper` | Matches the renderer's existing case-normalization pattern |
| Skip behavior | `slog.Info` + fall through (NOT `continue`) | Each branch is independent — skipping CUPED shouldn't suppress session-level. Falling through preserves that |
| Skip log level | `Info` (not `Warn` or `Error`) | This is expected behavior, not degraded behavior — the metric's primary SQL still ran. Reserving `Warn` for the existing `RenderForType` failure path keeps log-level signal meaningful |
| MLRATE inclusion | Yes | Same column-convention mismatch; same fix shape. Avoids a future #511.5 |
| QoE branch | Already gated by `!m.IsQoEMetric` — leave unchanged | QoE is a separate template family with its own column conventions; not in #511's scope |

### 3. Testing

Modify `services/metrics/internal/config/testdata/seed_adr026_phase1.json`:

- Enable `session_level: true` on the experiment.
- Enable `lifecycle_stratification_enabled: true` on the experiment.
- Add `cuped_covariate_metric_id: "watch_time_minutes"` to one of the new-type metrics (the gate fires before we look up the covariate, so referencing a non-existent metric is fine — but using a real legacy one keeps the fixture consistent).
- Add a fifth metric to the experiment: a legacy `MEAN` metric named `watch_time_minutes` with the standard MEAN config. This proves post-processing still runs for legacy types in the same experiment.
- Reference the legacy metric from `secondary_metric_ids` so it's actually computed.

The MLRATE branch requires several extra fields (`mlrate_enabled`, `mlrate_feature_event_types`, `mlrate_model_uri`). To keep the fixture compact, MLRATE coverage is verified by **direct unit test** of `isLegacyStyle` rather than by the e2e fixture. The end-to-end fixture covers CUPED, session-level, and lifecycle.

Extend `TestStandardJob_Run_ADR026Phase1_NewTypes` in `services/metrics/internal/jobs/standard_test.go`:

- New subtest **`new types skip legacy post-processing`**: filter `qlWriter.AllEntries()` for new-type metric IDs (`mobile_avg_watch_time`, `composite_engagement`, `stream_starts_24h`); assert ZERO entries with `JobType` ∈ `{session_level_metric, lifecycle_metric, cuped_covariate}`.
- New subtest **`legacy types still run post-processing`**: filter for the legacy `watch_time_minutes` metric ID; assert it produces entries with the post-processing `JobType` values (proves the gate doesn't break legacy behavior).
- The existing 3 subtests (filtered_mean / composite / windowed_count primary SQL) continue to assert the new types produce their main `daily_metric` SQL — proving the gate doesn't break primary computation.

Add a **direct unit test for `isLegacyStyle`** in `standard_test.go`:

```go
func TestIsLegacyStyle(t *testing.T) {
    legacy := []string{"MEAN", "PROPORTION", "COUNT", "RATIO", "PERCENTILE", "CUSTOM",
        "mean", "ratio", "Custom"} // case-insensitive
    nonLegacy := []string{"FILTERED_MEAN", "COMPOSITE", "WINDOWED_COUNT",
        "filtered_mean", "", "UNKNOWN_TYPE"}
    for _, t_ := range legacy {
        assert.True(t, isLegacyStyle(t_), "%q should be legacy", t_)
    }
    for _, t_ := range nonLegacy {
        assert.False(t, isLegacyStyle(t_), "%q should not be legacy", t_)
    }
}
```

This exercises MLRATE coverage indirectly (via the helper) and locks down case-insensitivity.

## Out of scope

| Concern | Tracked elsewhere |
| --- | --- |
| Per-type post-processing (option b — proper SQL templates) | #511 (this issue's option b — defer to future ADR) |
| M5 validation of legal feature combinations (e.g. "can a COMPOSITE have a CUPED covariate?") | #433 |
| Loader refactor to consume proto MetricDefinition | #506 |
| Future Option B proto migration | #476 |

## Decisions log

| Decision | Choice | Rationale |
| --- | --- | --- |
| Option a vs b | a (gate) | b is ADR-scale; a is small fast-follow that closes the production-risk gap |
| Include MLRATE in gate set | Yes | Same column-convention mismatch as the other 3 branches; same fix shape |
| Helper return type | `bool` | Matches Go-idiomatic `is*` predicate pattern |
| Skip log level | `Info` | Expected behavior, not degraded — primary SQL still runs |
| Skip behavior | Fall through (no `continue`) | Branches are independent |
| MLRATE e2e coverage | Direct unit test of `isLegacyStyle` | Keeps fixture compact; full e2e MLRATE setup requires too many extra fields |

## References

- Issue #511 — primary deliverable
- PR #510 / Issue #504 — the wiring that made these branches reachable for new types
- `services/metrics/internal/jobs/standard.go` — the four post-processing branches
- `services/metrics/internal/config/testdata/seed_adr026_phase1.json` — fixture from PR #510 to extend
- `services/metrics/internal/jobs/standard_test.go` — test file with `TestStandardJob_Run_ADR026Phase1_NewTypes` to extend
