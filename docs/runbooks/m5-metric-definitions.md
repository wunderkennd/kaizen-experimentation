# M5 Custom Metric Definitions — Operator Runbook

## Service Overview

**Binary**: `experimentation-management` (Rust)
**Port**: 50055 (gRPC)
**Profile**: Control plane — synchronous CRUD against PostgreSQL
**State**: Stateful — metric definitions live in `metric_definitions` (PG)
**Audience**: Data engineers, experimentation owners, anyone authoring metrics

### What it does

Hosts the CRUD surface for `MetricDefinition` resources. Phase 1 of ADR-026 added three structured custom metric types — **FILTERED_MEAN**, **COMPOSITE**, **WINDOWED_COUNT** — alongside the existing six built-in types (MEAN, PROPORTION, RATIO, COUNT, PERCENTILE, CUSTOM).

This runbook walks through creating each of the three new types via the M6 UI form or `grpcurl`.

### RPCs (Phase 1 scope)

| RPC | Purpose |
|-----|---------|
| `CreateMetricDefinition` | Create a new metric of any type |
| `GetMetricDefinition` | Fetch a metric by `metric_id` |
| `ListMetricDefinitions` | List metrics with optional `type` filter |

Update and delete RPCs do **not** exist; metrics are immutable by design. Mutating an in-flight metric would silently invalidate historical experiment comparisons — author a new metric with a new ID instead. See ADR-026 § "Implementation status" and #434 for the spec discussion.

### Dependencies

| Dependency | Required | Failure mode |
|------------|----------|--------------|
| PostgreSQL (`metric_definitions`) | Yes | `CreateMetricDefinition` returns `Internal`; reads fail |
| M3 Metrics service | No (decoupled) | Existing metrics keep computing; M5 just rejects new defs that fail validation |

---

## Configuration

| Env var | Default | Description |
|---------|---------|-------------|
| `DATABASE_URL` | — | PostgreSQL DSN; migrations 001–011 must be applied |
| `MANAGEMENT_GRPC_ADDR` | `0.0.0.0:50055` | gRPC listen address |
| `KAFKA_BROKERS` | — | Kafka brokers for lifecycle / guardrail topics |
| `KAFKA_ENABLED` | `false` | Set `true` to enable Kafka emission |
| `RUST_LOG` | `info` | Log level (`debug` surfaces validator decisions) |

Dev / staging / prod DSN sources: see `docs/runbooks/artifact-registry-credentials.md` and `infra/Pulumi.{dev,staging,prod}.yaml`.

### Startup

```bash
DATABASE_URL="postgres://user:pass@db:5432/experimentation" \
MANAGEMENT_GRPC_ADDR="0.0.0.0:50055" \
RUST_LOG=info \
./experimentation-management
```

---

## Creating Metrics

All three Phase 1 types share the common `MetricDefinition` envelope: `metric_id`, `name`, `type`, `stakeholder` (USER / PROVIDER / PLATFORM), `aggregation_level` (USER / EXPERIMENT / PROVIDER), `lower_is_better`. The per-type config goes in the `type_config` oneof.

### FILTERED_MEAN

**When to use**: average of a numeric column, restricted to rows matching a WHERE-clause predicate.

> **Anti-pattern**: FILTERED_MEAN with an empty filter. Use `METRIC_TYPE_MEAN` instead — same computation, simpler shape. M5 rejects empty `filter_sql` with the hint `use METRIC_TYPE_MEAN if no filter is needed`.

**Via the M6 UI**:
1. Navigate to `/metrics/new`.
2. Set **Metric Type** to `Filtered Mean`.
3. Fill in common fields (metric_id, name, stakeholder, aggregation level, source event type).
4. In the FILTERED_MEAN section:
   - **Value column** — the numeric column to average (e.g. `duration_ms`).
   - **Filter SQL** — a WHERE predicate (e.g. `platform = 'mobile' AND duration_ms > 5000`).
5. The form preview shows the proto-JSON payload that will be submitted.
6. Click **Create Metric**.

**Via grpcurl**:

```bash
grpcurl -plaintext [::1]:50055 \
  experimentation.management.v1.ExperimentManagementService/CreateMetricDefinition \
  -d '{
    "metric": {
      "metricId": "mobile_watch_time_filtered",
      "name": "Mobile Watch Time (FILTERED_MEAN)",
      "type": "METRIC_TYPE_FILTERED_MEAN",
      "sourceEventType": "heartbeat",
      "stakeholder": "METRIC_STAKEHOLDER_USER",
      "aggregationLevel": "METRIC_AGGREGATION_LEVEL_USER",
      "filteredMean": {
        "filterSql": "platform = '\''mobile'\'' AND duration_ms > 5000",
        "valueColumn": "duration_ms"
      }
    }
  }'
```

**Constraints** (enforced by M5; UI surfaces some inline):

| Field | Rule |
|-------|------|
| `value_column` | matches `^[a-z_][a-z0-9_]*$` (bare lowercase identifier) |
| `filter_sql` | REQUIRED — non-empty after trim |
| `filter_sql` length | `<= 4096` characters |
| `filter_sql` operators | `=`, `!=`, `<`, `<=`, `>`, `>=`, `AND`, `OR`, `NOT`, `IN`, `IS NULL`, `IS NOT NULL` |
| `filter_sql` rejects | `LIKE`, `BETWEEN`, `REGEXP_LIKE`, any function call (`LOWER(x)`, `COUNT(*)`), subqueries (`SELECT`), semicolons, SQL comments (`--`, `/* */`), uppercase identifiers |

Phase 2 (MetricQL) may extend the allowlist; see ADR-026.

**More examples**:

| Use case | `valueColumn` | `filterSql` |
|----------|---------------|-------------|
| Long-session watch time | `session_duration_ms` | `session_duration_ms > 600000` |
| Mobile conversion rate | `converted` | `platform = 'mobile'` |
| US / CA engagement only | `engagement_score` | `country IN ('US', 'CA') AND engagement_score IS NOT NULL` |

### COMPOSITE

**When to use**: combine 2+ existing metrics via an arithmetic operator. Operands must be metrics that already exist in M5.

**Via the M6 UI**:
1. Navigate to `/metrics/new`.
2. Set **Metric Type** to `Composite`.
3. Fill in common fields.
4. In the COMPOSITE section:
   - **Operator** — `ADD`, `SUBTRACT`, `MULTIPLY`, `DIVIDE`, or `WEIGHTED_SUM`.
   - **Operands** — pick 2+ existing metrics from the searchable combobox. Weight inputs appear only when operator = `WEIGHTED_SUM`.
5. The form preview shows the proto-JSON payload; arity errors surface inline.
6. Click **Create Metric**.

**Via grpcurl** (WEIGHTED_SUM uses integer enum value `5`):

```bash
grpcurl -plaintext [::1]:50055 \
  experimentation.management.v1.ExperimentManagementService/CreateMetricDefinition \
  -d '{
    "metric": {
      "metricId": "engagement_composite",
      "name": "Engagement Composite (WEIGHTED_SUM)",
      "type": "METRIC_TYPE_COMPOSITE",
      "stakeholder": "METRIC_STAKEHOLDER_USER",
      "aggregationLevel": "METRIC_AGGREGATION_LEVEL_USER",
      "composite": {
        "operator": "COMPOSITE_OPERATOR_WEIGHTED_SUM",
        "operands": [
          {"metricId": "watch_time_minutes", "weight": 0.7},
          {"metricId": "metric-1",            "weight": 0.3}
        ]
      }
    }
  }'
```

**Constraints**:

| Aspect | Rule |
|--------|------|
| `operator` | must not be `COMPOSITE_OPERATOR_UNSPECIFIED` |
| Arity (`ADD` / `MULTIPLY` / `WEIGHTED_SUM`) | `>= 2` operands |
| Arity (`SUBTRACT` / `DIVIDE`) | exactly 2 operands (`operands[0] op operands[1]`) |
| `WEIGHTED_SUM` weights | every operand `weight > 0` (proto3 default `0.0` is rejected — set explicitly) |
| Operand existence | every `metric_id` must resolve via `GetMetricDefinition` before insert |
| Cycle detection | DFS over the operand graph at create time; the offending path is in the error |
| Depth cap | composites-of-composites allowed up to **5** levels; depth 6+ rejected (`DEFAULT_DEPTH_CAP = 5`) |

**More examples**:

| Use case | Operator | Operands |
|----------|----------|----------|
| Net revenue per session | `SUBTRACT` | `revenue_per_session`, `refunds_per_session` |
| Funnel completion rate | `DIVIDE` | `step_3_count`, `step_1_count` |
| Engagement score | `WEIGHTED_SUM` | `watch_time_minutes` × 0.7, `sessions_per_week` × 0.3 |

### WINDOWED_COUNT

**When to use**: count events of a specific type that occur within N hours of **each user's first exposure** to the experiment.

> **Window anchor**: the window is per-user, exposure-anchored. A user exposed at `T + 3 days` gets a window from `T + 3d` to `T + 3d + N hours`, **not** from experiment start. This is a common point of confusion.

**Via the M6 UI**:
1. Navigate to `/metrics/new`.
2. Set **Metric Type** to `Windowed Count`.
3. Fill in common fields.
4. In the WINDOWED_COUNT section:
   - **Event type** — the event to count (lowercase identifier, e.g. `signup_completed`).
   - **Window hours** — integer in `(0, 8760]`.
   - **Filter SQL** (optional) — same allowlist as FILTERED_MEAN. Leave empty for no filter.
5. The form preview shows the proto-JSON payload.
6. Click **Create Metric**.

**Via grpcurl**:

```bash
grpcurl -plaintext [::1]:50055 \
  experimentation.management.v1.ExperimentManagementService/CreateMetricDefinition \
  -d '{
    "metric": {
      "metricId": "signup_24h_count",
      "name": "Signups within 24h of exposure",
      "type": "METRIC_TYPE_WINDOWED_COUNT",
      "stakeholder": "METRIC_STAKEHOLDER_USER",
      "aggregationLevel": "METRIC_AGGREGATION_LEVEL_USER",
      "windowedCount": {
        "eventType": "signup_completed",
        "filterSql": "",
        "windowHours": 24
      }
    }
  }'
```

**Constraints**:

| Field | Rule |
|-------|------|
| `event_type` | non-empty, matches `^[a-z_][a-z0-9_]*$` |
| `window_hours` | integer in `(0, 8760]` — minimum 1 hour, maximum 1 year |
| `filter_sql` | OPTIONAL (empty = no filter); when set, same allowlist as FILTERED_MEAN |

**More examples**:

| Use case | `eventType` | `windowHours` | `filterSql` |
|----------|-------------|---------------|-------------|
| 1-week retention proxy | `content_start` | 168 | `""` |
| 1-hour purchase conversion | `purchase_completed` | 1 | `""` |
| 7-day mobile signups | `signup_completed` | 168 | `platform = 'mobile'` |

---

## Common Mistakes

- **Using FILTERED_MEAN without a filter**. Use `METRIC_TYPE_MEAN` instead — same computation, simpler shape. M5 rejects empty `filter_sql` on FILTERED_MEAN with the message `filter_sql is required for FILTERED_MEAN; use METRIC_TYPE_MEAN if no filter is needed`.
- **COMPOSITE cycles**. A composite referencing itself directly or transitively is rejected at create time with the offending path. Example: `A → B → C → A` fails with `composite cycle detected: A -> B -> C -> A`.
- **COMPOSITE depth > 5**. Composites of composites are allowed up to 5 levels deep. Beyond that, refactor into a flatter composition or reuse intermediate metric definitions. Error: `composite metric depth 6 exceeds maximum of 5`.
- **`LIKE` / `BETWEEN` / `REGEXP_LIKE` in `filter_sql`**. Not in the Phase 1 allowlist — they widen the ReDoS / cost surface in ways Phase 1 isn't ready to defend yet. Phase 2 may extend the allowlist; file a request if you need them.
- **Uppercase identifiers in `filter_sql`**. Spark identifiers are case-insensitive; M5 requires lowercase to avoid collision/shadowing. Rewrite `Platform = 'mobile'` as `platform = 'mobile'`.
- **WINDOWED_COUNT window anchor confusion**. The window is per-user, exposure-anchored, **not** experiment-start-anchored.
- **`WEIGHTED_SUM` without explicit weights**. Proto3 scalars default to `0.0`; M5 rejects `weight <= 0` for `WEIGHTED_SUM` operands. Always set every weight explicitly.
- **Mutating existing metrics**. Not supported — there is no Update/Delete RPC. Author a new metric with a new ID.

---

## Health Checks

### Quick probe (gRPC)

```bash
# Should return a list (possibly empty) — service is healthy.
grpcurl -plaintext [::1]:50055 \
  experimentation.management.v1.ExperimentManagementService/ListMetricDefinitions \
  -d '{}'
```

In dev environments with `sql/migrations/099_seed_test.sql` applied, the response includes the seed metrics `watch_time_minutes`, `metric-1`, `mobile_watch_time_filtered`, `engagement_composite`, and `signup_24h_count`.

### Read a single metric

```bash
grpcurl -plaintext [::1]:50055 \
  experimentation.management.v1.ExperimentManagementService/GetMetricDefinition \
  -d '{"metricId": "engagement_composite"}'
```

---

## Troubleshooting

| Symptom | Likely cause | Action |
|---------|--------------|--------|
| `InvalidArgument: filter_sql must not contain function calls` for `country IN('US')` (no space) | Pre-`#552` build pre-dating BUG-0002 fix | Upgrade — fixed in #552; report if seen on current build |
| `InvalidArgument: composite cycle detected: <path>` | A composite (transitively) references itself | Refactor to break the cycle; you cannot edit metrics, so author a new one with a different ID |
| `InvalidArgument: COMPOSITE metric '<id>' references operands that do not exist` | Operand metric_id was never created | Confirm via `GetMetricDefinition`; create the operand first |
| `InvalidArgument: composite metric depth N exceeds maximum of 5` | Too many nested composites | Flatten: replace deeply nested composites with intermediate metrics computed independently |
| `InvalidArgument: windowed_count.window_hours must be <= 8760 (1 year)` | Requesting more than a year window | Phase 1 caps at 8760; redesign as 12 separate monthly experiments or file a request |
| `InvalidArgument: filter_sql contains disallowed token: <token>` | Token not in the allowlist (likely `LIKE` / `BETWEEN` / uppercase identifier) | Rewrite within the allowlist or escalate to extend Phase 1 |
| `InvalidArgument: filter_sql contains unterminated string literal` | Missing closing single quote | Add the closing `'`; Phase 1 doesn't support `''` or `\'` escapes |

---

## See Also

- ADR-026: `docs/adrs/026-custom-metrics-layer.md` — architecture + Phase 2 (MetricQL) + Phase 3 (CUSTOM deprecation) roadmap.
- M5 validators: `crates/experimentation-management/src/validators/{mod,filter_sql,composite_cycle}.rs`.
- M5 store: `crates/experimentation-management/src/store.rs`.
- M3 SQL templates: `services/metrics/internal/spark/templates/{filtered_mean,composite,windowed_count}.sql.tmpl`.
- M6 UI: `ui/src/app/metrics/new/page.tsx` (form); `ui/src/components/metrics/` (per-type sections).
- Proto: `proto/experimentation/common/v1/metric.proto`, `proto/experimentation/management/v1/management_service.proto`.
- Seed data: `sql/migrations/099_seed_test.sql`.
- Related runbooks: `docs/runbooks/m4a-analysis.md`, `docs/runbooks/m4b-policy.md`.
- Issues: `#434` (M6 UI, this work), `#552` (M5 backend), `#435` / `#436` (Phase 2 MetricQL).
