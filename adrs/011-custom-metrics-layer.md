# ADR-011: Custom Metrics Definition Layer

- **Status**: Proposed
- **Date**: 2026-03-17
- **Author**: Agent-6 / Devin (requested by @wunderkennd)

## Context

Kaizen Experimentation currently supports six metric types via protobuf-defined `MetricDefinition` messages (M5 Management Service):

| Type | Aggregation | Example |
|------|-------------|---------|
| MEAN | `AVG(value)` per user | Watch time per session |
| PROPORTION | Binary event rate | Conversion rate |
| RATIO | Numerator sum / denominator sum (delta method for variance) | Revenue per session |
| COUNT | Event count per user | Sessions per week |
| PERCENTILE | `PERCENTILE_APPROX(value, p)` | p95 time-to-first-frame |
| CUSTOM | Raw Spark SQL expression | Arbitrary computation |

The first five types are **structured**: M3 (Metric Computation Engine) renders them via Go `text/template` SQL templates (`services/metrics/internal/spark/templates/*.sql.tmpl`), with type-specific validation in M5 (`services/management/internal/validation/metric.go`). These templates automatically handle exposure joins, variant grouping, CUPED covariate computation, lifecycle segmentation, and session-level aggregation.

The sixth type, **CUSTOM**, is an escape hatch: users provide raw Spark SQL in the `custom_sql` protobuf field. M3 wraps it in a CTE and joins it to the exposure table (`templates/custom.sql.tmpl`). The only validation is a regex blocklist of DDL/DML keywords (`spark/validate.go`).

### Problems with the Current CUSTOM Metric Approach

1. **No semantic validation** -- The regex blocklist (`CREATE|DROP|ALTER|...`) catches obvious DDL/DML but allows arbitrarily complex SQL that may be incorrect, non-performant, or reference non-existent tables/columns. There is no schema awareness.

2. **No composability** -- A CUSTOM metric cannot reference another metric's definition. If two metrics share a common subquery (e.g., "active users who watched > 30 min"), the SQL must be duplicated. Changes to the shared logic require updating every metric that embeds it.

3. **No lineage or impact analysis** -- When an upstream table schema changes (e.g., a column is renamed in Delta Lake), there is no way to identify which CUSTOM metrics break without executing them all. Standard metrics have explicit `source_event_type` fields that can be validated against the event catalog.

4. **No dry-run or preview** -- Users cannot preview what their CUSTOM SQL produces before attaching it to an experiment. A malformed metric is only discovered when M3 attempts to execute it during a scheduled job.

5. **Inconsistent statistical treatment** -- CUSTOM metrics must output `(user_id, metric_value)` rows, but this contract is enforced only by the CTE join pattern in `custom.sql.tmpl`. If the SQL returns duplicate user IDs or NULL values, M4a's statistical engine may produce incorrect results (inflated sample size, biased means).

6. **No governance or review workflow** -- Anyone with Experimenter role can create a CUSTOM metric with arbitrary SQL that runs against the production Delta Lake. There is no approval step, cost estimation, or access control beyond the role check.

7. **Spark-only execution** -- CUSTOM SQL is Spark SQL dialect. It cannot be used for real-time guardrail checks, ad-hoc exploration in Postgres, or cross-engine validation.

### Decision Drivers

- **Self-service**: Data scientists and product managers should define metrics without writing SQL
- **Safety**: Metrics must produce statistically valid inputs for M4a (one row per user, no NULLs, no duplicates)
- **Performance**: Metric computation must complete within the M3 scheduling window (hourly for guardrails, daily for full analysis)
- **Composability**: Complex metrics should be built from simpler, tested building blocks
- **Governance**: Metric definitions should be reviewable, versioned, and auditable
- **Compatibility**: The solution must work with the existing Spark-based M3 pipeline and protobuf contracts

## Decision

Adopt **Option F: Hybrid (Enhanced Protobuf + MetricQL)**, delivered in three phases.

**Phase 1 (weeks 1-3):** Add new structured protobuf metric types (`FILTERED_MEAN`, `COMPOSITE`, `WINDOWED_COUNT`) to cover the most common CUSTOM use cases (~60%) with full type-safe validation and zero new dependencies.

**Phase 2 (weeks 4-8):** Introduce MetricQL, a purpose-built expression language for complex/composed metrics. MetricQL expressions are parsed, validated against an event type catalog, and compiled directly to Spark SQL. This covers the remaining ~35% of CUSTOM use cases with composability (`@metric_ref` syntax) and semantic validation.

**Phase 3 (week 9+):** Deprecate the raw `custom_sql` field. Log warnings on new CUSTOM metric creation, migrate existing CUSTOM metrics to Tier 1 (structured) or Tier 2 (expression), and eventually remove CUSTOM from the M6 UI.

The system provides three tiers of metric definition:

| Tier | Definition Method | Validation | Use Case |
|------|-------------------|------------|----------|
| **Tier 1: Structured** | Protobuf fields (existing + new types) | Full type-safe validation at creation time | 80% of metrics: standard aggregations with filters |
| **Tier 2: Expression** | MetricQL string | Parser + semantic validation with event catalog lookup | 15% of metrics: composed/windowed/funnel metrics |
| **Tier 3: Raw SQL** (deprecated) | `custom_sql` field | Regex blocklist only (existing) | 5% of metrics: truly novel computations |

### Why this approach

1. **Phase 1 ships in 2-3 weeks** and immediately covers ~60% of CUSTOM metric use cases with zero new dependencies. This is the highest-ROI first step.

2. **Phase 2 ships in weeks 4-8** and provides a proper composition language for the remaining complex cases. By this point, real usage patterns from Phase 1 will inform the MetricQL grammar design.

3. **The phased approach manages risk.** If Phase 1 covers enough use cases, Phase 2 can be descoped or deferred. If MetricQL proves too complex, the team has a working system to fall back on.

4. **Spark compatibility is non-negotiable.** Options B (Malloy) and D (dbt) both introduce Spark integration challenges that add weeks of effort and ongoing maintenance. The hybrid approach compiles directly to Spark SQL via the existing M3 template infrastructure.

5. **Governance (Option E's strength) can be added later** as a cross-cutting concern on top of any option. It doesn't need to be built into the metric definition layer itself.

### What we explicitly do NOT recommend

- **Malloy (Option B):** The Spark SQL gap is a dealbreaker. Building and maintaining a Malloy->SparkSQL transpiler is not justified when MetricQL achieves the same composability goals with direct Spark compilation.

- **dbt (Option D):** Replacing M3's Go-based Spark SQL rendering with dbt is too invasive. dbt is excellent for data transformation pipelines but is a poor fit for an experimentation platform where metric computation is tightly coupled to exposure joins, variant grouping, and statistical analysis triggers.

- **Raw SQL as the long-term answer:** The current CUSTOM type should be treated as technical debt, not a feature.

## Consequences

### Positive

- Most metrics (80%+) use the simplest, safest path (structured protobuf) with full validation at creation time
- Complex metrics get a purpose-built language (MetricQL) instead of raw SQL, with semantic validation and composability
- Incremental delivery -- each phase ships independently and adds value
- Clear migration path from CUSTOM -> Expression -> Structured
- No new external runtime dependencies (Malloy compiler, dbt, etc.)
- Both new definition methods compile directly to Spark SQL, reusing existing M3 template infrastructure
- MetricQL `@metric_ref` syntax enables metric composition with automatic cycle detection
- Event type catalog validation provides dry-run capability at definition time

### Negative

- Three ways to define a metric creates cognitive overhead ("which tier do I use?")
- Two codepaths in M3 (template rendering for structured types, AST compilation for MetricQL expressions)
- MetricQL is a new internal language to build and maintain (~3 weeks for parser/compiler)
- The migration from CUSTOM to Expression requires manual rewriting of each existing metric
- Each new structured type (Option A path) still requires a new protobuf type, template, and validation logic

### Risks

- **MetricQL grammar proves insufficient for edge cases** (Medium likelihood, Medium impact) -- Mitigation: Keep CUSTOM type as deprecated fallback; extend grammar incrementally based on real usage
- **Composite metrics create expensive query plans** (Medium likelihood, High impact) -- Mitigation: Add query cost estimation (from Option E) as a follow-up
- **Team resistance to learning MetricQL** (Low likelihood, Medium impact) -- Mitigation: MetricQL is simpler than SQL; provide documentation and examples
- **Phase 1 types cover most needs, making Phase 2 unnecessary** (Medium likelihood, Low impact) -- This is a positive outcome; defer Phase 2 and save effort
- **MetricQL parser bugs produce incorrect SQL** (Medium likelihood, High impact) -- Mitigation: Comprehensive golden-file test suite; shadow-run MetricQL vs. CUSTOM SQL for existing metrics during migration

## Alternatives Considered

### Option A: Enhanced Protobuf DSL (Extend Current System)

**Approach:** Add new structured metric types to the existing protobuf schema to cover the most common CUSTOM use cases, reducing the need for raw SQL.

**New types proposed:**

```protobuf
enum MetricType {
  // ... existing types ...
  METRIC_TYPE_FILTERED_MEAN = 7;       // MEAN with WHERE clause predicates
  METRIC_TYPE_FUNNEL = 8;              // Multi-step conversion funnel
  METRIC_TYPE_RETENTION = 9;           // N-day retention rate
  METRIC_TYPE_COMPOSITE = 10;          // Arithmetic combination of other metrics
  METRIC_TYPE_WINDOWED_COUNT = 11;     // Count within time window of exposure
}

message FilterPredicate {
  string field = 1;         // e.g., "properties.platform"
  string operator = 2;      // e.g., "=", "IN", ">", "LIKE"
  repeated string values = 3;
}

message MetricDefinition {
  // ... existing fields ...
  repeated FilterPredicate filters = 15;           // For FILTERED_MEAN
  repeated string funnel_event_types = 16;         // For FUNNEL (ordered steps)
  int32 retention_day = 17;                        // For RETENTION (e.g., 7, 14, 30)
  repeated CompositeOperand composite_operands = 18; // For COMPOSITE
  int32 window_hours = 19;                         // For WINDOWED_COUNT
}

message CompositeOperand {
  string metric_id = 1;     // Reference to another MetricDefinition
  double coefficient = 2;   // Multiplier (e.g., 0.7 * watch_time + 0.3 * sessions)
  string operator = 3;      // "+", "-", "*", "/"
}
```

| Pros | Cons | Why rejected as standalone |
|------|------|---------------------------|
| Minimal architectural change, extends existing pattern | Each new use case requires a new protobuf type, template, and validation logic | Does not address composability for complex metrics; CUSTOM escape hatch remains |
| Full type safety via protobuf | Proto schema grows indefinitely | Adopted as Phase 1 of the hybrid approach (Option F) |
| No new runtime dependencies | Still no dry-run capability | |
| COMPOSITE type enables basic metric composition | | |

**Estimated effort:** 2-3 weeks

### Option B: Malloy Semantic Layer

**Approach:** Adopt [Malloy](https://www.malloydata.dev/) as a semantic modeling language for metric definitions. Metric authors write Malloy source files; M3 compiles Malloy to SQL at job execution time.

**Example:**

```malloy
source: metric_events is duckdb.table('delta.metric_events') extend {
  dimension: platform is properties.'platform'
  measure:
    active_watch_time is value.sum() { where: event_type = 'heartbeat' and value > 0 }
    engagement_score is active_watch_time / qualified_sessions
}
```

| Pros | Cons | Why rejected |
|------|------|--------------|
| Rich semantic modeling with composability | **No production Spark SQL backend** -- Malloy targets DuckDB, BigQuery, Postgres only | Spark SQL gap is a dealbreaker; building a transpiler adds 2-3 weeks + ongoing maintenance |
| Type-safe compiler catches errors at definition time | Pre-1.0, API stability not guaranteed | |
| Automatic lineage tracking | New language for team to learn | |

**Estimated effort:** 6-8 weeks

### Option C: MetricQL -- Domain-Specific Expression Language

**Approach:** Design a lightweight, purpose-built expression language for Kaizen metric definitions. MetricQL expressions are parsed, validated, and compiled to Spark SQL by M3.

**Example metric definitions:**

```
# Simple: proportion of users who started a stream
proportion(stream_start)

# Filtered mean: average watch time on mobile
mean(heartbeat.value) where properties.platform = 'mobile'

# Windowed: count of sessions within 7 days of exposure
count_distinct(session.session_id) within 7 days of exposure

# Composite: weighted engagement score referencing other metrics
0.7 * @watch_time_minutes + 0.3 * @sessions_per_week

# Ratio with referenced metrics
ratio(@total_revenue, @total_sessions)
```

**Grammar (EBNF sketch):**

```
metric_def   := aggregation '(' source ')' filter? window? ;
aggregation  := 'mean' | 'sum' | 'count' | 'count_distinct' | 'proportion'
              | 'percentile(' NUMBER ')' | 'ratio(' metric_ref ',' metric_ref ')' ;
source       := event_type ( '.' field )? ;
filter       := 'where' predicate ( 'and' predicate )* ;
predicate    := field_ref operator value ;
field_ref    := 'properties.' IDENTIFIER | IDENTIFIER ;
operator     := '=' | '!=' | '>' | '<' | '>=' | '<=' | 'in' | 'like' ;
value        := STRING | NUMBER | '[' value (',' value)* ']' ;
window       := 'within' NUMBER ('hours' | 'days') 'of' 'exposure' ;
metric_ref   := '@' IDENTIFIER ;
composite    := metric_ref ARITH_OP metric_ref ;
ARITH_OP     := '+' | '-' | '*' | '/' ;
```

| Pros | Cons | Why rejected as standalone |
|------|------|---------------------------|
| Purpose-built for experimentation metrics | Requires building and maintaining a parser/compiler (~3 weeks) | Excellent for complex metrics but overkill for simple filtered means; adopted as Phase 2 of hybrid |
| Compiles directly to Spark SQL | Another DSL for team to learn | |
| `@metric_ref` enables composition with cycle detection | Edge cases in grammar will emerge over time | |
| Semantic validation with event catalog | | |

**Estimated effort:** 5-6 weeks

### Option D: dbt Metrics Layer

**Approach:** Adopt [dbt](https://www.getdbt.com/) with [MetricFlow](https://docs.getdbt.com/docs/build/about-metricflow) to define metrics as dbt models. M3 invokes dbt to materialize metric tables.

| Pros | Cons | Why rejected |
|------|------|--------------|
| Industry-standard, large community | Heaviest architectural change -- replaces M3's Go-based Spark SQL rendering | Too invasive; exposure joins and variant grouping need custom dbt macros |
| Built-in lineage, documentation, data freshness checks | dbt-spark adapter has limitations | |
| Version-controlled YAML definitions | Two sources of truth (dbt YAML vs. M5 Postgres) | |
| MetricFlow provides composable measures | Adds Python + dbt to runtime dependencies | |

**Estimated effort:** 8-12 weeks

### Option E: SQL Builder with Governance Layer

**Approach:** Replace `custom_sql` with a structured SQL builder (`MetricSpec` protobuf) that provides guardrails, validation, composition, and an approval workflow with ACLs and cost controls.

| Pros | Cons | Why rejected as standalone |
|------|------|---------------------------|
| Full governance (approval workflow, ACLs, cost controls) | `MetricSpec` protobuf grows complex over time | Governance can be added as a cross-cutting concern on top of any option; doesn't need to be in the metric definition layer |
| Compiles directly to Spark SQL | `value_expression` is still a raw SQL fragment | |
| Cost estimation prevents expensive queries | Governance workflow adds friction for rapid iteration | |

**Estimated effort:** 5-7 weeks

### Decision Matrix

| Criterion (weight) | A: Enhanced Proto | B: Malloy | C: MetricQL | D: dbt | E: SQL Builder | F: Hybrid (A+C) |
|---|---|---|---|---|---|---|
| **Self-service** (20%) | Medium | High | High | High | Medium | High |
| **Safety** (20%) | High | High | High | High | High | High |
| **Spark compatibility** (15%) | Native | **Low** | Native | Medium | Native | Native |
| **Composability** (15%) | Medium | High | High | High | Medium | High |
| **Implementation effort** (10%) | **Low (2-3w)** | High (6-8w) | Medium (5-6w) | **Very High (8-12w)** | Medium (5-7w) | Medium-High (8-10w, phased) |
| **Governance** (10%) | Low | Low | Low | Medium | **High** | Low-Medium |
| **Ecosystem/community** (5%) | N/A | Growing, pre-1.0 | N/A | **Large, mature** | N/A | N/A |
| **Migration risk** (5%) | **Very Low** | High | Medium | **Very High** | Medium | Medium (phased) |

**Weighted scores** (5-point scale, higher is better):

| Option | Score |
|--------|-------|
| A: Enhanced Protobuf | 3.4 |
| B: Malloy | 2.9 |
| C: MetricQL | 3.7 |
| D: dbt Metrics Layer | 2.8 |
| E: SQL Builder + Governance | 3.5 |
| **F: Hybrid (A + C)** | **3.8** |

## Implementation Plan

### Phase 1: Enhanced Protobuf Types (Weeks 1-3)

**Week 1:**
- Add `FILTERED_MEAN`, `COMPOSITE`, `WINDOWED_COUNT` to `MetricType` enum
- Add `FilterPredicate`, `CompositeOperand`, `WindowSpec` messages to `metric.proto`
- Implement M5 validation for new types (filter field allowlist, composite cycle detection, window bounds)

**Week 2:**
- Implement `filtered_mean.sql.tmpl`, `composite.sql.tmpl`, `windowed_count.sql.tmpl` in M3
- Add `RenderForType` cases in `renderer.go`
- Golden-file tests for each new template

**Week 3:**
- M6 UI: filter builder component, composite metric form, window configuration
- Audit existing CUSTOM metrics and migrate those that fit the new types
- Documentation in DocMost

### Phase 2: MetricQL Expression Language (Weeks 4-8)

**Week 4-5:**
- Design final MetricQL grammar based on Phase 1 usage patterns
- Implement lexer, parser, and AST in `pkg/metricql/`
- Comprehensive parser tests (valid expressions, error messages, edge cases)

**Week 6:**
- Implement semantic analyzer: event type validation against catalog, `@metric_ref` resolution, cycle detection
- Add `ListEventTypes` RPC to M5 for catalog-aware validation

**Week 7:**
- Implement SQL code generator: AST -> Spark SQL
- Golden-file tests comparing generated SQL against hand-written expected output
- Integration test: MetricQL -> SQL -> Spark execution -> M4a analysis

**Week 8:**
- M6 UI: expression editor with syntax highlighting and autocomplete
- Migrate remaining CUSTOM metrics to MetricQL expressions
- Deprecation warning on `custom_sql` field

### Phase 3: Deprecation (Week 9+)

- Log warnings when CUSTOM metrics are created via API
- M6 UI: remove CUSTOM type from metric creation form (API still accepts it)
- Monitor for any remaining CUSTOM metric usage
- After 2 release cycles with zero new CUSTOM metrics: remove from UI entirely

## Appendix

### A. Comparison to Industry Approaches

| Platform | Metric Definition Approach |
|----------|---------------------------|
| **Spotify (Confidence)** | Structured metric types in internal config system; complex metrics via PySpark UDFs registered in a metric catalog |
| **Netflix (XP)** | SQL-based metric definitions stored in a metadata service; metrics are SQL snippets that are composed into larger queries |
| **Eppo** | YAML-based metric definitions with a fact-based semantic model; SQL is generated from the model |
| **Statsig** | Structured metric types (event count, aggregation, ratio, funnel) with filter predicates; custom SQL for power users |
| **GrowthBook** | SQL-based with a Jinja-like templating system for composition; metrics defined per data source |
| **LaunchDarkly (prev. Catamorphic)** | Event-based metric types (conversion, numeric, custom); limited composition |

Our recommended approach (Hybrid: structured types + expression language) is most similar to **Statsig's** model (structured types for common cases, SQL escape hatch for complex) but replaces the raw SQL escape hatch with a safer, purpose-built expression language.

### B. Event Types Available in Kaizen (for MetricQL Catalog Validation)

Based on the current `MetricEvent.event_type` values observed in seed data and documentation:

```
stream_start, heartbeat, stream_end, search, impression, revenue,
session, playback_start, playback_error, qoe_rebuffer, composite,
click, add_to_list, share, rating, download, subscription_change
```

### C. Proto Changes (Proposed for Discussion)

```protobuf
message MetricDefinition {
  // ... existing fields 1-14 ...

  // Tier 1 extensions (from Option A)
  repeated FilterPredicate filters = 15;
  repeated string funnel_event_types = 16;
  int32 retention_day = 17;
  repeated CompositeOperand composite_operands = 18;
  int32 window_hours = 19;

  // Tier 2: MetricQL expression (from Option C)
  string expression = 20;

  // Tier 3: Raw SQL (existing field 9, now deprecated)
  // string custom_sql = 9; // DEPRECATED: use expression or structured fields instead
}
```

## References

- `proto/experimentation/common/v1/metric.proto` -- Current MetricDefinition schema
- `services/metrics/internal/spark/renderer.go` -- SQL template rendering
- `services/metrics/internal/spark/validate.go` -- Custom SQL validation
- `services/metrics/internal/spark/templates/custom.sql.tmpl` -- Custom metric template
- `services/metrics/internal/querylog/writer.go` -- Query audit logging
- `services/management/internal/validation/metric.go` -- Metric creation validation
- ADR-001: Language Selection (Rust for hot paths, Go for orchestration)
- ADR-010: ConnectRPC as RPC Framework
- [Malloy Language](https://www.malloydata.dev/) -- Semantic data modeling language
- [dbt MetricFlow](https://docs.getdbt.com/docs/build/about-metricflow) -- dbt semantic layer
- [Eppo Metric Definitions](https://docs.geteppo.com/data-management/metrics/) -- Fact-based metric model
- [Statsig Metrics](https://docs.statsig.com/metrics) -- Structured metric types
