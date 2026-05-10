# ADR-026 Phase 1 — M3 MetricConfig Wiring (Production Readiness)

- **Date:** 2026-05-06
- **Status:** Approved
- **Author:** Kenneth Sylvain + Claude Code
- **Implements:** [Issue #504](https://github.com/wunderkennd/kaizen-experimentation/issues/504)
- **Builds on:** [PR #497](https://github.com/wunderkennd/kaizen-experimentation/pull/497) (issue #432, ADR-026 Phase 1 renderer)

## Context

PR #497 shipped the proto schema, M3 SQL templates, renderer plumbing, and validation arms for `FILTERED_MEAN`, `COMPOSITE`, `WINDOWED_COUNT`. The renderer is correct and fully tested.

**The renderer is unreachable from production.** `services/metrics/internal/jobs/standard.go` constructs `spark.TemplateParams` from `services/metrics/internal/config/MetricConfig` (the JSON-loaded config struct). `MetricConfig` predates ADR-026 Phase 1 and has no fields for the new types — `FilterSQL`, `ValueColumn`, `EventType`, `WindowHours`, `Operands`, `Operator`. A real-world FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT metric arriving from M5 would:

1. Hit `RenderForType` with empty new-type fields.
2. Be rejected by the renderer's validation arms (e.g. "FILTERED_MEAN requires non-empty filter_sql").
3. Be downgraded to `slog.Warn("skipping unsupported metric type", ...)` by `standard.go:94`.
4. Produce no `metric_summaries` row.

This spec wires `MetricConfig` and `standard.go` to the new `TemplateParams` fields, closes the production-readiness gap, and tightens the WARN log so unknown-config errors and unknown-type errors are distinguishable.

## Decision

**Extend `MetricConfig` with 6 flat fields plus an `OperandConfig` sub-struct.** Forward all 6 fields into `TemplateParams` in `standard.go`. Rephrase the WARN log to be accurate when the type is supported but per-type config is incomplete.

### Rationale

The existing `MetricConfig` is **already flat** — sibling fields like `NumeratorEventType`, `DenominatorEventType`, `Percentile`, `CustomSQL` discriminated by the `Type` string. Extending flat matches the existing pattern, requires no migration of the existing seed JSON files or test fixtures, and is the minimum-viable change to unblock production.

The proto's `MetricDefinition` uses a hybrid `oneof type_config` for the new types (per ADR-026 Phase 1's Option C) but `MetricConfig` is a JSON-loaded Go struct that doesn't consume proto wire-format directly. A future loader refactor (tracked as **#506**) may consolidate `MetricConfig` and `MetricDefinition` into a single proto-driven layer; that refactor is explicitly out of scope here.

## Architecture

### 1. `MetricConfig` extension — `services/metrics/internal/config/loader.go`

Add 6 flat JSON-tagged fields to the existing `MetricConfig` struct:

```go
type MetricConfig struct {
    // ... existing fields unchanged ...

    // ADR-026 Phase 1 — FILTERED_MEAN
    FilterSQL   string `json:"filter_sql,omitempty"`
    ValueColumn string `json:"value_column,omitempty"`

    // ADR-026 Phase 1 — COMPOSITE
    Operands []OperandConfig `json:"operands,omitempty"`
    Operator string          `json:"operator,omitempty"` // ADD, SUBTRACT, MULTIPLY, DIVIDE, WEIGHTED_SUM

    // ADR-026 Phase 1 — WINDOWED_COUNT
    EventType   string `json:"event_type,omitempty"`   // distinct from SourceEventType (which scopes filtered_mean reads)
    WindowHours int32  `json:"window_hours,omitempty"`
}
```

Add a new sub-struct (config-package-local — does not depend on `spark`):

```go
// OperandConfig is the config-layer representation of one operand of a
// COMPOSITE metric. Mirrors the proto CompositeOperand fields.
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

### Design choices

| Choice | Decision | Why |
| --- | --- | --- |
| Flat vs. nested oneof in JSON | Flat | Matches existing `MetricConfig` pattern; avoids invalidating every seed file. |
| `OperandConfig` location | Config-package-local | Avoids `config → spark` import dependency. Conversion to `spark.OperandParam` lives in `standard.go`. |
| `WindowHours` type | `int32` | Type-matches `spark.TemplateParams.WindowHours` to avoid lossy conversion. JSON unmarshal of integers into `int32` is well-supported. |
| `Weight` shape | `float64` (no wrapper) | Same proto3-default-zero gotcha as the proto field; documented inline; M5 / renderer reject zero weights for WEIGHTED_SUM. |
| `EventType` separate from `SourceEventType` | Yes | The two play different roles. `WINDOWED_COUNT` has only `EventType`; `FILTERED_MEAN` has only `SourceEventType`. Conflating them would force a confusing field-overload pattern. |

### 2. `standard.go` forwarding — `services/metrics/internal/jobs/standard.go`

Update the `TemplateParams` construction (currently lines 68–78 of `Run`) to include the new fields. Convert `[]config.OperandConfig` → `[]spark.OperandParam` via a small local helper:

```go
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
```

Helper added near the bottom of `standard.go` (file-private):

```go
// toSparkOperands converts config-layer OperandConfig values to the
// spark.OperandParam shape consumed by the renderer's composite template.
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

### 3. WARN log fix — `services/metrics/internal/jobs/standard.go:93–96`

Current behavior treats every renderer error the same:

```go
rendered, err := j.renderer.RenderForType(m.Type, params)
if err != nil {
    slog.Warn("skipping unsupported metric type",
        "metric_id", m.MetricID, "type", m.Type, "error", err)
    continue
}
```

The phrasing "unsupported metric type" is misleading when the type IS supported but the config is incomplete (e.g., FILTERED_MEAN with `filter_sql == ""` after this PR's wiring). The renderer's error message already says exactly what's wrong; the log message should not contradict it.

Replace with:

```go
rendered, err := j.renderer.RenderForType(m.Type, params)
if err != nil {
    slog.Warn("skipping metric: render error",
        "metric_id", m.MetricID, "type", m.Type, "error", err)
    continue
}
```

This is a minimum-change fix per the issue's acceptance criterion #4. A future improvement could distinguish "unknown type" (config layer wasn't taught about a new type) from "validation error" (config was incomplete), but that requires either typed errors from the renderer or a known-types allow-list in `standard.go`. Both are out of scope for #504. The new message is honest in both cases.

### 4. Testing strategy

Two new test cases in `services/metrics/internal/jobs/standard_test.go` (or the equivalent existing test file — implementation will discover the right location):

#### Test A — config round-trip

Construct a `MetricConfig` for each new type via JSON literal:

```go
filterMean := mustUnmarshalMetric(t, `{
    "metric_id": "mobile_avg_watch_time",
    "name": "Mobile avg watch time",
    "type": "FILTERED_MEAN",
    "source_event_type": "heartbeat",
    "filter_sql": "platform = 'mobile'",
    "value_column": "duration_ms"
}`)
// ... assert filterMean.FilterSQL == "platform = 'mobile'", etc.
```

Three subcases (one per new type) exercising the JSON loader + `MetricConfig` field hydration.

#### Test B — end-to-end SQL rendering

For each new type, drive `StandardJob.Run` (or `Run`-equivalent extracted helper) with:
- A mock `executor` that records the SQL it was asked to execute.
- A `ConfigStore` populated with one experiment + one metric of the new type.

Assert that the executor received non-empty SQL containing the type-distinctive identifiers:
- FILTERED_MEAN: `filtered_data` CTE name.
- COMPOSITE: `operand_rows` CTE name; reads from `delta.metric_summaries`.
- WINDOWED_COUNT: `windowed_events` CTE name; `INTERVAL N HOURS` literal.

This proves the wiring is end-to-end correct: JSON → `MetricConfig` → `TemplateParams` → renderer → executable SQL.

#### Existing tests

Pre-existing tests that load `MetricConfig` from JSON and don't set the new fields should continue to pass — all new fields have `omitempty` JSON tags and zero-value Go defaults.

## Out of scope

| Concern | Tracked elsewhere |
| --- | --- |
| Loader refactor to consume proto MetricDefinition directly | **#506 (NEW)** |
| M5 semantic validation of new-type config (cycle detection, allowlist, etc.) | #433 |
| M6 UI for the new types | #434 |
| M3 scheduler topological-order for COMPOSITE operand dependencies | #475 |
| M4a coordination on COMPOSITE `assignment_probability = 1.0` | informal review |

## Decisions log

| Decision | Choice | Rationale |
| --- | --- | --- |
| Loader refactor in this PR? | No — file separately as #506 | Out of scope for production-readiness fix; would inflate risk |
| `MetricConfig` shape | Flat extension | Matches existing pattern; avoids seed-file migration |
| `OperandConfig` location | Config-package-local | No `config → spark` dependency |
| `WindowHours` type | `int32` | Matches `spark.TemplateParams` |
| WARN log fix | Rephrase to "skipping metric: render error" | Honest in both validation-error and unknown-type cases |
| Distinguish unknown-type from validation-error in log? | No (defer) | Requires typed errors or allow-list; out of scope |

## References

- Issue #504 — this spec's primary deliverable
- PR #497 / Issue #432 — the renderer this wiring connects
- Issue #506 — future loader refactor (deferred)
- `services/metrics/internal/config/loader.go` — file modified
- `services/metrics/internal/jobs/standard.go` — file modified
- `services/metrics/internal/spark/renderer.go` — already extended in #497, no change here
