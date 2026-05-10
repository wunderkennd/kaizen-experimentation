# ADR-026 Phase 1 — M3 MetricConfig Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire `services/metrics/internal/config/MetricConfig` and `services/metrics/internal/jobs/standard.go` to the new `TemplateParams` fields so production FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT metrics actually compute (closes the production-readiness gap from PR #497, issue #504).

**Architecture:** Flat extension of `MetricConfig` with 6 new JSON-tagged fields plus a config-package-local `OperandConfig` struct. `standard.go` forwards the new fields into `TemplateParams` via a small `toSparkOperands` helper. WARN log message tightened so "type supported but config incomplete" no longer reads as "type unsupported." See spec at `docs/superpowers/specs/2026-05-06-adr-026-phase1-m3-config-wiring.md`.

**Tech Stack:** Go 1.22+, `encoding/json`, `github.com/stretchr/testify`. Builds on the renderer + templates already shipped in PR #497.

**Worktree:** `.claude/worktrees/adr-026-phase1-m3-config/` on branch `agent-3/feat/adr-026-phase1-m3-config-wiring` (off `origin/main`). Spec committed as `e9615b0`.

---

## File Structure

| File | Action | Responsibility |
| --- | --- | --- |
| `services/metrics/internal/config/loader.go` | Modify | Add 6 JSON-tagged fields to `MetricConfig` + new `OperandConfig` struct |
| `services/metrics/internal/config/loader_test.go` | Modify | Add JSON round-trip tests for the new fields |
| `services/metrics/internal/config/testdata/seed_config.json` | Modify | Add one experiment + three new-type metrics (one per type) for the e2e test |
| `services/metrics/internal/jobs/standard.go` | Modify | Forward new fields into `TemplateParams`; add `toSparkOperands` helper; rephrase WARN log |
| `services/metrics/internal/jobs/standard_test.go` | Modify | Add end-to-end integration test (config → standard.go → renderer → SQL) |

Total expected diff: ~50 lines impl + ~150 lines test/fixture.

---

## Task 1: Extend `MetricConfig` with new fields

**Files:**
- Modify: `services/metrics/internal/config/loader.go`
- Modify: `services/metrics/internal/config/loader_test.go`

TDD: write the failing test first (compile error, then assertion failure), then add fields.

- [ ] **Step 1.1: Write a test referencing fields that don't exist yet (RED)**

In `services/metrics/internal/config/loader_test.go`, add a new test at the bottom of the file:

```go
func TestMetricConfig_ADR026Phase1_RoundTrip(t *testing.T) {
	t.Run("filtered_mean", func(t *testing.T) {
		raw := `{
			"metric_id": "mobile_avg_watch_time",
			"name": "Mobile avg watch time",
			"type": "FILTERED_MEAN",
			"source_event_type": "heartbeat",
			"filter_sql": "platform = 'mobile'",
			"value_column": "duration_ms"
		}`
		var m MetricConfig
		require.NoError(t, json.Unmarshal([]byte(raw), &m))
		assert.Equal(t, "FILTERED_MEAN", m.Type)
		assert.Equal(t, "platform = 'mobile'", m.FilterSQL)
		assert.Equal(t, "duration_ms", m.ValueColumn)
	})

	t.Run("composite", func(t *testing.T) {
		raw := `{
			"metric_id": "engagement_score",
			"name": "Composite engagement",
			"type": "COMPOSITE",
			"operator": "WEIGHTED_SUM",
			"operands": [
				{"metric_id": "watch_time_minutes", "weight": 0.7},
				{"metric_id": "stream_start_rate", "weight": 0.3}
			]
		}`
		var m MetricConfig
		require.NoError(t, json.Unmarshal([]byte(raw), &m))
		assert.Equal(t, "COMPOSITE", m.Type)
		assert.Equal(t, "WEIGHTED_SUM", m.Operator)
		require.Len(t, m.Operands, 2)
		assert.Equal(t, "watch_time_minutes", m.Operands[0].MetricID)
		assert.InDelta(t, 0.7, m.Operands[0].Weight, 1e-9)
		assert.Equal(t, "stream_start_rate", m.Operands[1].MetricID)
		assert.InDelta(t, 0.3, m.Operands[1].Weight, 1e-9)
	})

	t.Run("windowed_count", func(t *testing.T) {
		raw := `{
			"metric_id": "stream_starts_24h",
			"name": "Stream starts within 24h",
			"type": "WINDOWED_COUNT",
			"event_type": "stream_start",
			"window_hours": 24
		}`
		var m MetricConfig
		require.NoError(t, json.Unmarshal([]byte(raw), &m))
		assert.Equal(t, "WINDOWED_COUNT", m.Type)
		assert.Equal(t, "stream_start", m.EventType)
		assert.Equal(t, int32(24), m.WindowHours)
	})

	t.Run("omitempty preserves backward compatibility", func(t *testing.T) {
		// A pre-ADR-026 metric (e.g. MEAN) must still unmarshal cleanly with
		// no JSON keys for the new fields.
		raw := `{
			"metric_id": "watch_time_minutes",
			"name": "Watch time",
			"type": "MEAN",
			"source_event_type": "heartbeat"
		}`
		var m MetricConfig
		require.NoError(t, json.Unmarshal([]byte(raw), &m))
		assert.Empty(t, m.FilterSQL)
		assert.Empty(t, m.ValueColumn)
		assert.Empty(t, m.Operands)
		assert.Empty(t, m.Operator)
		assert.Empty(t, m.EventType)
		assert.Equal(t, int32(0), m.WindowHours)
	})
}
```

The file's existing imports include `testing`, testify's `assert` and `require`. Add `"encoding/json"` to the import block if it's not already there:

```go
import (
	"encoding/json"
	"testing"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)
```

- [ ] **Step 1.2: Run the test — expect compile error**

Run: `go test ./services/metrics/internal/config/ -run TestMetricConfig_ADR026Phase1_RoundTrip -v`
Expected: COMPILE FAIL with errors like `undefined: m.FilterSQL`, `undefined: m.Operands`, `undefined: OperandConfig` (the test references fields and a type that don't exist yet).

- [ ] **Step 1.3: Add the new fields and `OperandConfig` struct**

In `services/metrics/internal/config/loader.go`, find the `MetricConfig` struct (currently lines 60-77) and append the 6 new fields after the existing `MLRATELookbackDays` field. Append `OperandConfig` definition immediately after the closing brace of `MetricConfig`:

```go
type MetricConfig struct {
	MetricID             string `json:"metric_id"`
	Name                 string `json:"name"`
	Type                 string `json:"type"`
	SourceEventType      string `json:"source_event_type"`
	NumeratorEventType   string `json:"numerator_event_type,omitempty"`
	DenominatorEventType string `json:"denominator_event_type,omitempty"`
	CupedCovariateMetricID string  `json:"cuped_covariate_metric_id,omitempty"`
	Percentile             float64 `json:"percentile,omitempty"`
	LowerIsBetter          bool    `json:"lower_is_better,omitempty"`
	IsQoEMetric          bool   `json:"is_qoe_metric,omitempty"`
	QoEField             string `json:"qoe_field,omitempty"`
	CustomSQL            string `json:"custom_sql,omitempty"`
	// MLRATE cross-fitting fields (ADR-015 Phase 2)
	MLRATEFeatureEventTypes []string `json:"mlrate_feature_event_types,omitempty"`
	MLRATEModelURI          string   `json:"mlrate_model_uri,omitempty"`
	MLRATELookbackDays      int      `json:"mlrate_lookback_days,omitempty"`

	// ADR-026 Phase 1 — FILTERED_MEAN
	FilterSQL   string `json:"filter_sql,omitempty"`
	ValueColumn string `json:"value_column,omitempty"`

	// ADR-026 Phase 1 — COMPOSITE
	Operands []OperandConfig `json:"operands,omitempty"`
	Operator string          `json:"operator,omitempty"` // ADD, SUBTRACT, MULTIPLY, DIVIDE, WEIGHTED_SUM

	// ADR-026 Phase 1 — WINDOWED_COUNT
	EventType   string `json:"event_type,omitempty"`   // distinct from SourceEventType
	WindowHours int32  `json:"window_hours,omitempty"`
}

// OperandConfig is the config-layer representation of one operand of a
// COMPOSITE metric. Mirrors the proto CompositeOperand fields (ADR-026 Phase 1).
//
// Weight is meaningful only for WEIGHTED_SUM operator; ignored otherwise.
// Note: encoding/json deserialises a missing or null number to 0.0, so
// WEIGHTED_SUM operands MUST set weight explicitly. The renderer's
// RenderForType arm rejects weight <= 0 for WEIGHTED_SUM.
type OperandConfig struct {
	MetricID string  `json:"metric_id"`
	Weight   float64 `json:"weight"`
}
```

- [ ] **Step 1.4: Run the test — expect PASS**

Run: `go test ./services/metrics/internal/config/ -run TestMetricConfig_ADR026Phase1_RoundTrip -v`
Expected: 4 subtests PASS (`filtered_mean`, `composite`, `windowed_count`, `omitempty preserves backward compatibility`).

- [ ] **Step 1.5: Run the rest of the config package tests — expect PASS**

Run: `go test ./services/metrics/internal/config/...`
Expected: every existing test still passes (the new fields are all `omitempty` so don't affect existing JSON fixtures).

- [ ] **Step 1.6: Commit**

```bash
git add services/metrics/internal/config/loader.go services/metrics/internal/config/loader_test.go
git commit -m "feat(metrics): MetricConfig fields for ADR-026 Phase 1 types

Adds 6 flat JSON-tagged fields to MetricConfig (FilterSQL, ValueColumn,
Operands, Operator, EventType, WindowHours) plus a config-package-local
OperandConfig struct, matching the existing flat MetricConfig pattern.

Round-trip tests cover each of the three new types plus a backward-
compatibility check that pre-ADR-026 metrics deserialise unchanged
(all new fields have omitempty JSON tags and zero-value Go defaults).

Refs #504"
```

---

## Task 2: Tighten WARN log message

**Files:**
- Modify: `services/metrics/internal/jobs/standard.go`

A one-word phrasing fix in the WARN log so it's accurate when the type IS supported but per-type config is incomplete. No new test (testing log output requires a slog handler interceptor that's heavier than the value gained for a literal-string change; the e2e test in Task 4 will exercise the path).

- [ ] **Step 2.1: Update the WARN message**

In `services/metrics/internal/jobs/standard.go`, find the `RenderForType` failure handler (currently around lines 92-97):

```go
} else {
	rendered, err := j.renderer.RenderForType(m.Type, params)
	if err != nil {
		slog.Warn("skipping unsupported metric type",
			"metric_id", m.MetricID, "type", m.Type, "error", err)
		continue
	}
	sql = rendered
	jobType = "daily_metric"
}
```

Change the WARN message string from `"skipping unsupported metric type"` to `"skipping metric: render error"`:

```go
} else {
	rendered, err := j.renderer.RenderForType(m.Type, params)
	if err != nil {
		slog.Warn("skipping metric: render error",
			"metric_id", m.MetricID, "type", m.Type, "error", err)
		continue
	}
	sql = rendered
	jobType = "daily_metric"
}
```

- [ ] **Step 2.2: Verify build**

Run: `go build ./services/metrics/...`
Expected: clean.

- [ ] **Step 2.3: Run existing standard_test — expect PASS (no behavioral change)**

Run: `go test ./services/metrics/internal/jobs/...`
Expected: every existing test still passes. The log message change is text-only; nothing tests the literal message string.

- [ ] **Step 2.4: Commit**

```bash
git add services/metrics/internal/jobs/standard.go
git commit -m "fix(metrics): WARN message on render error no longer says 'unsupported type'

The previous message 'skipping unsupported metric type' is misleading
when the type IS supported (e.g., FILTERED_MEAN) but per-type config
is incomplete (e.g., empty filter_sql). The renderer's error already
says exactly what's wrong; the log message should not contradict it.

Rephrased to 'skipping metric: render error' — honest in both the
'unknown type' and 'validation error' cases. The full error from the
renderer remains logged in the 'error' attribute.

Refs #504"
```

---

## Task 3: Forward new fields into `TemplateParams` + `toSparkOperands` helper

**Files:**
- Modify: `services/metrics/internal/jobs/standard.go`

Wire the new `MetricConfig` fields into the `TemplateParams` construction in `Run`. Add a small file-private `toSparkOperands` helper to convert `[]config.OperandConfig` → `[]spark.OperandParam`. (No new test in this task — the e2e test in Task 4 exercises the wiring end-to-end.)

- [ ] **Step 3.1: Add the new fields to the `TemplateParams` literal**

In `services/metrics/internal/jobs/standard.go`, find the `params := spark.TemplateParams{...}` literal in `Run` (currently lines 69-78). Extend it with the 6 new fields:

```go
for _, m := range metrics {
	params := spark.TemplateParams{
		ExperimentID:         exp.ExperimentID,
		MetricID:             m.MetricID,
		SourceEventType:      m.SourceEventType,
		ComputationDate:      computationDate,
		NumeratorEventType:   m.NumeratorEventType,
		DenominatorEventType: m.DenominatorEventType,
		CustomSQL:            m.CustomSQL,
		Percentile:           m.Percentile,
		// ADR-026 Phase 1
		FilterSQL:   m.FilterSQL,
		ValueColumn: m.ValueColumn,
		Operator:    m.Operator,
		EventType:   m.EventType,
		WindowHours: m.WindowHours,
		Operands:    toSparkOperands(m.Operands),
	}
	// ... rest unchanged
```

- [ ] **Step 3.2: Add the `toSparkOperands` helper**

Append at the bottom of `services/metrics/internal/jobs/standard.go` (after the `Run` function):

```go
// toSparkOperands converts config-layer OperandConfig values to the
// spark.OperandParam shape consumed by the renderer's composite template.
// Returns nil for an empty/nil input (matches Go's zero-value convention).
func toSparkOperands(in []config.OperandConfig) []spark.OperandParam {
	if len(in) == 0 {
		return nil
	}
	out := make([]spark.OperandParam, len(in))
	for i, op := range in {
		out[i] = spark.OperandParam{
			MetricID: op.MetricID,
			Weight:   op.Weight,
		}
	}
	return out
}
```

- [ ] **Step 3.3: Verify build**

Run: `go build ./services/metrics/...`
Expected: clean.

- [ ] **Step 3.4: Run existing tests — expect PASS**

Run: `go test ./services/metrics/...`
Expected: every existing test passes. The new TemplateParams fields are all zero-valued for existing seed metrics (MEAN/RATIO/etc.), so behavior is unchanged for non-new-type metrics.

- [ ] **Step 3.5: Commit**

```bash
git add services/metrics/internal/jobs/standard.go
git commit -m "feat(metrics): forward MetricConfig new-type fields into TemplateParams

Wires the 6 ADR-026 Phase 1 fields (FilterSQL, ValueColumn, Operator,
EventType, WindowHours, Operands) from MetricConfig into the
TemplateParams literal in standard.go's Run loop.

Adds a file-private toSparkOperands helper that converts
[]config.OperandConfig to []spark.OperandParam. Helper is local to
standard.go to avoid a config -> spark import dependency in the
config package.

Existing metrics (MEAN, RATIO, etc.) are unaffected — all 6 new
TemplateParams fields are zero-valued for those metrics, and the
renderer's existing arms don't read them.

Refs #504"
```

---

## Task 4: End-to-end integration test

**Files:**
- Modify: `services/metrics/internal/config/testdata/seed_config.json`
- Modify: `services/metrics/internal/jobs/standard_test.go`

This test exercises the full path: JSON → `MetricConfig` → `TemplateParams` → renderer → executable SQL. It proves that the wiring from Tasks 1 and 3 actually produces correct SQL for each new type.

The cleanest approach is a NEW seed file dedicated to ADR-026 Phase 1 fixtures, so we don't perturb the existing `seed_config.json` and any tests that depend on its exact metric counts. Plan: create `services/metrics/internal/config/testdata/seed_adr026_phase1.json` with one experiment and three metrics (one per new type).

- [ ] **Step 4.1: Create the new seed file**

Create `services/metrics/internal/config/testdata/seed_adr026_phase1.json`:

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
      "secondary_metric_ids": ["composite_engagement", "stream_starts_24h"],
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
      "value_column": "duration_ms"
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
    }
  ]
}
```

Note: `source_event_type` is required by existing `MetricConfig` JSON tags (no `omitempty`), so the COMPOSITE and WINDOWED_COUNT metrics set it to `"n/a"` (the field is unused by their respective templates but the loader expects a non-zero value). This matches how RATIO metrics in the existing seed handle their (unused-for-RATIO) `source_event_type`.

- [ ] **Step 4.2: Add the integration test**

Append to `services/metrics/internal/jobs/standard_test.go`:

```go
func TestStandardJob_Run_ADR026Phase1_NewTypes(t *testing.T) {
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_adr026_phase1.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(123)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfgStore, renderer, executor, qlWriter)

	ctx := context.Background()
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-0000000adr26")
	require.NoError(t, err)
	assert.Equal(t, 3, result.MetricsComputed,
		"all 3 new-type metrics should compute successfully (no slog.Warn skip)")

	calls := executor.GetCalls()
	require.GreaterOrEqual(t, len(calls), 3, "executor should receive >= 3 SQL calls (one per metric, plus any post-processing)")

	// Each metric's primary SQL must contain its type-distinctive identifier.
	// Use the first executor call per metric_id from the query log.
	entries := qlWriter.AllEntries()
	sqlByMetric := make(map[string]string, 3)
	for _, e := range entries {
		if e.JobType == "daily_metric" {
			sqlByMetric[e.MetricID] = e.SQLText
		}
	}

	t.Run("filtered_mean SQL", func(t *testing.T) {
		sql, ok := sqlByMetric["mobile_avg_watch_time"]
		require.True(t, ok, "FILTERED_MEAN metric must have produced SQL")
		assert.Contains(t, sql, "filtered_data",
			"FILTERED_MEAN template must produce a `filtered_data` CTE")
		assert.Contains(t, sql, "platform = 'mobile'",
			"FILTERED_MEAN SQL must inline the configured filter_sql")
		assert.Contains(t, sql, "me.duration_ms",
			"FILTERED_MEAN SQL must reference the configured value_column")
	})

	t.Run("composite SQL", func(t *testing.T) {
		sql, ok := sqlByMetric["composite_engagement"]
		require.True(t, ok, "COMPOSITE metric must have produced SQL")
		assert.Contains(t, sql, "operand_rows",
			"COMPOSITE template must produce an `operand_rows` CTE")
		assert.Contains(t, sql, "delta.metric_summaries",
			"COMPOSITE template must read from delta.metric_summaries")
		assert.Contains(t, sql, "0.7", "WEIGHTED_SUM SQL must inline operand weights")
		assert.Contains(t, sql, "0.3", "WEIGHTED_SUM SQL must inline operand weights")
	})

	t.Run("windowed_count SQL", func(t *testing.T) {
		sql, ok := sqlByMetric["stream_starts_24h"]
		require.True(t, ok, "WINDOWED_COUNT metric must have produced SQL")
		assert.Contains(t, sql, "windowed_events",
			"WINDOWED_COUNT template must produce a `windowed_events` CTE")
		assert.Contains(t, sql, "INTERVAL 24 HOURS",
			"WINDOWED_COUNT SQL must inline the configured window_hours")
		assert.Contains(t, sql, "me.event_type = 'stream_start'",
			"WINDOWED_COUNT SQL must inline the configured event_type")
	})
}
```

- [ ] **Step 4.3: Run the test — expect PASS**

Run: `go test ./services/metrics/internal/jobs/ -run TestStandardJob_Run_ADR026Phase1_NewTypes -v`
Expected: outer test + 3 subtests all PASS. `result.MetricsComputed == 3` proves none of the new-type metrics fell into the WARN+skip branch.

If it fails:
- If the outer test fails with `MetricsComputed != 3`, one of the new-type metrics is hitting the WARN+skip path. Check the test output for the slog.Warn message; it'll name which metric failed and why. Most likely: a fixture field is missing (e.g., empty `filter_sql` rejected by the renderer's validation arm).
- If a subtest fails on a `Contains` assertion, the rendered SQL doesn't match the template's expected output. Print `sql` with `t.Logf("%s", sql)` to debug.

- [ ] **Step 4.4: Run the full jobs package test suite — expect PASS**

Run: `go test ./services/metrics/internal/jobs/...`
Expected: every test passes (including the existing `TestStandardJob_Run` which uses a different seed file).

- [ ] **Step 4.5: Commit**

```bash
git add services/metrics/internal/config/testdata/seed_adr026_phase1.json \
        services/metrics/internal/jobs/standard_test.go
git commit -m "test(metrics): end-to-end test for ADR-026 Phase 1 new-type metrics

Adds testdata/seed_adr026_phase1.json with one experiment and three
metrics (one per new type) plus an integration test that proves:

- All 3 new-type metrics compute successfully (no slog.Warn skip).
- FILTERED_MEAN SQL has filtered_data CTE + inlined filter + value column.
- COMPOSITE SQL has operand_rows CTE + reads delta.metric_summaries +
  inlines operand weights.
- WINDOWED_COUNT SQL has windowed_events CTE + correct INTERVAL HOURS
  literal + inlined event_type.

This is the production-readiness verification: it proves the JSON ->
MetricConfig -> TemplateParams -> renderer chain works end-to-end for
each of the new types.

Refs #504"
```

---

## Task 5: Workspace verification + PR open

**Files:** none modified directly; this task is the final smoke + PR.

- [ ] **Step 5.1: Run the full Go test suite**

Run: `go test ./...`
Expected: every Go package passes.

If anything fails (especially in services/metrics/), STOP and report BLOCKED with details.

- [ ] **Step 5.2: Run `go vet`**

Run: `go vet ./...`
Expected: clean.

- [ ] **Step 5.3: Confirm git status is clean**

Run: `git status`
Expected: clean working tree (or only `.beads/`/`.superpowers/` runtime cruft).

- [ ] **Step 5.4: Push branch**

Run: `git push -u origin agent-3/feat/adr-026-phase1-m3-config-wiring`
Expected: branch pushed.

- [ ] **Step 5.5: Open the PR**

```bash
gh pr create --base main --head agent-3/feat/adr-026-phase1-m3-config-wiring \
  --title "feat(metrics): ADR-026 Phase 1 — wire MetricConfig to FILTERED_MEAN/COMPOSITE/WINDOWED_COUNT" \
  --body "$(cat <<'PRBODY'
Closes [#504](https://github.com/wunderkennd/kaizen-experimentation/issues/504). Closes the production-readiness gap surfaced in [PR #497](https://github.com/wunderkennd/kaizen-experimentation/pull/497)'s final review.

Spec: \`docs/superpowers/specs/2026-05-06-adr-026-phase1-m3-config-wiring.md\`
Plan: \`docs/superpowers/plans/2026-05-06-adr-026-phase1-m3-config-wiring.md\`

## What changed

- **\`MetricConfig\` extension** — 6 flat JSON-tagged fields (FilterSQL, ValueColumn, Operands, Operator, EventType, WindowHours) plus a config-package-local \`OperandConfig\` struct. Matches the existing flat \`MetricConfig\` pattern (NumeratorEventType, Percentile, CustomSQL all sibling-flat). All new fields are \`omitempty\` so existing seed JSON files are untouched.
- **\`standard.go\` forwarding** — the 6 fields flow into \`TemplateParams\` via a small file-private \`toSparkOperands\` helper that converts \`[]config.OperandConfig\` → \`[]spark.OperandParam\` (no \`config\` → \`spark\` import dependency).
- **WARN log fix** — phrasing changed from \"skipping unsupported metric type\" to \"skipping metric: render error\" so the message is accurate when the type IS supported but per-type config is incomplete.
- **End-to-end test** — \`testdata/seed_adr026_phase1.json\` with one experiment and three new-type metrics, plus \`TestStandardJob_Run_ADR026Phase1_NewTypes\` proving each type produces correct SQL through the full JSON → MetricConfig → TemplateParams → renderer chain.

## Verification

- [x] \`go test ./services/metrics/...\` — all green
- [x] \`go test ./...\` — all green
- [x] \`go vet ./...\` — clean
- [ ] CI on this PR — full matrix

## Out of scope

- M5 semantic validation of new-type config — issue #433
- M6 UI for new types — issue #434
- M3 scheduler topological-order for COMPOSITE — issue #475
- Loader refactor to consume proto MetricDefinition directly — issue #506

Refs #432 (Phase 1 renderer), #506 (loader refactor follow-up).
PRBODY
)"
```

- [ ] **Step 5.6: Capture the PR URL in the report**

Note the PR number returned by \`gh pr create\` for the final report.

---

## Self-Review Checklist

Run after writing the plan:

**1. Spec coverage:**
- [x] §1 MetricConfig extension (6 fields + OperandConfig) → Task 1
- [x] §2 standard.go forwarding (TemplateParams literal extension) → Task 3
- [x] §2 toSparkOperands helper → Task 3
- [x] §3 WARN log rephrase → Task 2
- [x] §4 testing strategy (JSON round-trip + end-to-end SQL render) → Tasks 1 + 4
- [x] Existing tests unaffected → omitempty tags + zero-value defaults verified by Task 1.5 and Task 3.4

**2. Placeholder scan:** No "TBD", "TODO", "implement later", or vague directives. Each step has actual code or commands.

**3. Type consistency:** `OperandConfig` defined in Task 1 is used in Task 3 (`toSparkOperands(in []config.OperandConfig)`); `spark.OperandParam` is the existing pre-Task type from PR #497. `MetricConfig` field names (`FilterSQL`, `ValueColumn`, `Operands`, `Operator`, `EventType`, `WindowHours`) are consistent across all four tasks. JSON tag names match the proto field names from PR #497's metric.proto (`filter_sql`, `value_column`, etc.) for visual symmetry between proto and config layers.

---

## Execution Choice

Two options:

1. **Subagent-Driven (recommended)** — Dispatch a fresh subagent per task with the task heading + sub-steps as the prompt. Two-stage review (spec compliance + code quality) per task. Same flow as #432/#497.

2. **Inline Execution** — Run all 5 tasks in this session via `superpowers:executing-plans` with checkpoints between tasks.

Subagent-driven is recommended for consistency with the prior #432 work and to keep this conversation's context lean.
