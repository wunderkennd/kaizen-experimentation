# ADR-026 Phase 1 — Design Spec

- **Date:** 2026-05-04
- **Status:** Approved
- **Author:** Kenneth Sylvain + Claude Code
- **Implements:** [Issue #432](https://github.com/wunderkennd/kaizen-experimentation/issues/432) (proto + M3 templates + renderer)
- **Downstream consumers:** [#433](https://github.com/wunderkennd/kaizen-experimentation/issues/433) (M5 validation), [#434](https://github.com/wunderkennd/kaizen-experimentation/issues/434) (M6 UI)
- **Parent ADR:** [ADR-026 Phase 1](../../adrs/026-custom-metrics-layer.md)

## Context

ADR-026 introduces a three-tier metric definition layer. Phase 1 adds three new structured `MetricType` enum values — `FILTERED_MEAN`, `COMPOSITE`, `WINDOWED_COUNT` — covering ~60% of current `CUSTOM` metric use cases at low risk.

This spec covers issue #432 (proto schema + M3 SQL templates + renderer plumbing) and the design boundary with #433 (M5 validation) and #434 (M6 UI). It does **not** specify M5 or M6 implementations beyond what the proto contract requires.

## Decision

**Adopt Option C: hybrid oneof.** Keep existing flat type-config fields untouched (`numerator_event_type`, `denominator_event_type`, `percentile`, `custom_sql`); introduce a new `oneof type_config` on `MetricDefinition` populated only by the three new types.

### Rationale

Option B (full oneof migration including existing types) is the cleaner steady-state schema, but it is a wire-format breaking change that touches 5 SDKs (Android Kotlin, iOS Swift, Web TypeScript, server-go, server-python), the M5 stored-metric corpus, all M3 templates, and all M4a / M6 consumers — roughly 100–200 file edits across 4 languages and 1–2 weeks of focused work plus cross-team coordination. That extends Phase 1's 3-week scope to 5–6 weeks for a refactor that is not on ADR-026's critical path.

Option C captures Option B's benefits *for the new types* (self-documenting per-type configs, oneof-exhaustive validation in code) while:

- Preserving wire compatibility for every stored `MetricDefinition`.
- Avoiding consumer churn in the 5 SDKs.
- Leaving the door open to a future "convert existing types into the same oneof" sprint, informed by real Phase 1 / Phase 2 usage data.

See [Future Work](#future-work) for the full-Option-B migration note.

## Architecture

### 1. Proto schema

Additions to `proto/experimentation/common/v1/metric.proto`:

```proto
// Existing enum, three new values appended (no renumbering):
enum MetricType {
  // ... existing 0..6 unchanged ...
  METRIC_TYPE_FILTERED_MEAN   = 7;
  METRIC_TYPE_COMPOSITE       = 8;
  METRIC_TYPE_WINDOWED_COUNT  = 9;
}

// Existing message, extended at the end (fields 1..16 unchanged):
message MetricDefinition {
  // ... existing 1..16 unchanged ...

  // Per-type config for the new types only.
  // Existing types continue to use their flat sibling fields.
  oneof type_config {
    FilteredMeanConfig   filtered_mean   = 17;
    CompositeConfig      composite       = 18;
    WindowedCountConfig  windowed_count  = 19;
  }
}

message FilteredMeanConfig {
  // Spark SQL fragment AND'd into the scan WHERE clause.
  // Validated in M5 (#433): column allowlist + parse check.
  // Example: "platform = 'mobile' AND duration_ms > 5000"
  string filter_sql = 1;

  // Column from source_event_type to AVG over.
  // Example: "duration_ms"
  string value_column = 2;

  // Note: source_event_type is read from the existing MetricDefinition.source_event_type
  // (field 5). No duplication.
}

message CompositeConfig {
  // Operands referenced by metric_id. Cycle detection lives in M5 (#433).
  repeated CompositeOperand operands = 1;
  CompositeOperator         operator = 2;
}

message CompositeOperand {
  string metric_id = 1;
  // Coefficient; only meaningful when operator = WEIGHTED_SUM. Defaults to 1.0.
  double weight    = 2;
}

enum CompositeOperator {
  COMPOSITE_OPERATOR_UNSPECIFIED  = 0;
  COMPOSITE_OPERATOR_ADD          = 1;
  COMPOSITE_OPERATOR_SUBTRACT     = 2;
  COMPOSITE_OPERATOR_MULTIPLY     = 3;
  COMPOSITE_OPERATOR_DIVIDE       = 4;
  COMPOSITE_OPERATOR_WEIGHTED_SUM = 5;
}

message WindowedCountConfig {
  // Event type to count. Validated against the event catalog in M5 (#433).
  string event_type = 1;

  // Optional Spark SQL fragment AND'd into the scan WHERE clause.
  string filter_sql = 2;

  // Time window relative to the user's first exposure (hours).
  int32 window_hours = 3;
}
```

#### Wire compatibility

- `buf breaking` passes: only additions, no field renumbering, no removed fields.
- Existing serialized `MetricDefinition` bytes deserialize unchanged.
- `oneof type_config` is unset for existing metrics — no migration needed.

#### Design choices

| Choice | Decision | Why |
| --- | --- | --- |
| `source_event_type` for FILTERED_MEAN | Read from the existing top-level field, not duplicated in `FilteredMeanConfig` | Single source of truth |
| `filter_sql` shape | String (Spark SQL fragment), not a structured `FilterPredicate` | Matches issue #433's wording; structured predicates are a separate sub-design |
| `CompositeOperator` shape | Enum | Type-safe; matches "operator is recognized" wording in #433 |
| `weight` location | Per-operand field, ignored by non-`WEIGHTED_SUM` operators | Avoids a per-operator config-message proliferation |
| Cycle detection on COMPOSITE | Not represented in proto; lives in M5 validation | Graph property, computed at validation time |
| `window_hours` shape | `int32`, not `google.protobuf.Duration` | Matches ADR Appendix C; promote to Duration only if sub-hour granularity is ever required |

### 2. M3 SQL templates

Three new templates under `services/metrics/internal/spark/templates/`:

#### `filtered_mean.sql.tmpl`

Custom CTEs (does not reuse `exposure_join`, which hardcodes the value column to `value` and one event-type filter):

```sql
WITH exposed_users AS (
    SELECT user_id, variant_id, MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = '{{.ExperimentID}}'
    GROUP BY user_id, variant_id
),
filtered_data AS (
    SELECT me.user_id, eu.variant_id, me.{{.ValueColumn}} AS value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = '{{.SourceEventType}}'
      AND ({{.FilterSQL}})
)
SELECT
    '{{.ExperimentID}}' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    '{{.MetricID}}' AS metric_id,
    AVG(fd.value) AS metric_value,
    CAST('{{.ComputationDate}}' AS DATE) AS computation_date,
    eu.assignment_probability
FROM exposed_users eu
LEFT JOIN filtered_data fd ON eu.user_id = fd.user_id AND eu.variant_id = fd.variant_id
GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability;
```

- `LEFT JOIN` keeps users with zero matching events (mirrors `count.sql.tmpl`).
- `{{.ValueColumn}}` interpolated as a bare identifier; M5 (#433) enforces an identifier allowlist so quote-escaping is unnecessary here.
- `{{.FilterSQL}}` wrapped in parens so user-written `OR` doesn't break operator precedence.

#### `composite.sql.tmpl`

Reads pre-computed operand values from `delta.metric_summaries` (where per-user per-metric values land), pivots by `metric_id`, applies the operator:

```sql
WITH operand_rows AS (
    SELECT user_id, variant_id, metric_id, metric_value
    FROM delta.metric_summaries
    WHERE experiment_id = '{{.ExperimentID}}'
      AND computation_date = CAST('{{.ComputationDate}}' AS DATE)
      AND metric_id IN ({{ range $i, $op := .Operands }}{{if $i}}, {{end}}'{{$op.MetricID}}'{{end}})
),
pivoted AS (
    SELECT
        user_id,
        variant_id,
        {{ range $i, $op := .Operands -}}
        MAX(CASE WHEN metric_id = '{{$op.MetricID}}' THEN metric_value END) AS m{{$i}}{{ if not (last $i $.Operands) }},{{ end }}
        {{ end }}
    FROM operand_rows
    GROUP BY user_id, variant_id
)
SELECT
    '{{.ExperimentID}}' AS experiment_id,
    user_id,
    variant_id,
    '{{.MetricID}}' AS metric_id,
    {{- if eq .Operator "ADD" }}
    ({{ range $i, $op := .Operands }}{{if $i}} + {{end}}m{{$i}}{{end}})
    {{- else if eq .Operator "SUBTRACT" }}
    ({{ range $i, $op := .Operands }}{{if $i}} - {{end}}m{{$i}}{{end}})
    {{- else if eq .Operator "MULTIPLY" }}
    ({{ range $i, $op := .Operands }}{{if $i}} * {{end}}m{{$i}}{{end}})
    {{- else if eq .Operator "DIVIDE" }}
    ({{ range $i, $op := .Operands }}{{if $i}} / NULLIF(m{{$i}}, 0){{else}}m{{$i}}{{end}}{{end}})
    {{- else if eq .Operator "WEIGHTED_SUM" }}
    ({{ range $i, $op := .Operands }}{{if $i}} + {{end}}({{$op.Weight}} * m{{$i}}){{end}})
    {{- end }} AS metric_value,
    CAST('{{.ComputationDate}}' AS DATE) AS computation_date,
    1.0 AS assignment_probability
FROM pivoted;
```

- `NULLIF(m_i, 0)` on `DIVIDE` prevents divide-by-zero job failures; result becomes NULL, which M4a treats as missing.
- `assignment_probability` is hard-coded to 1.0 because COMPOSITE has no single source event — operand metrics carry their own assignment probabilities. Coordination point with Agent-4 (M4a) flagged in Out-of-Scope below.
- Requires registering a `last` template helper on the renderer's `template.FuncMap`. Definition: `func(i int, slice []OperandParam) bool { return i == len(slice)-1 }` — used to suppress the trailing comma in the pivot SELECT list.

##### Scheduler dependency (deferred to a sibling issue)

`COMPOSITE` reads from `delta.metric_summaries`, meaning operand metrics must be computed *first*. M3's current scheduler iterates `metric_definitions` independently and does not enforce a topological order over operand dependencies.

This is real additional scope outside #432's "proto + templates + golden files" acceptance criteria. To be filed as a sibling P1 issue: **"ADR-026 Phase 1: M3 scheduler dependency ordering for COMPOSITE."** That issue must merge before any production COMPOSITE metric is enabled, but #432 itself can ship without it.

#### `windowed_count.sql.tmpl`

Custom CTEs because it needs `event_timestamp` and `exposure_timestamp`, which the existing `exposure_join` template doesn't expose:

```sql
WITH exposed_users AS (
    SELECT user_id, variant_id,
           MIN(exposure_timestamp) AS exposure_ts,
           MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = '{{.ExperimentID}}'
    GROUP BY user_id, variant_id
),
windowed_events AS (
    SELECT eu.user_id, eu.variant_id
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = '{{.EventType}}'
      AND me.event_timestamp >= eu.exposure_ts
      AND me.event_timestamp <  eu.exposure_ts + INTERVAL {{.WindowHours}} HOURS
      {{ if .FilterSQL }}AND ({{.FilterSQL}}){{ end }}
)
SELECT
    '{{.ExperimentID}}' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    '{{.MetricID}}' AS metric_id,
    CAST(COUNT(we.user_id) AS DOUBLE) AS metric_value,
    CAST('{{.ComputationDate}}' AS DATE) AS computation_date,
    eu.assignment_probability
FROM exposed_users eu
LEFT JOIN windowed_events we ON eu.user_id = we.user_id AND eu.variant_id = we.variant_id
GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability;
```

- Half-open interval `[exposure_ts, exposure_ts + window)` matches industry convention.
- `MIN(exposure_timestamp)` anchors to the user's first exposure (a user can be exposed multiple times; first wins).
- Schema dependency: assumes `delta.exposures` has an `exposure_timestamp` column. Implementation must verify against the actual Delta Lake schema before the template ships.

### 3. Renderer plumbing

Three new helpers on `SQLRenderer` (`services/metrics/internal/spark/renderer.go`), pattern matching `RenderMean` / `RenderRatio`:

```go
func (r *SQLRenderer) RenderFilteredMean(p TemplateParams) (string, error)  { return r.Render("filtered_mean.sql.tmpl", p) }
func (r *SQLRenderer) RenderComposite(p TemplateParams) (string, error)     { return r.Render("composite.sql.tmpl", p) }
func (r *SQLRenderer) RenderWindowedCount(p TemplateParams) (string, error) { return r.Render("windowed_count.sql.tmpl", p) }
```

Three new arms in `RenderForType` (`renderer.go:132`), each validating required fields before render and returning a metric-id-tagged error on failure (mirrors the existing `CUSTOM` arm):

```go
case "FILTERED_MEAN":
    if p.FilterSQL == ""   { return "", fmt.Errorf("spark: FILTERED_MEAN metric %q requires non-empty filter_sql", p.MetricID) }
    if p.ValueColumn == "" { return "", fmt.Errorf("spark: FILTERED_MEAN metric %q requires non-empty value_column", p.MetricID) }
    return r.RenderFilteredMean(p)

case "COMPOSITE":
    if len(p.Operands) == 0                                  { return "", fmt.Errorf("spark: COMPOSITE metric %q requires at least one operand", p.MetricID) }
    if p.Operator == "" || p.Operator == "UNSPECIFIED"       { return "", fmt.Errorf("spark: COMPOSITE metric %q requires a known operator", p.MetricID) }
    return r.RenderComposite(p)

case "WINDOWED_COUNT":
    if p.EventType == ""   { return "", fmt.Errorf("spark: WINDOWED_COUNT metric %q requires non-empty event_type", p.MetricID) }
    if p.WindowHours <= 0  { return "", fmt.Errorf("spark: WINDOWED_COUNT metric %q requires window_hours > 0", p.MetricID) }
    return r.RenderWindowedCount(p)
```

Update the `default` arm's error message to include the three new types.

`TemplateParams` gains six new fields, populated by M3 from the per-type config in `MetricDefinition.type_config`:

```go
type TemplateParams struct {
    // ... existing ...
    FilterSQL    string         // FILTERED_MEAN, WINDOWED_COUNT
    ValueColumn  string         // FILTERED_MEAN
    EventType    string         // WINDOWED_COUNT (named distinctly from SourceEventType)
    WindowHours  int32          // WINDOWED_COUNT
    Operands     []OperandParam // COMPOSITE
    Operator     string         // COMPOSITE — uppercase ADD|SUBTRACT|MULTIPLY|DIVIDE|WEIGHTED_SUM
}

type OperandParam struct {
    MetricID string
    Weight   float64
}
```

### 4. Renderer-side validation surface

This is a thin guard against missing fields that would render invalid SQL. **Full semantic validation lives in #433 (M5).**

| Type | Renderer-side checks |
| --- | --- |
| `FILTERED_MEAN` | `filter_sql` non-empty; `value_column` non-empty |
| `COMPOSITE` | ≥ 1 operand; each operand has non-empty `metric_id`; operator ≠ `UNSPECIFIED` |
| `WINDOWED_COUNT` | `event_type` non-empty; `window_hours > 0` |

Anything more than "is this field empty / zero" — column allowlist enforcement, Spark SQL parse check, COMPOSITE cycle detection, event catalog lookup — is M5's responsibility.

### 5. Testing strategy

#### Golden files

Under `services/metrics/internal/spark/testdata/golden/`:

| Template | Scenarios | Files |
| --- | --- | --- |
| `filtered_mean` | simple filter; multi-clause AND/OR filter; filter referencing `value_column` itself | 3 |
| `composite` | one golden per operator (5) + WEIGHTED_SUM with non-uniform weights | 6 |
| `windowed_count` | with filter, without filter, 1-hour window, 168-hour (7-day) window | 4 |

Total: 13 new golden files.

#### Test runner

A `TestRenderForType_Goldens` table-driven test loops over fixtures, calls the renderer, diffs against the golden file, fails with a unified diff on mismatch. Same pattern as existing `mean` / `ratio` golden tests.

#### Contract tests

None new in #432. The proto change is purely additive (no field renumbering, no broken consumers). M5 contract test for the new types lives in #433.

#### Proto checks in CI

- `buf lint` — passes (additions only, no naming-rule violations).
- `buf breaking` — passes (additions only).

## Out of scope

The following are explicitly deferred from #432 and tracked elsewhere:

| Concern | Tracked by |
| --- | --- |
| Full M5 semantic validation (allowlist, parse check, cycle detection, event catalog) | #433 |
| M6 UI for the new types | #434 |
| M3 scheduler topological-order for COMPOSITE operand dependencies | **New sibling P1 issue** to be filed alongside #432 |
| M4a `assignment_probability` semantics for COMPOSITE | Coordination ticket with Agent-4 to be filed if #432 review surfaces concerns |

## Future work

### Full Option B migration (existing types into the oneof)

A future ADR may consolidate the existing flat type-config fields (`numerator_event_type`, `denominator_event_type`, `percentile`, `custom_sql`) into the `oneof type_config` introduced here. That would unify the schema at the cost of:

- Wire-format breaking change requiring coordinated rollout across 5 SDKs (Android, iOS, Web, server-go, server-python).
- Migration of the M5 stored-metric corpus.
- Permanent annotation of the affected types in `buf breaking` for the migration window.
- Cosmetic regression in some language bindings (Rust prost-generated `oneof` access is more verbose than flat field access; same for Go).

Out of scope for ADR-026; warrants its own ADR and its own sprint if the team chooses to pursue it. A lightweight tracking issue will be filed alongside this spec to keep the option visible.

## Decisions log

| Decision | Choice | Rationale |
| --- | --- | --- |
| Proto shape | Option C: hybrid oneof for new types only | Captures B's ergonomics for new types; avoids B's migration tax for existing types |
| `filter_sql` representation | Spark SQL fragment as string | Matches issue #433 wording; structured predicates would be a separate sub-design |
| `CompositeOperator` representation | Enum | Type-safe; matches "operator is recognized" in issue #433 |
| `window_hours` representation | `int32` hours | Matches ADR Appendix C; promote to `Duration` only if sub-hour granularity is needed |
| COMPOSITE scheduler dependency | Spin out as sibling P1 issue (Option B from brainstorm) | Keeps #432 focused on its acceptance criteria; scheduler work has its own reviewer focus |
| `assignment_probability` for COMPOSITE | Hard-coded to 1.0; flagged for Agent-4 coordination | Operand metrics encode their own; M4a behavior to be confirmed |

## References

- ADR-026 — `docs/adrs/026-custom-metrics-layer.md`
- ADR-014 (existing fields 15–16 on `MetricDefinition`) — `docs/adrs/014-multi-stakeholder-metrics.md`
- Current `MetricDefinition` proto — `proto/experimentation/common/v1/metric.proto`
- Existing M3 templates — `services/metrics/internal/spark/templates/`
- Existing M3 renderer — `services/metrics/internal/spark/renderer.go`
- Issue #432 (this spec's primary deliverable)
- Issue #433 (M5 validation, downstream consumer of this proto)
- Issue #434 (M6 UI, downstream consumer of this proto)
