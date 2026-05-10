# Issue #511 Post-Processing Type-Gates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate the four post-processing branches in `services/metrics/internal/jobs/standard.go` (CUPED, MLRATE, session-level, lifecycle) behind an `isLegacyStyle` predicate so ADR-026 Phase 1 metric types (FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT) cleanly skip post-processing instead of silently producing wrong SQL.

**Architecture:** A small file-private helper `isLegacyStyle(string) bool` returns true for the 6 legacy types; each of the 4 post-processing branches wraps its existing body in a nested if/else that emits a single-line `slog.Info` skip message for non-legacy types. See spec at `docs/superpowers/specs/2026-05-07-issue-511-post-processing-type-gates.md`.

**Tech Stack:** Go 1.22+, `log/slog`, existing `strings` import already in `standard.go`. `github.com/stretchr/testify` for assertions.

**Worktree:** `.claude/worktrees/issue-511-post-processing-gates/` on branch `agent-3/fix/issue-511-post-processing-type-gates` (off `origin/main`). Spec committed as `3f52f9a`.

---

## File Structure

| File | Action | Responsibility |
| --- | --- | --- |
| `services/metrics/internal/jobs/standard.go` | Modify | Add `isLegacyStyle` helper + wrap 4 post-processing branches in type-gates |
| `services/metrics/internal/jobs/standard_test.go` | Modify | Add `TestIsLegacyStyle` direct unit test + extend `TestStandardJob_Run_ADR026Phase1_NewTypes` with skip-verification subtests |
| `services/metrics/internal/config/testdata/seed_adr026_phase1.json` | Modify | Enable `session_level: true` + `lifecycle_stratification_enabled: true` on experiment; add `cuped_covariate_metric_id` to one new-type metric; add a legacy MEAN metric to verify post-processing still runs for legacy types |

Total expected diff: ~25 lines impl + ~50 lines test + ~15 lines fixture.

---

## Task 1: Add `isLegacyStyle` helper with TDD

**Files:**
- Modify: `services/metrics/internal/jobs/standard.go`
- Modify: `services/metrics/internal/jobs/standard_test.go`

TDD: write the failing test first (compile error), then add the helper.

- [ ] **Step 1.1: Write the failing test**

Append to `services/metrics/internal/jobs/standard_test.go`:

```go
func TestIsLegacyStyle(t *testing.T) {
	t.Run("legacy types return true", func(t *testing.T) {
		legacy := []string{
			"MEAN", "PROPORTION", "COUNT", "RATIO", "PERCENTILE", "CUSTOM",
			"mean", "proportion", "count", "ratio", "percentile", "custom",
			"Custom", "Ratio",
		}
		for _, mt := range legacy {
			assert.True(t, isLegacyStyle(mt), "%q should be legacy", mt)
		}
	})

	t.Run("ADR-026 Phase 1 types return false", func(t *testing.T) {
		nonLegacy := []string{
			"FILTERED_MEAN", "COMPOSITE", "WINDOWED_COUNT",
			"filtered_mean", "composite", "windowed_count",
		}
		for _, mt := range nonLegacy {
			assert.False(t, isLegacyStyle(mt), "%q should not be legacy", mt)
		}
	})

	t.Run("unknown types return false", func(t *testing.T) {
		unknown := []string{"", " ", "UNKNOWN_TYPE", "FOO_BAR"}
		for _, mt := range unknown {
			assert.False(t, isLegacyStyle(mt), "%q should not be legacy", mt)
		}
	})
}
```

The file's existing imports already include `testing`, `assert`, `require`. No new imports needed.

- [ ] **Step 1.2: Run the test — expect compile error**

Run: `go test ./services/metrics/internal/jobs/ -run TestIsLegacyStyle -v`
Expected: COMPILE FAIL with `undefined: isLegacyStyle`.

- [ ] **Step 1.3: Add the helper to `standard.go`**

In `services/metrics/internal/jobs/standard.go`, append the helper near the bottom of the file (next to the existing `toSparkOperands` helper, after the `Run` function):

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

The `strings` package is already imported in `standard.go`; no new imports needed.

- [ ] **Step 1.4: Run the test — expect PASS**

Run: `go test ./services/metrics/internal/jobs/ -run TestIsLegacyStyle -v`
Expected: 3 subtests PASS (`legacy types return true`, `ADR-026 Phase 1 types return false`, `unknown types return false`).

- [ ] **Step 1.5: Run the existing jobs suite — expect PASS**

Run: `go test ./services/metrics/internal/jobs/...`
Expected: every existing test still passes (the helper is a new isolated function with no callers yet).

- [ ] **Step 1.6: Commit**

```bash
git add services/metrics/internal/jobs/standard.go services/metrics/internal/jobs/standard_test.go
git commit -m "feat(metrics): isLegacyStyle helper for ADR-026 Phase 1 type-gating

Adds a file-private predicate that returns true for the 6 legacy
MetricType values (MEAN, PROPORTION, COUNT, RATIO, PERCENTILE, CUSTOM)
and false for ADR-026 Phase 1 types and unknowns.

Will be used in the next commit to gate the four post-processing
branches in standard.go's Run loop. The helper is case-insensitive
to match the case-normalization the renderer's RenderForType already
applies.

Refs #511"
```

---

## Task 2: Apply type-gates to the four post-processing branches

**Files:**
- Modify: `services/metrics/internal/jobs/standard.go`

Wire the helper into the four post-processing branches in `Run`. Each gate wraps the existing branch body in a nested if/else; the skip path emits a one-line `slog.Info` and falls through (no `continue` — branches are independent).

No new tests in this task — Task 3's e2e fixture exercises the gates end-to-end.

- [ ] **Step 2.1: Gate the CUPED covariate branch**

In `services/metrics/internal/jobs/standard.go`, find the CUPED block in `Run` (currently around lines 162-206 — locate by `if m.CupedCovariateMetricID != "" && exp.StartedAt != "" {`). Wrap its existing body in a nested if/else:

```go
		// If metric has a CUPED covariate configured and experiment has a start date,
		// compute the pre-experiment covariate value for variance reduction.
		if m.CupedCovariateMetricID != "" && exp.StartedAt != "" {
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping CUPED covariate: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
				covMetric, err := j.config.GetMetric(m.CupedCovariateMetricID)
				if err != nil {
					return nil, fmt.Errorf("jobs: resolve CUPED covariate metric %s for %s: %w",
						m.CupedCovariateMetricID, m.MetricID, err)
				}

				cupedParams := params
				cupedParams.CupedEnabled = true
				cupedParams.CupedCovariateEventType = covMetric.SourceEventType
				cupedParams.ExperimentStartDate = exp.StartedAt
				cupedParams.CupedLookbackDays = defaultCupedLookbackDays

				cupedSQL, err := j.renderer.RenderCupedCovariate(cupedParams)
				if err != nil {
					return nil, fmt.Errorf("jobs: render CUPED covariate for %s: %w", m.MetricID, err)
				}

				cupedResult, err := j.executor.ExecuteAndWrite(ctx, cupedSQL, "delta.metric_summaries")
				if err != nil {
					return nil, fmt.Errorf("jobs: execute CUPED covariate for %s: %w", m.MetricID, err)
				}
				m3metrics.SparkQueryDuration.WithLabelValues("cuped_covariate").Observe(cupedResult.Duration.Seconds())
				m3metrics.SparkQueryRows.WithLabelValues("cuped_covariate").Observe(float64(cupedResult.RowCount))

				if err := j.queryLog.Log(ctx, querylog.Entry{
					ExperimentID: experimentID,
					MetricID:     m.MetricID,
					SQLText:      cupedSQL,
					RowCount:     cupedResult.RowCount,
					DurationMs:   cupedResult.Duration.Milliseconds(),
					JobType:      "cuped_covariate",
				}); err != nil {
					return nil, fmt.Errorf("jobs: log CUPED covariate query for %s: %w", m.MetricID, err)
				}

				slog.Info("computed CUPED covariate",
					"experiment_id", experimentID,
					"metric_id", m.MetricID,
					"covariate_metric_id", m.CupedCovariateMetricID,
					"rows", cupedResult.RowCount,
				)
			}
		}
```

- [ ] **Step 2.2: Gate the MLRATE cross-fit branch**

Find the MLRATE block (around lines 208-223 — locate by `if exp.MLRATEEnabled && len(m.MLRATEFeatureEventTypes) > 0 && m.MLRATEModelURI != "" && exp.StartedAt != "" {`). Wrap:

```go
		// MLRATE cross-fitting: if experiment has MLRATE enabled and metric has
		// feature config, generate K-fold cross-fitted predictions as AVLM covariates.
		if exp.MLRATEEnabled && len(m.MLRATEFeatureEventTypes) > 0 && m.MLRATEModelURI != "" && exp.StartedAt != "" {
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping MLRATE cross-fit: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
				mlrateJob := NewMLRATEJob(j.renderer, j.executor, j.queryLog)
				mlrateResult, err := mlrateJob.Run(ctx, exp, &m, computationDate)
				if err != nil {
					return nil, fmt.Errorf("jobs: MLRATE cross-fit for %s: %w", m.MetricID, err)
				}

				slog.Info("computed MLRATE cross-fitted predictions",
					"experiment_id", experimentID,
					"metric_id", m.MetricID,
					"folds", mlrateResult.Folds,
					"users_scored", mlrateResult.UsersScored,
				)
			}
		}
```

- [ ] **Step 2.3: Gate the session-level branch**

Find the session-level block (around lines 226-258 — locate by `if exp.SessionLevel && !m.IsQoEMetric {`). Wrap:

```go
		// Session-level aggregation: if enabled, also compute per-session metrics.
		if exp.SessionLevel && !m.IsQoEMetric {
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping session-level metric: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
				slParams := params
				slParams.SessionLevel = true

				slSQL, err := j.renderer.RenderSessionLevelMean(slParams)
				if err != nil {
					return nil, fmt.Errorf("jobs: render session-level metric for %s: %w", m.MetricID, err)
				}

				slResult, err := j.executor.ExecuteAndWrite(ctx, slSQL, "delta.metric_summaries")
				if err != nil {
					return nil, fmt.Errorf("jobs: execute session-level metric for %s: %w", m.MetricID, err)
				}
				m3metrics.SparkQueryDuration.WithLabelValues("session_level_metric").Observe(slResult.Duration.Seconds())
				m3metrics.SparkQueryRows.WithLabelValues("session_level_metric").Observe(float64(slResult.RowCount))

				if err := j.queryLog.Log(ctx, querylog.Entry{
					ExperimentID: experimentID,
					MetricID:     m.MetricID,
					SQLText:      slSQL,
					RowCount:     slResult.RowCount,
					DurationMs:   slResult.Duration.Milliseconds(),
					JobType:      "session_level_metric",
				}); err != nil {
					return nil, fmt.Errorf("jobs: log session-level metric query for %s: %w", m.MetricID, err)
				}

				slog.Info("computed session-level metric",
					"experiment_id", experimentID,
					"metric_id", m.MetricID,
					"rows", slResult.RowCount,
				)
			}
		}
```

- [ ] **Step 2.4: Gate the lifecycle branch**

Find the lifecycle block (around lines 261-293 — locate by `if exp.LifecycleStratificationEnabled && !m.IsQoEMetric {`). Wrap:

```go
		// Lifecycle segmentation: if enabled, also compute per-lifecycle-segment metrics.
		if exp.LifecycleStratificationEnabled && !m.IsQoEMetric {
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping lifecycle metric: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
				lcParams := params
				lcParams.LifecycleEnabled = true

				lcSQL, err := j.renderer.RenderLifecycleMean(lcParams)
				if err != nil {
					return nil, fmt.Errorf("jobs: render lifecycle metric for %s: %w", m.MetricID, err)
				}

				lcResult, err := j.executor.ExecuteAndWrite(ctx, lcSQL, "delta.metric_summaries")
				if err != nil {
					return nil, fmt.Errorf("jobs: execute lifecycle metric for %s: %w", m.MetricID, err)
				}
				m3metrics.SparkQueryDuration.WithLabelValues("lifecycle_metric").Observe(lcResult.Duration.Seconds())
				m3metrics.SparkQueryRows.WithLabelValues("lifecycle_metric").Observe(float64(lcResult.RowCount))

				if err := j.queryLog.Log(ctx, querylog.Entry{
					ExperimentID: experimentID,
					MetricID:     m.MetricID,
					SQLText:      lcSQL,
					RowCount:     lcResult.RowCount,
					DurationMs:   lcResult.Duration.Milliseconds(),
					JobType:      "lifecycle_metric",
				}); err != nil {
					return nil, fmt.Errorf("jobs: log lifecycle metric query for %s: %w", m.MetricID, err)
				}

				slog.Info("computed lifecycle metric",
					"experiment_id", experimentID,
					"metric_id", m.MetricID,
					"rows", lcResult.RowCount,
				)
			}
		}
```

- [ ] **Step 2.5: Verify build**

Run: `go build ./services/metrics/...`
Expected: clean.

- [ ] **Step 2.6: Run all jobs tests — expect PASS**

Run: `go test ./services/metrics/internal/jobs/...`
Expected: every existing test passes. Existing tests use only legacy types, so the gates are no-ops for them.

- [ ] **Step 2.7: Commit**

```bash
git add services/metrics/internal/jobs/standard.go
git commit -m "fix(metrics): gate post-processing branches on isLegacyStyle

Wraps the four post-processing branches in standard.go's Run loop
(CUPED, MLRATE, session-level, lifecycle) in nested if/else with an
isLegacyStyle gate. Non-legacy types (FILTERED_MEAN, COMPOSITE,
WINDOWED_COUNT) emit a single-line slog.Info skip message and fall
through to the next branch.

Skip behavior is fall-through (no continue) so independent branches
don't suppress each other — skipping CUPED for a non-legacy type
should not also suppress session-level for that metric (though both
will skip if both are enabled, by the same gate).

Existing legacy-type metrics are unaffected — the gate is a no-op
for them.

Refs #511"
```

---

## Task 3: Extend e2e test with skip-verification

**Files:**
- Modify: `services/metrics/internal/config/testdata/seed_adr026_phase1.json`
- Modify: `services/metrics/internal/jobs/standard_test.go`

Augment the existing fixture to enable session-level and lifecycle on the experiment, attach a CUPED covariate to one new-type metric, and add a legacy MEAN metric to prove post-processing still runs for legacy types in the same run.

- [ ] **Step 3.1: Modify the seed fixture**

Replace `services/metrics/internal/config/testdata/seed_adr026_phase1.json` with the following (additions: `session_level: true`, `lifecycle_stratification_enabled: true` on the experiment; `cuped_covariate_metric_id` on `mobile_avg_watch_time`; new `secondary_metric_ids` includes `legacy_watch_time`; new fifth metric `legacy_watch_time` of type MEAN):

```json
{
  "experiments": [
    {
      "experiment_id": "e0000000-0000-0000-0000-0000000adr26",
      "name": "adr026_phase1_smoke",
      "type": "STANDARD",
      "state": "RUNNING",
      "started_at": "2026-05-01",
      "primary_metric_id": "mobile_avg_watch_time",
      "secondary_metric_ids": ["composite_engagement", "stream_starts_24h", "legacy_watch_time"],
      "session_level": true,
      "lifecycle_stratification_enabled": true,
      "variants": [
        {"variant_id": "control", "name": "Control", "traffic_fraction": 0.5, "is_control": true},
        {"variant_id": "treatment", "name": "Treatment", "traffic_fraction": 0.5, "is_control": false}
      ]
    }
  ],
  "metrics": [
    {
      "metric_id": "mobile_avg_watch_time",
      "name": "Mobile avg watch time",
      "type": "FILTERED_MEAN",
      "source_event_type": "heartbeat",
      "filter_sql": "platform = 'mobile'",
      "value_column": "duration_ms",
      "cuped_covariate_metric_id": "legacy_watch_time"
    },
    {
      "metric_id": "composite_engagement",
      "name": "Composite engagement",
      "type": "COMPOSITE",
      "source_event_type": "n/a",
      "operator": "WEIGHTED_SUM",
      "operands": [
        {"metric_id": "watch_time_minutes", "weight": 0.7},
        {"metric_id": "stream_start_rate", "weight": 0.3}
      ]
    },
    {
      "metric_id": "stream_starts_24h",
      "name": "Stream starts within 24h of exposure",
      "type": "WINDOWED_COUNT",
      "source_event_type": "n/a",
      "event_type": "stream_start",
      "window_hours": 24
    },
    {
      "metric_id": "legacy_watch_time",
      "name": "Legacy watch time (MEAN)",
      "type": "MEAN",
      "source_event_type": "heartbeat"
    }
  ]
}
```

- [ ] **Step 3.2: Update the existing test's `MetricsComputed` assertion**

In `services/metrics/internal/jobs/standard_test.go`, find `TestStandardJob_Run_ADR026Phase1_NewTypes`. The previous test asserted `MetricsComputed == 3`; now it must assert `MetricsComputed == 4` (the 3 new-type + 1 legacy). Update that assertion:

```go
	assert.Equal(t, 4, result.MetricsComputed,
		"all 4 metrics (3 new-type + 1 legacy) should compute primary SQL successfully")
```

- [ ] **Step 3.3: Add new subtests asserting gate behavior**

Append two new subtests to `TestStandardJob_Run_ADR026Phase1_NewTypes`, after the existing `windowed_count SQL` subtest:

```go
	// Build a set lookup of post-processing JobType values per metric_id from the query log.
	postProcessJobTypes := map[string]bool{
		"session_level_metric": true,
		"lifecycle_metric":     true,
		"cuped_covariate":      true,
	}
	postProcByMetric := make(map[string][]string) // metric_id -> []JobType
	for _, e := range entries {
		if postProcessJobTypes[e.JobType] {
			postProcByMetric[e.MetricID] = append(postProcByMetric[e.MetricID], e.JobType)
		}
	}

	t.Run("new types skip legacy post-processing", func(t *testing.T) {
		newTypeIDs := []string{"mobile_avg_watch_time", "composite_engagement", "stream_starts_24h"}
		for _, id := range newTypeIDs {
			assert.Empty(t, postProcByMetric[id],
				"%s (new ADR-026 Phase 1 type) should NOT emit any session-level / lifecycle / cuped_covariate SQL", id)
		}
	})

	t.Run("legacy types still run post-processing", func(t *testing.T) {
		got := postProcByMetric["legacy_watch_time"]
		assert.Contains(t, got, "session_level_metric",
			"legacy MEAN metric should produce session_level_metric SQL when session_level is enabled")
		assert.Contains(t, got, "lifecycle_metric",
			"legacy MEAN metric should produce lifecycle_metric SQL when lifecycle_stratification is enabled")
		// Note: legacy_watch_time has no cuped_covariate_metric_id, so no cuped_covariate entry expected.
	})
```

- [ ] **Step 3.4: Run the test — expect PASS**

Run: `go test ./services/metrics/internal/jobs/ -run TestStandardJob_Run_ADR026Phase1_NewTypes -v`
Expected: outer test + 5 subtests PASS (the 3 existing primary-SQL subtests + 2 new gate-behavior subtests).

If the new subtests fail:
- If `new types skip legacy post-processing` fails because `postProcByMetric` has entries for new-type IDs, the gate isn't firing for that metric — check the slog output for the skip message. Most likely cause: `isLegacyStyle` returns true unexpectedly, or the gate wraps the wrong block.
- If `legacy types still run post-processing` fails, the legacy MEAN metric is being incorrectly gated. Verify `isLegacyStyle("MEAN") == true` (Task 1's unit test).

- [ ] **Step 3.5: Run the full jobs suite — expect PASS**

Run: `go test ./services/metrics/internal/jobs/...`
Expected: every test passes. Existing tests using `seed_config.json` are unaffected (they don't use the new fixture).

- [ ] **Step 3.6: Commit**

```bash
git add services/metrics/internal/config/testdata/seed_adr026_phase1.json services/metrics/internal/jobs/standard_test.go
git commit -m "test(metrics): verify ADR-026 type-gates skip post-processing for new types

Extends seed_adr026_phase1.json with session_level + lifecycle_stratification
enabled on the experiment, a CUPED covariate on the FILTERED_MEAN metric,
and a fifth legacy MEAN metric (legacy_watch_time) to prove the gate
still runs post-processing for legacy types.

Two new subtests in TestStandardJob_Run_ADR026Phase1_NewTypes:
- 'new types skip legacy post-processing' — asserts ZERO post-processing
  JobType entries (session_level_metric, lifecycle_metric, cuped_covariate)
  for any of the 3 new-type metric_ids.
- 'legacy types still run post-processing' — asserts the legacy MEAN
  metric DOES emit session_level_metric and lifecycle_metric entries.

Updated MetricsComputed assertion from 3 to 4 (3 new-type + 1 legacy).

Refs #511"
```

---

## Task 4: Workspace verification + PR open

**Files:** none modified directly; this task is the final smoke + PR.

- [ ] **Step 4.1: Run the full Go test suite**

Run: `go test ./...`
Expected: every Go package passes. Pre-existing failures in `services/management` (gen/go/experimentation/management/v1 setup-failed pattern from prior PRs) are acceptable; verify by checking they reproduce on `origin/main` if uncertain.

- [ ] **Step 4.2: Run `go vet`**

Run: `go vet ./...`
Expected: clean.

- [ ] **Step 4.3: Confirm git status is clean**

Run: `git status`
Expected: clean working tree (or only `.beads/`/`.superpowers/` runtime cruft).

- [ ] **Step 4.4: Push branch**

Run: `git push -u origin agent-3/fix/issue-511-post-processing-type-gates`
Expected: branch pushed; GitHub returns the PR-creation URL.

- [ ] **Step 4.5: Open the PR**

```bash
gh pr create --base main --head agent-3/fix/issue-511-post-processing-type-gates \
  --title "fix(metrics): gate post-processing branches on isLegacyStyle (#511)" \
  --body "$(cat <<'PRBODY'
Closes [#511](https://github.com/wunderkennd/kaizen-experimentation/issues/511) (option a — type-gate). Option b (per-type post-processing templates) remains open for a future ADR.

Spec: \`docs/superpowers/specs/2026-05-07-issue-511-post-processing-type-gates.md\`
Plan: \`docs/superpowers/plans/2026-05-07-issue-511-post-processing-type-gates.md\`

## What changed

- **`isLegacyStyle` helper** in `services/metrics/internal/jobs/standard.go`: case-insensitive switch returning true for the 6 legacy MetricType values (MEAN, PROPORTION, COUNT, RATIO, PERCENTILE, CUSTOM), false for everything else.
- **Four post-processing gates** in `Run`: CUPED covariate, MLRATE cross-fit, session-level, lifecycle. Each branch wraps its existing body in a nested if/else; the skip path emits a single-line \`slog.Info\` and falls through (no \`continue\`).
- **Test fixture extension** in \`services/metrics/internal/config/testdata/seed_adr026_phase1.json\`: enables \`session_level: true\` and \`lifecycle_stratification_enabled: true\` on the experiment, attaches a \`cuped_covariate_metric_id\` to one new-type metric, and adds a fifth legacy MEAN metric to prove post-processing still runs for legacy types.
- **Two new subtests** in \`TestStandardJob_Run_ADR026Phase1_NewTypes\`: verify new-type metrics skip post-processing AND legacy metrics still run it.
- **Direct unit test** \`TestIsLegacyStyle\` covering legacy / non-legacy / unknown / case-insensitive paths (also exercises MLRATE coverage indirectly since the helper is shared across all 4 branches).

## Verification

- [x] \`go test ./services/metrics/...\` — all green
- [x] \`go test ./...\` — all green (pre-existing services/management setup-failed errors unrelated)
- [x] \`go vet ./...\` — clean
- [ ] CI on this PR — full matrix

## Closes the loop on PR #510

The latent bug surfaced in PR #510's final review (#510's review comments) is fixed: any combination of new-type metric + (\`session_level: true\` | \`lifecycle_stratification_enabled: true\` | \`cuped_covariate_metric_id\` | MLRATE config) now skips post-processing cleanly with a discoverable log message instead of producing silently-wrong (or empty) SQL.

## Out of scope

- Per-type post-processing templates (option b) — future ADR.
- M5 validation of legal feature combinations — issue #433.
- Loader refactor — issue #506.

Refs #432 (Phase 1 renderer), #504 / #510 (M3 wiring).
PRBODY
)"
```

- [ ] **Step 4.6: Capture the PR URL in the report**

Note the PR number returned by \`gh pr create\` for the final report.

---

## Self-Review Checklist

**1. Spec coverage:**
- [x] §1 `isLegacyStyle` helper (file-private, case-insensitive switch over 6 legacy types) → Task 1
- [x] §2 four post-processing gates with branch-specific skip messages → Task 2
- [x] §3 testing: extended fixture with experiment flags + CUPED covariate + legacy MEAN → Task 3
- [x] §3 direct unit test for `isLegacyStyle` → Task 1
- [x] §3 e2e subtests asserting new types skip + legacy types run → Task 3
- [x] §2 design choice: `slog.Info` not `slog.Warn`, fall-through not `continue` → Task 2's gate code

**2. Placeholder scan:** No "TBD", "TODO", "implement later", or vague directives. Each step has actual code or commands.

**3. Type consistency:** `isLegacyStyle` is defined once (Task 1) with signature `func(string) bool`, called identically from 4 sites (Task 2). The fixture's `cuped_covariate_metric_id: "legacy_watch_time"` references the legacy metric defined in the same fixture. Test assertions reference the right `MetricID` values (`mobile_avg_watch_time`, `composite_engagement`, `stream_starts_24h`, `legacy_watch_time`) and the right `JobType` values (`session_level_metric`, `lifecycle_metric`, `cuped_covariate`).

---

## Execution Choice

Two options:

1. **Subagent-Driven (recommended)** — Same workflow as PR #497 / PR #510: fresh subagent per task with two-stage review (spec compliance + code quality). Each of the 4 tasks runs to a clean commit before the next.

2. **Inline Execution** — Run all 4 tasks in this session via `superpowers:executing-plans` with checkpoints between tasks.

Subagent-driven is recommended for consistency with prior work on this feature stream.
