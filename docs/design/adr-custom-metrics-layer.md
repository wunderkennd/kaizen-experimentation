# ADR: Custom Metrics Definition Layer

**Status:** Proposed
**Date:** 2026-03-17
**Author:** Agent-6 / Devin (requested by @wunderkennd)
**Deciders:** Platform Engineering, Data Engineering, Product

---

## 1. Context

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

---

## 2. Alternatives Considered

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

**New SQL templates:**
- `filtered_mean.sql.tmpl` -- Standard mean template with injected `WHERE` clause from `FilterPredicate` list
- `funnel.sql.tmpl` -- Sequential event matching with time ordering
- `retention.sql.tmpl` -- Compares activity before/after exposure + N days
- `composite.sql.tmpl` -- Joins pre-computed metric results and applies arithmetic
- `windowed_count.sql.tmpl` -- Count events within `window_hours` of exposure timestamp

**Changes required:**

| Component | Change | Effort |
|-----------|--------|--------|
| `proto/common/v1/metric.proto` | Add new types, filter predicates, composite operands | 1 day |
| `services/metrics/internal/spark/templates/` | 5 new SQL templates | 3 days |
| `services/metrics/internal/spark/renderer.go` | New `RenderForType` cases | 1 day |
| `services/management/internal/validation/metric.go` | Validation for new types (funnel ordering, composite cycles, etc.) | 2 days |
| M6 UI (metric creation form) | Form fields for filters, funnel steps, retention window | 3 days |

**Pros:**
- Minimal architectural change -- extends the existing pattern
- Full type safety via protobuf; all validation happens at creation time in M5
- New templates get the same exposure join, CUPED, lifecycle, and session-level support as standard metrics for free
- No new runtime dependencies
- COMPOSITE type enables metric composition without raw SQL
- Filtered means cover ~60% of observed CUSTOM metric use cases (source: typical experimentation platforms)

**Cons:**
- Each new use case requires a new protobuf type, template, and validation logic -- the type enum grows indefinitely
- Composite metrics introduce dependency graphs that need cycle detection and topological ordering in M3
- Still no dry-run capability (validation is structural, not data-aware)
- CUSTOM type remains as a last-resort escape hatch
- The proto schema becomes increasingly complex for downstream consumers

**Estimated effort:** 2-3 weeks

---

### Option B: Malloy Semantic Layer

**Approach:** Adopt [Malloy](https://www.malloydata.dev/) as a semantic modeling language for metric definitions. Metric authors write Malloy source files that define reusable measures, dimensions, and queries. M3 compiles Malloy to SQL at job execution time.

**Architecture:**

```
User defines metric in Malloy DSL
         |
         v
  M5 stores Malloy source in metric_definitions table (new `malloy_source` field)
         |
         v
  M3 at job time: Malloy compiler -> Spark SQL (via DuckDB SQL dialect + transpilation)
         |
         v
  Standard M3 pipeline: execute SQL, join to exposures, write to Delta Lake
```

**Example Malloy metric definition:**

```malloy
source: metric_events is duckdb.table('delta.metric_events') extend {
  dimension: platform is properties.'platform'
  dimension: content_genre is properties.'genre'

  measure:
    active_watch_time is value.sum() { where: event_type = 'heartbeat' and value > 0 }
    qualified_sessions is count(distinct session_id) { where: value > 1800 }
    engagement_score is active_watch_time / qualified_sessions
}
```

**Changes required:**

| Component | Change | Effort |
|-----------|--------|--------|
| `proto/common/v1/metric.proto` | Add `string malloy_source = 16` field | 0.5 days |
| M3: New `malloy/` package | Malloy compiler integration (Go SDK or subprocess) | 2 weeks |
| M3: SQL transpilation layer | Malloy outputs DuckDB SQL; need DuckDB->Spark SQL transpiler | 2-3 weeks |
| M5: Validation | Parse Malloy source, validate references, check for side effects | 1 week |
| M6 UI: Malloy editor | Syntax-highlighted editor with autocomplete | 2 weeks |
| Infrastructure | Malloy compiler binary in Docker images | 1 day |

**Pros:**
- Rich semantic modeling -- measures, dimensions, joins, and filters are first-class concepts
- Composability is native -- measures reference other measures, sources extend other sources
- Type-safe -- Malloy compiler catches reference errors, type mismatches, and ambiguous joins at definition time
- Lineage is automatic -- the Malloy compiler knows which tables/columns each measure depends on
- Growing ecosystem with VS Code extension, documentation, and community

**Cons:**
- **Spark SQL gap is the critical blocker** -- Malloy compiles to DuckDB, BigQuery, or Postgres SQL. Kaizen's M3 pipeline runs Spark SQL. There is no production-ready Malloy->SparkSQL backend. Building one is a significant undertaking (~2-3 weeks minimum, ongoing maintenance).
- Adds a new language to the platform that the team must learn and maintain
- Malloy is still pre-1.0 (as of March 2026) -- API stability is not guaranteed
- Runtime compilation adds latency to M3 job scheduling (acceptable for daily, concerning for hourly guardrails)
- The Malloy model becomes a second source of truth alongside protobuf -- synchronization risk
- Debugging generated SQL is harder than debugging hand-written SQL

**Estimated effort:** 6-8 weeks

---

### Option C: MetricQL -- Domain-Specific Expression Language

**Approach:** Design a lightweight, purpose-built expression language for Kaizen metric definitions. MetricQL expressions are parsed, validated, and compiled to Spark SQL by M3. The language is intentionally narrow -- it only expresses metric computations, not general-purpose queries.

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
metric_ref   := '@' IDENTIFIER ;                // reference to another metric
composite    := metric_ref ARITH_OP metric_ref ; // arithmetic combination
ARITH_OP     := '+' | '-' | '*' | '/' ;
```

**Example metric definitions:**

```
# Simple: proportion of users who started a stream
proportion(stream_start)

# Filtered mean: average watch time on mobile
mean(heartbeat.value) where properties.platform = 'mobile'

# Windowed: count of sessions within 7 days of exposure
count_distinct(session.session_id) within 7 days of exposure

# Funnel: search -> click -> play (implicit sequential ordering)
funnel(search, click, stream_start)

# Composite: weighted engagement score referencing other metrics
0.7 * @watch_time_minutes + 0.3 * @sessions_per_week

# Retention: proportion of users active 7 days after exposure
proportion(session) within 7 days of exposure

# Ratio with referenced metrics
ratio(@total_revenue, @total_sessions)
```

**Architecture:**

```
User writes MetricQL expression (string)
         |
         v
  M5: Parser -> AST -> Semantic validation (event types exist, fields exist, no cycles)
         |
         v
  Stored in metric_definitions.expression (new proto field)
         |
         v
  M3 at job time: AST -> Spark SQL (direct code generation, no intermediate language)
         |
         v
  Standard M3 pipeline: execute SQL, join to exposures, write to Delta Lake
```

**Changes required:**

| Component | Change | Effort |
|-----------|--------|--------|
| New package: `pkg/metricql/` | Lexer, parser, AST, semantic analyzer, SQL code generator | 3 weeks |
| `proto/common/v1/metric.proto` | Add `string expression = 16` field, deprecate `custom_sql` | 0.5 days |
| M5: `validation/metric.go` | Parse expression, run semantic validation (event type catalog lookup) | 1 week |
| M3: `spark/renderer.go` | New `RenderExpression(ast)` method that generates Spark SQL from AST | 1 week |
| M6 UI: Expression editor | Syntax-highlighted input with autocomplete and live preview | 2 weeks |
| M5: Event type catalog | New `ListEventTypes` RPC that queries Kafka schema registry or Delta Lake metadata | 1 week |

**Pros:**
- Purpose-built for experimentation metrics -- every construct maps directly to a Kaizen concept
- No external dependencies -- the parser/compiler is owned code, tightly integrated with M3 templates
- Compiles directly to Spark SQL -- no intermediate language or transpilation gap
- Expressions are short, readable, and diffable (version control friendly)
- Semantic validation can check event types against a live catalog at definition time (dry-run)
- The `@metric_ref` syntax enables composition with automatic cycle detection
- Can be extended incrementally (add `funnel()`, `retention()`, `percentile()` as needed)
- SQL generation reuses existing `exposure_join.sql.tmpl` and CUPED patterns

**Cons:**
- Requires building and maintaining a parser/compiler (non-trivial, ~3 weeks upfront)
- Another DSL for the team to learn (though simpler than Malloy or full SQL)
- Edge cases in the grammar will emerge over time -- needs ongoing language design work
- The expression language may eventually need features that make it resemble SQL anyway (subqueries, joins)
- Testing the compiler requires comprehensive golden-file tests to ensure SQL correctness

**Estimated effort:** 5-6 weeks

---

### Option D: dbt Metrics Layer

**Approach:** Adopt [dbt (data build tool)](https://www.getdbt.com/) with its [MetricFlow semantic layer](https://docs.getdbt.com/docs/build/about-metricflow) to define metrics as dbt models. Metric definitions live in `.yml` files alongside dbt models. M3 invokes dbt to materialize metric tables, then reads the results.

**Example dbt metric definition:**

```yaml
# models/metrics/schema.yml
semantic_models:
  - name: metric_events
    defaults:
      agg_time_dimension: timestamp
    model: ref('stg_metric_events')
    entities:
      - name: user
        type: primary
        expr: user_id
    dimensions:
      - name: platform
        type: categorical
        expr: "properties['platform']"
      - name: event_type
        type: categorical
    measures:
      - name: total_watch_time
        agg: sum
        expr: value
        filter: "{{ Dimension('event_type') }} = 'heartbeat'"
      - name: stream_starts
        agg: count
        filter: "{{ Dimension('event_type') }} = 'stream_start'"
      - name: unique_sessions
        agg: count_distinct
        expr: session_id

metrics:
  - name: avg_watch_time_per_user
    type: derived
    label: "Average Watch Time per User"
    type_params:
      expr: total_watch_time / unique_sessions
  - name: stream_start_rate
    type: ratio
    label: "Stream Start Rate"
    type_params:
      numerator:
        name: stream_starts
      denominator:
        name: unique_sessions
```

**Architecture:**

```
User defines metric in dbt YAML (via UI form or direct file edit)
         |
         v
  Stored in git repo (dbt project) -- version controlled, PR-reviewable
         |
         v
  M3 at job time: dbt run -> materializes metric tables in Delta Lake / warehouse
         |
         v
  M4a reads materialized tables for statistical analysis
```

**Changes required:**

| Component | Change | Effort |
|-----------|--------|--------|
| New repo/directory: `metrics-dbt/` | dbt project with staging models, semantic models, metric definitions | 2 weeks |
| M3: `jobs/` package | Replace direct Spark SQL execution with dbt invocation (dbt CLI or dbt Cloud API) | 2-3 weeks |
| M5: Metric CRUD | Either: (a) M5 generates dbt YAML from protobuf and commits to repo, or (b) metrics are defined in dbt and M5 reads them | 2 weeks |
| M4a: Result ingestion | Read from dbt-materialized tables instead of M3's current output format | 1 week |
| Infrastructure | dbt Core in Docker images, or dbt Cloud account + API integration | 1 week |
| M6 UI: Metric form | Either a YAML editor or a form that maps to dbt metric YAML | 2 weeks |

**Pros:**
- Industry-standard tool with large community, extensive documentation, and wide adoption
- MetricFlow semantic layer provides composable measures, dimensions, and derived metrics
- Built-in lineage tracking, documentation generation, and data freshness checks
- Version-controlled metric definitions in git (PR review workflow built in)
- dbt tests validate data quality (not null, unique, accepted values) at materialization time
- Supports Spark (via dbt-spark adapter), Postgres, BigQuery, Snowflake -- multi-engine compatible
- dbt Cloud provides a managed execution environment with scheduling, logging, and alerting

**Cons:**
- **Heaviest architectural change** -- M3's Go-based Spark SQL rendering would be replaced by dbt's Python-based compilation. The entire M3 job scheduling model changes.
- dbt expects to own the materialization lifecycle. Integrating it with M3's existing hourly/daily scheduling and M4a's analysis trigger requires careful orchestration.
- Two sources of truth risk: metric metadata in dbt YAML vs. experiment config in M5 Postgres. Need a synchronization mechanism.
- dbt-spark adapter has limitations (no incremental models with merge strategy, limited snapshot support)
- Adds Python and dbt to the runtime dependency graph (currently Go + Rust only in backend services)
- dbt Cloud is a paid service; dbt Core is free but requires self-hosting the scheduler
- Exposure joins and variant grouping are Kaizen-specific -- these would need custom dbt macros, reducing the benefit of the standard dbt ecosystem
- Team needs to learn dbt (YAML config, Jinja templating, ref() semantics, MetricFlow)

**Estimated effort:** 8-12 weeks

---

### Option E: SQL Builder with Governance Layer

**Approach:** Replace the raw `custom_sql` field with a structured SQL builder that provides guardrails, validation, and composition while still allowing full SQL expressiveness. The builder is a Go library in M3 that constructs SQL from a declarative specification.

**Metric specification format (JSON/protobuf):**

```protobuf
message MetricSpec {
  string metric_id = 1;
  string name = 2;

  // Source configuration
  string source_table = 3;           // e.g., "delta.metric_events"
  string value_expression = 4;       // e.g., "value", "CAST(properties['duration'] AS DOUBLE)"
  string user_id_field = 5;          // default: "user_id"

  // Aggregation
  AggregationType aggregation = 6;   // MEAN, SUM, COUNT, COUNT_DISTINCT, PROPORTION, PERCENTILE
  double percentile_value = 7;       // for PERCENTILE

  // Filters
  repeated FilterClause filters = 8;

  // Composition
  repeated MetricReference numerator_refs = 9;   // for DERIVED metrics
  repeated MetricReference denominator_refs = 10;
  string derived_expression = 11;                 // e.g., "@revenue / @sessions"

  // Windowing
  WindowSpec window = 12;

  // Governance
  string approved_by = 13;
  string approval_ticket = 14;       // e.g., JIRA ticket
  repeated string allowed_tables = 15; // ACL: which Delta Lake tables this metric may query
  int64 max_scan_bytes = 16;         // cost control: max bytes scanned per execution
}

message FilterClause {
  string field = 1;
  string operator = 2;
  oneof value {
    string string_value = 3;
    double numeric_value = 4;
    StringList string_list = 5;
  }
}

message WindowSpec {
  int32 duration = 1;
  string unit = 2;  // "hours" or "days"
  string anchor = 3; // "exposure" or "absolute"
}

message MetricReference {
  string metric_id = 1;
  double coefficient = 2;
}
```

**Architecture:**

```
User fills structured form in M6 UI
         |
         v
  M5: Validates MetricSpec (schema checks, ACL checks, cycle detection, cost estimation)
         |
         v
  Stored as MetricSpec protobuf in Postgres
         |
         v
  M3 at job time: SQL Builder compiles MetricSpec -> Spark SQL
         |              (reuses existing templates + adds filter/window/composition logic)
         v
  Standard M3 pipeline: execute SQL, join to exposures, write to Delta Lake
```

**Changes required:**

| Component | Change | Effort |
|-----------|--------|--------|
| `proto/common/v1/metric.proto` | Add `MetricSpec` message alongside existing `MetricDefinition` | 1 day |
| New package: `services/metrics/internal/builder/` | SQL builder that compiles `MetricSpec` to Spark SQL | 2 weeks |
| `services/management/internal/validation/` | MetricSpec validation (ACL, cost estimation, cycle detection) | 1 week |
| M5: Governance RPCs | `SubmitMetricForApproval`, `ApproveMetric`, `RejectMetric` | 1 week |
| M3: `spark/renderer.go` | New `RenderFromSpec(spec)` method | 1 week |
| M6 UI: Metric builder form | Structured form with filter builder, aggregation picker, composition UI | 2 weeks |
| M5: Cost estimation | Query Delta Lake table metadata (`DESCRIBE DETAIL`) to estimate scan cost | 1 week |

**Pros:**
- Full governance -- approval workflow, ACLs, cost controls
- Structured enough for validation but flexible enough for complex metrics
- Compiles directly to Spark SQL -- no transpilation gap
- The `MetricSpec` protobuf is the single source of truth (no second language or tool)
- Backward compatible -- existing `MetricDefinition` types continue to work; `MetricSpec` is an alternative for complex cases
- Cost estimation prevents expensive queries from being scheduled
- ACL per metric limits blast radius (e.g., a metric can only query `delta.metric_events`, not `delta.users`)
- The SQL builder can reuse existing M3 templates as building blocks

**Cons:**
- The `MetricSpec` protobuf can become complex as more features are added (similar growth problem as Option A)
- `value_expression` is still a raw SQL fragment for the SELECT clause -- not fully validated
- The governance workflow (approval, rejection) adds friction for rapid iteration
- Building a cost estimator that works across Delta Lake tables is non-trivial
- Derived metrics with `derived_expression` still have the composability concerns of raw SQL (though more constrained)

**Estimated effort:** 5-7 weeks

---

### Option F: Hybrid -- Enhanced Protobuf + MetricQL for Complex Cases

**Approach:** Combine Option A (enhanced protobuf types for common patterns) with Option C (MetricQL for complex cases). Standard metrics use the structured protobuf types. Complex metrics use MetricQL expressions. CUSTOM raw SQL is deprecated but remains for backward compatibility.

**Three tiers of metric definition:**

| Tier | Definition Method | Validation | Use Case |
|------|-------------------|------------|----------|
| **Tier 1: Structured** | Protobuf fields (existing + new types from Option A) | Full type-safe validation at creation time | 80% of metrics: standard aggregations with filters |
| **Tier 2: Expression** | MetricQL string (Option C) | Parser + semantic validation with event catalog lookup | 15% of metrics: composed/windowed/funnel metrics |
| **Tier 3: Raw SQL** (deprecated) | `custom_sql` field | Regex blocklist only (existing) | 5% of metrics: truly novel computations |

**Proto changes:**

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

**Migration path:**

1. **Phase 1 (weeks 1-3):** Ship Option A -- new protobuf types, templates, validation. Migrate existing CUSTOM metrics that fit the new types.
2. **Phase 2 (weeks 4-8):** Ship MetricQL parser/compiler. Migrate remaining CUSTOM metrics to expressions.
3. **Phase 3 (week 9+):** Deprecate `custom_sql` field. Log warnings when CUSTOM metrics are created. Eventually remove after all metrics are migrated.

**Pros:**
- Incremental delivery -- each phase ships independently and adds value
- Most metrics (80%+) use the simplest, safest path (structured protobuf)
- Complex metrics get a purpose-built language (MetricQL) instead of raw SQL
- Clear migration path from CUSTOM -> Expression -> Structured
- Raw SQL remains available as a safety valve during migration

**Cons:**
- Three ways to define a metric creates cognitive overhead (which tier do I use?)
- Two codepaths in M3 (template rendering for structured, AST compilation for expressions)
- MetricQL is still a new language to build and maintain
- The migration from CUSTOM to Expression requires manual rewriting of each metric

**Estimated effort:** 8-10 weeks (phased over 3 months)

---

## 3. Decision Matrix

| Criterion (weight) | A: Enhanced Proto | B: Malloy | C: MetricQL | D: dbt | E: SQL Builder | F: Hybrid (A+C) |
|---|---|---|---|---|---|---|
| **Self-service** (20%) | Medium -- still requires understanding proto field semantics | High -- rich modeling language | High -- concise expressions | High -- YAML + MetricFlow | Medium -- structured but verbose form | High -- simple for common, expressive for complex |
| **Safety** (20%) | High -- full proto validation | High -- compiler catches errors | High -- parser + semantic validation | High -- dbt tests + schema checks | High -- validation + governance | High -- best of both |
| **Spark compatibility** (15%) | Native -- extends existing templates | **Low -- no Spark backend** | Native -- compiles directly to Spark SQL | Medium -- dbt-spark adapter has limitations | Native -- extends existing builder | Native |
| **Composability** (15%) | Medium -- COMPOSITE type only | High -- native in language | High -- `@metric_ref` syntax | High -- MetricFlow derived metrics | Medium -- `MetricReference` proto | High |
| **Implementation effort** (10%) | **Low (2-3 weeks)** | High (6-8 weeks) | Medium (5-6 weeks) | **Very High (8-12 weeks)** | Medium (5-7 weeks) | Medium-High (8-10 weeks, phased) |
| **Governance** (10%) | Low -- no approval workflow | Low -- no built-in governance | Low -- no built-in governance | Medium -- git PR workflow | **High -- approval + ACL + cost** | Low-Medium |
| **Ecosystem/community** (5%) | N/A (internal) | Growing but pre-1.0 | N/A (internal) | **Large, mature** | N/A (internal) | N/A (internal) |
| **Migration risk** (5%) | **Very Low** -- additive | High -- new runtime dependency | Medium -- new parser to maintain | **Very High** -- replaces M3 core | Medium -- new proto schema | Medium -- phased rollout |

**Weighted scores** (5-point scale, higher is better):

| Option | Score |
|--------|-------|
| A: Enhanced Protobuf | 3.4 |
| B: Malloy | 2.9 |
| C: MetricQL | 3.7 |
| D: dbt Metrics Layer | 2.8 |
| E: SQL Builder + Governance | 3.5 |
| F: Hybrid (A + C) | **3.8** |

---

## 4. Recommendation

**Option F: Hybrid (Enhanced Protobuf + MetricQL)** is the recommended approach, delivered in phases.

### Rationale

1. **Phase 1 (Enhanced Protobuf) ships in 2-3 weeks** and immediately covers ~60% of CUSTOM metric use cases with zero new dependencies. This is the highest-ROI first step.

2. **Phase 2 (MetricQL) ships in weeks 4-8** and provides a proper composition language for the remaining complex cases. By this point, real usage patterns from Phase 1 will inform the MetricQL grammar design.

3. **The phased approach manages risk.** If Phase 1 covers enough use cases, Phase 2 can be descoped or deferred. If MetricQL proves too complex, the team has a working system to fall back on.

4. **Spark compatibility is non-negotiable.** Options B (Malloy) and D (dbt) both introduce Spark integration challenges that add weeks of effort and ongoing maintenance. Options A, C, E, and F all compile directly to Spark SQL via the existing M3 template infrastructure.

5. **Governance (Option E's strength) can be added later** as a cross-cutting concern on top of any option. It doesn't need to be built into the metric definition layer itself.

### What we explicitly do NOT recommend

- **Malloy (Option B):** The Spark SQL gap is a dealbreaker. Building and maintaining a Malloy->SparkSQL transpiler is not justified when MetricQL achieves the same composability goals with direct Spark compilation.

- **dbt (Option D):** Replacing M3's Go-based Spark SQL rendering with dbt is too invasive. dbt is excellent for data transformation pipelines but is a poor fit for an experimentation platform where metric computation is tightly coupled to exposure joins, variant grouping, and statistical analysis triggers.

- **Raw SQL as the long-term answer:** The current CUSTOM type should be treated as technical debt, not a feature. It will remain available but should be actively migrated away from.

---

## 5. Implementation Plan

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

---

## 6. Appendix

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

### C. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| MetricQL grammar proves insufficient for edge cases | Medium | Medium | Keep CUSTOM type as deprecated fallback; extend grammar incrementally |
| Composite metrics create expensive query plans | Medium | High | Add query cost estimation (from Option E) as a follow-up |
| Team resistance to learning MetricQL | Low | Medium | MetricQL is simpler than SQL; provide documentation and examples |
| Phase 1 types cover most needs, making Phase 2 unnecessary | Medium | Low | This is a positive outcome -- defer Phase 2 and save effort |
| MetricQL parser bugs produce incorrect SQL | Medium | High | Comprehensive golden-file test suite; shadow-run MetricQL vs. CUSTOM SQL for existing metrics |

---

## 7. References

- `proto/experimentation/common/v1/metric.proto` -- Current MetricDefinition schema
- `services/metrics/internal/spark/renderer.go` -- SQL template rendering
- `services/metrics/internal/spark/validate.go` -- Custom SQL validation
- `services/metrics/internal/spark/templates/custom.sql.tmpl` -- Custom metric template
- `services/metrics/internal/querylog/writer.go` -- Query audit logging
- `services/management/internal/validation/metric.go` -- Metric creation validation
- [Malloy Language](https://www.malloydata.dev/) -- Semantic data modeling language
- [dbt MetricFlow](https://docs.getdbt.com/docs/build/about-metricflow) -- dbt semantic layer
- [Eppo Metric Definitions](https://docs.geteppo.com/data-management/metrics/) -- Fact-based metric model
- [Statsig Metrics](https://docs.statsig.com/metrics) -- Structured metric types
