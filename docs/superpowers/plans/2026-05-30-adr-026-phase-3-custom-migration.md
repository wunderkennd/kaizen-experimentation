# ADR-026 Phase 3: CUSTOM metric migration + deprecation + UI removal (#437)

**Status:** Design lock — RFC for review.
**Issue:** [#437](https://github.com/wunderkennd/kaizen-experimentation/issues/437) — P1, `sprint-5.6`, `cluster-a`, owners `agent-3` + `agent-5` + `agent-6`.
**Blocked by:** ~~Phase 1 (PR #475)~~ + ~~Phase 2 backend (PR #565)~~ + ~~Phase 2 M5+M6 (PR #570)~~ — all merged. This plan is execution-ready against `services/metrics/internal/metricql/` (Go parser/compiler) + `crates/experimentation-management/src/validators/metricql/` (Rust validator) + `ui/src/components/metrics/metricql/` (CM6 editor) all on main.

---

## Summary

Phases 1 and 2 added structured + expression-based metric definitions; this plan closes ADR-026 by **migrating existing `CUSTOM` SQL metrics into the structured / MetricQL tiers** and **gating new CUSTOM creation behind a deprecation warning**, with eventual UI removal after a 4-week sunset window.

The migration must be **trustworthy** — the existing CUSTOM corpus is operator-authored Spark SQL of arbitrary shape, and a wrong auto-translation could silently change a production metric's value (which downstream experiment-analysis tooling treats as ground truth). PR #567's earlier attempt at this work was rejected partly because its regex-based translator had a known correctness bug ("naive wrapping generates invalid MetricQL that bypasses M5's validation but crashes M3 Go scheduler compilation," per its own admission). This plan forbids regex-based translation entirely; all translation is AST-based via `sqlparser-rs`, and every proposed migration must pass a **shadow-run equivalence check** before it can be applied.

### Non-goals (v1 of #437)

- **Forced migration of un-translatable CUSTOMs.** Metrics whose SQL the translator cannot confidently rewrite stay as CUSTOM with a deprecation tail. Operators can rewrite manually to MetricQL or leave as-is; the migration tool never destructively rewrites in-place.
- **CUSTOM removal from the proto.** Even after UI removal, `MetricType::Custom` stays in `proto/experimentation/common/v1/metric.proto` for read-compatibility with existing rows. Removing it is a proto-breaking change deferred to a Phase 3.1 follow-up if/when zero CUSTOMs remain.
- **Cross-org migration assistance.** Each org migrates its own CUSTOMs at its own pace; the tool runs per-org via standard M5 RPC auth.
- **Backfill of historical metric_summaries.** Migrated metrics start fresh; existing time-series data for the original CUSTOM stays under the old metric_id (which is preserved). No reprocessing.
- **Spark SQL features beyond MetricQL's grammar.** CUSTOM SQL may use window functions, CTEs, joins to external tables, etc. Those that don't fit MetricQL's grammar are classified as Tier 3 (non-migratable) by design.

---

## Phase 3 Contract (Locks) — binding for implementers

Eight Locks freeze the cross-cutting design decisions. **Locks are normative — copy verbatim, do not drift.** PR #567 explicitly violated merged Locks; the same workflow discipline applies here. If a Lock seems wrong, BLOCK and escalate via an issue comment rather than overriding in implementation.

| # | Lock | One-line answer |
|---|---|---|
| L1 | Translator approach | **AST-based via `sqlparser-rs`** (Spark dialect); regex translation explicitly forbidden |
| L2 | Migration tool location | New Rust binary `crates/experimentation-management/src/bin/custom_migrator.rs` (lives under M5 for ownership; reuses M5's MetricQL validator for output verification) |
| L3 | Equivalence verification | **Shadow-run mandatory before apply** — M3 schedules parallel computation of the candidate translation, M3's `delta.metric_summaries` outputs compared row-by-row with configurable per-type tolerance |
| L4 | Tier classification | Tier 1 (structured: FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT) → Tier 2 (METRICQL) → Tier 3 (un-translatable; stays CUSTOM). Translator emits the *most structured* match it can confidently produce |
| L5 | Deprecation warning surface | M5 emits Connect/gRPC trailer header `x-kaizen-deprecation` on successful CUSTOM creates/updates; does NOT block. M6 surfaces as a toast + persistent banner on the metric page |
| L6 | UI rollout | Three-phase: (3.A) relabel `CUSTOM` → "Custom SQL (deprecated)" + warning icon; (3.B) after 4 weeks + zero new CUSTOMs telemetry, hide CUSTOM from create form behind feature flag `m6.metric_type.custom.hidden`; (3.C) deferred to follow-up |
| L7 | Apply mechanism | Two-step: tool generates a per-metric JSON migration proposal + diff; operator applies via new M5 RPC `MigrateMetricDefinition(old_id, new_metric)` which creates the new metric and writes a soft-link record. Never destructive in-place |
| L8 | Production survey | Plan does NOT depend on a prior survey; the tool's pattern library is conservative-by-default (only patterns that round-trip 100% of a generated corpus are auto-translated; anything else is Tier 3). A real-world survey is operational work that follows the tool, not blocks it |

---

### L1 — Translator approach: AST-based, not regex

**Decision: parse CUSTOM SQL with `sqlparser-rs` (Spark dialect), pattern-match the resulting AST against translation rules.**

Three approaches were considered:

| Option | Pros | Cons |
|---|---|---|
| (a) AST-based with `sqlparser-rs` | Round-trippable: source → AST → translation rule → verify against AST. Handles arbitrary whitespace, comments, identifier quoting. The same parser is used by 50+ open-source projects (Apache DataFusion, GlueSQL, etc.) — battle-tested. | New Rust dependency (~250KB compiled; modest workspace impact). Spark dialect coverage is good but not 100% — some Spark-isms may not parse, which classifies as Tier 3 (the correct fallback). |
| (b) Regex pattern library | No new deps; quick prototype. | **Known bug class:** PR #567's regex translator generated invalid MetricQL that bypassed M5's validator but crashed M3 compilation. Regex SQL parsing is famously brittle — any unforeseen identifier, subquery, or comment placement breaks it. **Forbidden.** |
| (c) Spark Catalyst via JNI | Most faithful: Spark's own parser. Could even use Spark's `LogicalPlan` for semantic equivalence. | Massive integration cost (JNI bridge, ~100MB Spark runtime in the M5 binary). Overkill for the translation task; we don't need full semantic plan equivalence, just shape-matching. |

**(a) wins.** `sqlparser-rs` gives us a typed AST (`sqlparser::ast::Statement`, `Query`, `Expr`, etc.) that we pattern-match against a small set of "shape templates" corresponding to FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT / METRICQL. Anything that doesn't match a template is Tier 3 (un-translatable). Crucially: **the translator is allowed — required — to fail gracefully**. Conservative emission is the goal; misclassification as Tier 3 is benign (operator-visible), incorrect translation is catastrophic (silent metric value drift).

Add to `crates/experimentation-management/Cargo.toml`:
```toml
sqlparser = { version = "0.50", features = ["serde"] }
```

---

### L2 — Migration tool location

The migration tool is a new Rust binary at `crates/experimentation-management/src/bin/custom_migrator.rs`.

Rationale:
- Lives under M5 (the metric-definition owner per ADR-025); the binary's only network deps are M5's own gRPC server (for applying migrations) and M3 (for shadow-run results).
- Reuses M5's `validators::metricql::validate_metricql()` to verify that any candidate METRICQL translation actually parses + semantically validates BEFORE proposing it. This is the round-trip check that catches PR #567's bug class.
- Cross-OS by default (the team builds Rust for Linux + macOS already); no shell-script fragility.
- Distributable as a single binary via `cargo build --release --bin custom_migrator`.

CLI shape (locked by L7's apply mechanism):

```
custom_migrator scan      --m5-addr <url> --output report.json
custom_migrator translate --report report.json --output proposals.json
custom_migrator shadow    --proposals proposals.json --m3-addr <url> --duration 1w
custom_migrator apply     --proposals proposals.json --shadow-results results.json --dry-run | --confirm
```

The four subcommands are independent (each reads/writes a file artifact); operators can pause + audit between any pair. No long-lived state in the binary itself.

---

### L3 — Equivalence verification: shadow-run is mandatory

**Decision: before any proposed migration is applied, M3 must schedule a parallel "shadow" computation of the new metric for at least 7 consecutive days, and the per-experiment, per-day, per-variant outputs in `delta.metric_summaries` must match the original CUSTOM's outputs within the per-type tolerance.**

Why mandatory: even an AST-correct translation can produce different numeric results if the original SQL had a subtle semantic the translator missed (e.g., `COALESCE` placement, NULL handling, window-frame defaults). Shadow-run catches this empirically; no static analysis can.

Per-type tolerance:
- Counts / proportions: **exact match** (integer arithmetic; any drift is a bug)
- Means / percentiles: **|new - old| / max(|old|, 1) ≤ 1e-9** (FP arithmetic; allow rounding)
- Composite / ratio: same as means
- MetricQL: same as means (Spark FP semantics)

Shadow-run mechanism (specified in Phase B):
- M3 receives a `ScheduleShadowComputation(original_metric_id, candidate_definition)` RPC
- M3 inserts a row into `metric_shadow_runs` table with status=PENDING
- The standard nightly metrics pass (Task B from #475) picks up shadow rows alongside real metrics
- After computation, M3 diffs `delta.metric_summaries` rows where `metric_id IN (original_id, shadow_id)` and writes the diff to `metric_shadow_run_results`
- The migration tool's `shadow` subcommand polls `metric_shadow_run_results` and aggregates per-proposal results

A proposal is **shadow-approved** only when 7 consecutive days of computation show within-tolerance equivalence for every (experiment, variant, day) tuple where both metrics produced output.

---

### L4 — Tier classification

The translator targets, in order of preference (most-structured first):

```
Tier 1 — Structured (preferred)
├── FILTERED_MEAN: SELECT AVG(<col>) FROM events WHERE event_type=... AND <filter>
├── COMPOSITE:    SELECT <op>(metric_a.value, metric_b.value) FROM ... JOIN ...
└── WINDOWED_COUNT: SELECT COUNT(*) FROM events WHERE event_type=... AND timestamp WITHIN N HOURS OF exposure

Tier 2 — MetricQL (when Tier 1 doesn't fit but the SQL is expressible in MetricQL's grammar)
├── Arithmetic over metric_refs:  0.7 * @watch_time + 0.3 * @ctr
├── Ratio:                         ratio(@a, @b)
└── Aggregation with filter/window: mean(field) where p='mobile' within 7 days of exposure

Tier 3 — Un-translatable (stays CUSTOM)
├── Window functions (ROW_NUMBER, LAG, etc.) — not in MetricQL grammar
├── Self-joins or joins to non-metric tables
├── CTEs with side-effects (CREATE TEMPORARY TABLE, etc.)
└── Anything the AST parser cannot parse at all
```

Pattern-matching is conservative: a SQL statement matches a Tier 1 shape ONLY if every clause maps unambiguously to a structured-type field. If two interpretations exist (e.g., `WHERE x = 'mobile'` could be a FILTERED_MEAN filter OR a METRICQL `where x = 'mobile'`), prefer the more-structured Tier 1.

The translator emits **at most one proposal per CUSTOM** — never "this could be FILTERED_MEAN or METRICQL; pick one." The operator-visible report shows the chosen tier with a one-sentence justification.

---

### L5 — Deprecation warning surface

**Decision: M5 emits a Connect/gRPC trailer header `x-kaizen-deprecation` on successful Create/Update of CUSTOM-typed metrics; the response itself is not modified.**

Trailer (vs leading header or response field) chosen because:
- Trailers are part of the standard gRPC response protocol; existing M6 connect-web client already surfaces them via `useTransport`'s `onError`/`onResponse` hooks
- Adding a new field to `MetricDefinition` for "is_deprecated" would be a proto-breaking-ish change and is the wrong shape (the deprecation is per-call, not per-row)
- Connect/Go and Connect/Web both natively support trailers

Header value (UTF-8 string): structured message with three parts:
```
x-kaizen-deprecation: kind=metric_type; type=CUSTOM; message=CUSTOM metrics are deprecated in favor of MetricQL or structured types. See docs/runbooks/m5-metric-definitions.md#custom-deprecation for the migration guide.
```

M5 emits the trailer whenever the request's `MetricDefinition.type == METRIC_TYPE_CUSTOM`, on both Create and Update success paths.

M5 also emits a telemetry counter `m5.metric_definition.custom.created` per CUSTOM creation, for tracking the deprecation curve.

M6 receives the trailer in the connect-web response and:
- Shows a non-dismissible toast: "Custom SQL metrics are deprecated. Use MetricQL or structured types instead. [Migration guide →]"
- Shows a persistent banner on the metric detail page until the user explicitly dismisses it (per-user, localStorage)

---

### L6 — UI rollout

Three-phase rollout, gated by a feature flag (`m6.metric_type.custom.hidden`):

**Phase 3.A (immediate, on Phase 3 ship):**
- `ui/src/components/metrics/metric-type-select.tsx`: relabel the CUSTOM option:
  - Current: `label: 'Custom SQL', description: 'Arbitrary Spark SQL (advanced — prefer structured types).'`
  - New: `label: 'Custom SQL (deprecated)', description: 'Deprecated — use MetricQL or structured types. Existing CUSTOMs continue to work.'`
- Add a warning icon next to the option (lucide-react `AlertTriangle`, amber-500)
- The CUSTOM option remains selectable; clicking it shows the deprecation banner in the editor pane

**Phase 3.B (after 4 weeks + telemetry shows ≤1 new CUSTOM/week for 2 consecutive weeks):**
- Operator (or scheduled job) sets `m6.metric_type.custom.hidden = true` for the org
- CUSTOM option is filtered out of `TYPE_OPTIONS` in `metric-type-select.tsx`
- Existing CUSTOM metrics remain viewable/editable on the metric detail page (the type selector on edit is read-only-locked-to-CUSTOM, so the operator can't escape it)

**Phase 3.C (deferred — out of #437 scope):**
- After 2 release cycles with zero new CUSTOM metrics globally: remove `MetricType::Custom` from the proto enum (breaking change). Filed as `#437.1` follow-up.

The feature flag lives in M6's existing flag system (`ui/src/lib/flags.ts`). Default: `false`.

---

### L7 — Apply mechanism: two-step, never destructive

**Decision: the migration tool produces a JSON proposal artifact; operator review + explicit confirmation triggers a new M5 RPC `MigrateMetricDefinition(old_metric_id, new_metric)` which (a) creates the new metric with a new metric_id, (b) writes a soft-link row in `metric_migrations` table pointing old_id → new_id.**

Why two-step:
- No accidental in-place rewrites; the old CUSTOM stays exactly as it was
- Downstream consumers (experiments referencing the old metric_id, dashboards, OPA policies) continue to work — they can opt into the new metric at their own pace
- Audit trail: every migration is a discrete event with a timestamp + operator identity + before/after JSON

New M5 RPC (added to `management_service.proto`):
```proto
service ExperimentManagementService {
  // Migrate a CUSTOM metric to a structured or MetricQL equivalent.
  // Creates a new MetricDefinition and writes a soft-link record. The
  // original CUSTOM metric is unchanged; downstream consumers continue
  // referring to the old metric_id until they explicitly migrate.
  rpc MigrateMetricDefinition(MigrateMetricDefinitionRequest)
      returns (MigrateMetricDefinitionResponse);
}

message MigrateMetricDefinitionRequest {
  string original_metric_id = 1;
  common.v1.MetricDefinition migrated_metric = 2;  // must have a new metric_id
  string shadow_run_result_id = 3;  // M3 reference; rejected if not APPROVED
}

message MigrateMetricDefinitionResponse {
  common.v1.MetricDefinition created = 1;
  string migration_id = 2;  // primary key of the metric_migrations row
}
```

New table (migration `014_adr026_phase3_metric_migrations.sql`):
```sql
CREATE TABLE metric_migrations (
  migration_id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  original_metric_id    TEXT NOT NULL REFERENCES metric_definitions(metric_id),
  migrated_metric_id    TEXT NOT NULL REFERENCES metric_definitions(metric_id),
  shadow_run_result_id  UUID NOT NULL,
  created_by            TEXT NOT NULL,
  created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT metric_migrations_no_self_migration CHECK (original_metric_id != migrated_metric_id)
);
CREATE INDEX idx_metric_migrations_original ON metric_migrations(original_metric_id);
CREATE INDEX idx_metric_migrations_migrated ON metric_migrations(migrated_metric_id);
```

The handler enforces:
- `original_metric_id` exists AND is `MetricType::Custom` (reject otherwise)
- `migrated_metric.metric_id != original_metric_id` (no in-place)
- `migrated_metric.type ∈ {FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT, METRICQL}` (no CUSTOM-to-CUSTOM)
- `shadow_run_result_id` resolves to an `APPROVED` row in `metric_shadow_run_results` (M3 RPC lookup)
- All standard CreateMetricDefinition validation runs on `migrated_metric` (including #571's live-lint pipeline if METRICQL)

---

### L8 — No production survey blocking the tool

The migration tool's pattern library is **conservative by default**: it only auto-translates SQL shapes that round-trip 100% against a synthetic test corpus (built from Phase 1's existing FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT examples, plus hand-written CUSTOM-to-MetricQL pairs).

A real-world survey (Phase E, operational) follows the tool's ship — it informs which patterns are worth adding next, but the v1 ship doesn't gate on it.

Why this matters: PR #567 attempted to ship a translator with no survey, which produced over-confident translation of patterns the author hadn't seen in production. The fix isn't "do the survey first" — it's "make the translator conservative so misses are visible as Tier 3 rather than hidden as wrong translations." The survey then drives the pattern-library grows.

---

## Phase A — Migration tool (Rust binary)

Files: `crates/experimentation-management/src/bin/custom_migrator.rs` (new), `crates/experimentation-management/src/migration/{mod,classifier,tier1,tier2,report}.rs` (new module), `crates/experimentation-management/Cargo.toml` (+`sqlparser`), `test-vectors/custom_migration_corpus.json` (new shared corpus).

### Task A1: Scaffold the migrator crate + module

- [ ] **Step 1:** Add `sqlparser = { version = "0.50", features = ["serde"] }` to `crates/experimentation-management/Cargo.toml`. Run `cargo check -p experimentation-management` to confirm no version-resolution conflicts.
- [ ] **Step 2:** Create `crates/experimentation-management/src/migration/mod.rs` with the public entry surface:

```rust
pub mod classifier;
pub mod tier1;
pub mod tier2;
pub mod report;

/// Classify a CUSTOM metric and (if possible) produce a translation proposal.
/// Returns `Tier::Untranslatable` for anything that doesn't fit a known shape;
/// the operator-visible report distinguishes "did not parse" from "parsed but
/// no matching pattern" from "matched but failed M5 validate_metricql round-trip."
pub fn classify_and_translate(
    custom_sql: &str,
    original: &MetricDefinition,
) -> ClassificationResult;

pub enum ClassificationResult {
    Tier1Filtered { proposal: MetricDefinition, reason: String },
    Tier1Composite { proposal: MetricDefinition, reason: String },
    Tier1WindowedCount { proposal: MetricDefinition, reason: String },
    Tier2Metricql { proposal: MetricDefinition, reason: String },
    Tier3Untranslatable { reason: String, parse_error: Option<String> },
}
```

- [ ] **Step 3:** RED — `migration/mod_test.rs` with a smoke test: invoke `classify_and_translate("", &dummy_custom_metric())`; assert it returns `Tier3` with reason "empty SQL."
- [ ] **Step 4:** GREEN — minimal impl that always returns `Tier3 { reason: "no patterns implemented yet" }`.
- [ ] **Step 5:** Commit. `feat(management): scaffold CUSTOM migration module (#437)`

### Task A2: Shared corpus + parity test

- [ ] **Step 1:** Author `test-vectors/custom_migration_corpus.json` with ~30 fixtures:
  - 10 Tier 1 cases (5 FILTERED_MEAN, 3 COMPOSITE, 2 WINDOWED_COUNT) — derived from Phase 1's existing structured-type examples, written as CUSTOM SQL
  - 10 Tier 2 cases (METRICQL expressions written as CUSTOM SQL — composites with arithmetic, ratios, percentile filters)
  - 10 Tier 3 cases (window functions, multi-table joins, recursive CTEs, malformed SQL, comments-only, etc.)

Each entry: `{ name, custom_sql, expected_tier, expected_proposal: <full MetricDefinition JSON> | null, expected_reason: <substring> }`.

- [ ] **Step 2:** RED — corpus test that iterates fixtures and asserts `classify_and_translate(fixture.custom_sql, ...).tier == fixture.expected_tier`. Initially all fixtures fail (no patterns implemented).
- [ ] **Step 3:** Commit. `test(management): CUSTOM migration corpus + parity test (#437)`

### Task A3: AST classifier

- [ ] **Step 1:** RED — `migration/classifier_test.rs` cases:
  - `parse_or_tier3("SELECT 1")` → returns parsed `Statement::Query`
  - `parse_or_tier3("SELECT * FRO bad")` → returns `Tier3 { reason: "SQL parse failed: ...", parse_error: Some(...) }`
  - `parse_or_tier3("SELECT COUNT(*) FROM events WHERE event_type = 'foo'")` → returns parsed, with shape-classifier returning `ShapeHint::FilteredAggregation`
- [ ] **Step 2:** GREEN — implement `parse_or_tier3` using `sqlparser::Parser::parse_sql(&SparkDialect{}, custom_sql)`. Extract shape hints from the top-level `Statement::Query`'s `SetExpr::Select` — projection items, FROM clause, WHERE clause, GROUP BY.
- [ ] **Step 3:** Commit. `feat(management): SQL AST parser + shape classifier (#437)`

### Task A4: Tier 1 translator

- [ ] **Step 1:** RED — corpus fixtures for FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT shapes (10 from A2).
- [ ] **Step 2:** GREEN — `migration/tier1.rs` matches `ShapeHint::FilteredAggregation` against:
  - **FILTERED_MEAN**: projection = `AVG(<col>)` OR `SUM(<col>)/COUNT(*)`; FROM = `events`; WHERE has `event_type = '<lit>'` and optionally `AND <filter>`; no GROUP BY beyond `user_id, variant_id`
  - **WINDOWED_COUNT**: projection = `COUNT(*)`; FROM = `events`; WHERE includes `event_type = ...` AND a window predicate (`event_ts BETWEEN ... AND ...` or `event_ts - exposure_ts < INTERVAL N HOURS`)
  - **COMPOSITE**: projection = arithmetic expression over `(SELECT ... FROM metric_summaries ...)` subqueries — pattern-match against the Phase 1 composite.sql.tmpl shape

For each, build the corresponding `MetricDefinition` proto with the right `type_config` oneof variant.

- [ ] **Step 3:** Verify each translation round-trips: feed the synthesized `MetricDefinition` through M5's existing `validators::validate_metric_definition` (the validator added in #436); reject the translation if validation fails. This catches PR #567's bug class — "the translator produced output that the validator wouldn't accept."
- [ ] **Step 4:** Commit. `feat(management): Tier 1 CUSTOM-to-structured translator (#437)`

### Task A5: Tier 2 translator (METRICQL)

- [ ] **Step 1:** RED — METRICQL corpus fixtures (10 from A2).
- [ ] **Step 2:** GREEN — `migration/tier2.rs` matches any shape that the Tier 1 patterns reject but that maps to MetricQL's grammar:
  - Arithmetic over metric_summary subqueries → `0.7 * @metric_a + 0.3 * @metric_b`
  - `SUM(numerator) / SUM(denominator)` over a single events scan → `ratio(@n, @d)` if numerator/denominator can be expressed as standalone metrics; otherwise stays as a raw aggregate
  - Aggregation with filter that includes more than the FILTERED_MEAN allowlist → `mean(<field>) where <predicate> [within N hours of exposure]`
- [ ] **Step 3:** Round-trip check: the generated METRICQL string must pass `metricql::validate_metricql(&proposal.metricql_expression, &ctx)` (the Rust validator added in #436). Reject if it doesn't.
- [ ] **Step 4:** Commit. `feat(management): Tier 2 CUSTOM-to-MetricQL translator (#437)`

### Task A6: Report generator

- [ ] **Step 1:** `migration/report.rs` — given a `Vec<ClassificationResult>` keyed by `original_metric_id`, generate:
  - JSON report (machine-readable; consumed by `apply` subcommand and the M6 dashboard if/when added)
  - Markdown summary (human-readable; printed by the `translate` subcommand)
- [ ] **Step 2:** RED — golden-file test: feed a 5-fixture batch through; assert the output JSON matches `testdata/report.golden.json` and Markdown matches `testdata/report.golden.md`.
- [ ] **Step 3:** Commit. `feat(management): migration report generator (#437)`

### Task A7: CLI binary + subcommands

- [ ] **Step 1:** `crates/experimentation-management/src/bin/custom_migrator.rs` — `clap`-based CLI with four subcommands per L2.
- [ ] **Step 2:** `scan` subcommand: calls `ListMetricDefinitions(type_filter=CUSTOM)` on M5, dumps results to JSON.
- [ ] **Step 3:** `translate` subcommand: reads scan output, runs `classify_and_translate` per metric, writes proposals JSON + Markdown summary.
- [ ] **Step 4:** `shadow` subcommand: for each Tier 1/Tier 2 proposal, calls `M3::ScheduleShadowComputation(original_id, proposal.migrated_metric)`; polls `M3::GetShadowResults` until all proposals have at least 7 days of results.
- [ ] **Step 5:** `apply` subcommand: reads proposals + shadow results, filters to APPROVED, calls `M5::MigrateMetricDefinition` per metric. `--dry-run` prints what would be applied; `--confirm` actually applies.
- [ ] **Step 6:** Commit. `feat(management): custom_migrator CLI binary (#437)`

---

## Phase B — Shadow-run pipeline (Go, M3)

Files: `proto/experimentation/metrics/v1/metrics_service.proto` (+`ScheduleShadowComputation`, `GetShadowResults`), `services/metrics/internal/shadow/{job,differ,storage}.go` (new), `sql/migrations/015_adr026_phase3_metric_shadow_runs.sql` (new).

### Task B1: Shadow-run scheduling RPC + DB table

- [ ] **Step 1:** Migration 015 — `metric_shadow_runs` table:
  ```sql
  CREATE TABLE metric_shadow_runs (
    shadow_id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    original_metric_id  TEXT NOT NULL,
    candidate_metric    JSONB NOT NULL,  -- full MetricDefinition
    scheduled_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status              TEXT NOT NULL CHECK (status IN ('PENDING','RUNNING','APPROVED','REJECTED','FAILED')),
    rejection_reason    TEXT
  );
  CREATE TABLE metric_shadow_run_results (
    result_id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    shadow_id           UUID NOT NULL REFERENCES metric_shadow_runs(shadow_id),
    experiment_id       TEXT NOT NULL,
    variant_id          TEXT NOT NULL,
    computation_date    DATE NOT NULL,
    original_value      DOUBLE PRECISION,
    candidate_value     DOUBLE PRECISION,
    diff_abs            DOUBLE PRECISION,
    diff_rel            DOUBLE PRECISION,
    within_tolerance    BOOLEAN NOT NULL
  );
  ```
- [ ] **Step 2:** Proto additions:
  ```proto
  rpc ScheduleShadowComputation(ScheduleShadowComputationRequest) returns (ScheduleShadowComputationResponse);
  rpc GetShadowResults(GetShadowResultsRequest) returns (GetShadowResultsResponse);
  rpc PromoteShadowResult(PromoteShadowResultRequest) returns (PromoteShadowResultResponse);
  ```
- [ ] **Step 3:** RED — handler tests for `ScheduleShadowComputation` (PENDING row inserted), `GetShadowResults` (returns rows filtered by shadow_id), `PromoteShadowResult` (transitions PENDING→APPROVED only when all days within tolerance, returns the result_id used by M5's MigrateMetricDefinition check).
- [ ] **Step 4:** GREEN — handlers.
- [ ] **Step 5:** Commit. `feat(metrics): shadow-run scheduling + storage (#437)`

### Task B2: Shadow-run computation in the nightly metrics pass

- [ ] **Step 1:** Extend `services/metrics/internal/jobs/standard.go`'s `Run` loop to pick up PENDING `metric_shadow_runs` alongside regular metrics. Compute the candidate metric using the same `metricql::Compile` (for METRICQL candidates) or the structured-type compute path (for Tier 1 candidates).
- [ ] **Step 2:** After computation, transition shadow row status PENDING→RUNNING→COMPLETED.
- [ ] **Step 3:** RED — integration test: insert a PENDING shadow row with a known equivalent candidate; run the pass; assert the row transitions correctly and `delta.metric_summaries` contains output under the shadow_id namespace (distinct from the original metric_id's rows).
- [ ] **Step 4:** Commit. `feat(metrics): shadow-run computation in StandardJob.Run (#437)`

### Task B3: Differ + tolerance check

- [ ] **Step 1:** `services/metrics/internal/shadow/differ.go` — for each (experiment, variant, computation_date) tuple where both original and shadow produced output, compute `diff_abs = |orig - cand|`, `diff_rel = diff_abs / max(|orig|, 1)`, and `within_tolerance = (metric_type == COUNT/PROPORTION ? diff_abs == 0 : diff_rel <= 1e-9)`. Persist to `metric_shadow_run_results`.
- [ ] **Step 2:** RED — differ tests:
  - Identical values → `within_tolerance = true`
  - Floating-point drift within tolerance → `within_tolerance = true`
  - Count drift by 1 → `within_tolerance = false`
  - One side missing → row written with NULL on the missing side, `within_tolerance = false`
- [ ] **Step 3:** Daily aggregation: a metric is `shadow_approved` only when **7 consecutive days, all (experiment, variant, day) tuples within tolerance**. `PromoteShadowResult` enforces this.
- [ ] **Step 4:** Commit. `feat(metrics): shadow-run differ + tolerance check (#437)`

---

## Phase C — Deprecation hooks (Rust M5 + Go variant)

Files: `crates/experimentation-management/src/grpc.rs` (deprecation trailer + telemetry on Create/Update Custom), `services/management/internal/handlers/metric.go` (same in Go variant), `crates/experimentation-management/src/telemetry.rs` (extend the counter set), `docs/runbooks/m5-metric-definitions.md` (extend with the migration runbook).

### Task C1: M5 Rust — deprecation trailer + telemetry

- [ ] **Step 1:** In `CreateMetricDefinition` handler, after successful insert, check `if request.metric.r#type() == MetricType::Custom`. If so:
  ```rust
  let mut response = Response::new(created);
  response.metadata_mut().insert(
      "x-kaizen-deprecation",
      MetadataValue::from_static(DEPRECATION_HEADER_CUSTOM),
  );
  metrics::counter!("m5.metric_definition.custom.created", 1);
  ```
  where `DEPRECATION_HEADER_CUSTOM` is the L5 fixed-string constant.
- [ ] **Step 2:** Same in `UpdateMetricDefinition` if the updated metric is CUSTOM (whether it was previously or not — captures "changed to CUSTOM").
- [ ] **Step 3:** RED — handler tests: create CUSTOM → response has `x-kaizen-deprecation` trailer; create FILTERED_MEAN → no trailer; counter incremented per CUSTOM create.
- [ ] **Step 4:** Commit. `feat(management): emit deprecation trailer for CUSTOM creates (#437)`

### Task C2: Go variant M5 — symmetric deprecation

- [ ] **Step 1:** In `services/management/internal/handlers/metric.go::CreateMetricDefinition`, mirror the Rust trailer + counter using ConnectRPC's `connect.Response.Trailer()` and a Prometheus counter. Same `x-kaizen-deprecation` value.
- [ ] **Step 2:** RED — handler test (mirrors C1).
- [ ] **Step 3:** Commit. `feat(management): Go variant deprecation trailer parity (#437)`

### Task C3: M5 MigrateMetricDefinition RPC

- [ ] **Step 1:** Proto + handler per L7. Migration 014 already added the `metric_migrations` table (Phase A's L7 spec).
- [ ] **Step 2:** Handler enforces L7's preconditions in this order: (a) `original_metric_id` exists & is CUSTOM; (b) `migrated_metric.type != CUSTOM`; (c) `migrated_metric.metric_id != original_metric_id`; (d) `shadow_run_result_id` resolves to APPROVED in M3; (e) `validate_metric_definition(migrated_metric)` passes.
- [ ] **Step 3:** Single atomic transaction: `INSERT INTO metric_definitions ... INSERT INTO metric_migrations ...`. On failure, both roll back.
- [ ] **Step 4:** RED — integration tests:
  - Happy path: CUSTOM original + valid METRICQL migrated + APPROVED shadow → both rows created
  - Original isn't CUSTOM → InvalidArgument
  - Migrated is CUSTOM → InvalidArgument
  - Same metric_id → InvalidArgument
  - Shadow not APPROVED → FailedPrecondition
  - Migrated fails validate_metric_definition (e.g., bad MetricQL) → InvalidArgument with the validation diagnostics
  - DB unique-constraint collision on migrated_metric_id → AlreadyExists
- [ ] **Step 5:** Commit. `feat(management): MigrateMetricDefinition RPC + migration 014 (#437)`

### Task C4: Go variant stub for MigrateMetricDefinition

- [ ] **Step 1:** Same pattern as `metricql_stubs.go` shipped in PR #570: stub method returning `Unimplemented` with an ADR-025 pointer.
- [ ] **Step 2:** RED — vet test: `*ExperimentService` satisfies the regenerated handler interface; calling the Go endpoint returns Unimplemented.
- [ ] **Step 3:** Commit. `feat(management): Go variant MigrateMetricDefinition stub (#437)`

---

## Phase D — UI rollout

Files: `ui/src/components/metrics/metric-type-select.tsx`, `ui/src/lib/flags.ts` (extend if needed), `ui/src/components/metrics/metric-form-shell.tsx`, `ui/src/lib/api.ts` (surface the deprecation trailer to UI consumers).

### Task D1: Relabel CUSTOM as deprecated (Phase 3.A)

- [ ] **Step 1:** Update `metric-type-select.tsx`'s `TYPE_OPTIONS`:
  - CUSTOM label → `'Custom SQL (deprecated)'`
  - CUSTOM description → `'Deprecated — use MetricQL or structured types. Existing CUSTOMs continue to work. See migration guide.'`
- [ ] **Step 2:** Add warning icon (`AlertTriangle` from `lucide-react`, color `text-amber-500`) next to the CUSTOM option label.
- [ ] **Step 3:** RED — Vitest:
  - Render select; CUSTOM option text contains "deprecated"
  - Warning icon present in the CUSTOM option
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit. `feat(ui): label CUSTOM metric type as deprecated (#437)`

### Task D2: Surface the deprecation trailer

- [ ] **Step 1:** Extend the Connect-Web interceptor in `ui/src/lib/api.ts` to read the `x-kaizen-deprecation` trailer from successful responses. Emit a toast via the existing `useToast` hook + persist a per-user "deprecation seen" record in localStorage.
- [ ] **Step 2:** On the metric detail page, show a persistent banner if the metric is CUSTOM (banner is per-metric, not per-session — every time you view a CUSTOM metric, the banner is there until dismissed for that metric).
- [ ] **Step 3:** RED — Vitest:
  - Mock Create CUSTOM → toast appears
  - Mock Create MEAN → no toast
  - Banner appears on CUSTOM metric detail
  - Banner dismiss persists to localStorage; reload → banner stays dismissed
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit. `feat(ui): surface CUSTOM deprecation trailer as toast + banner (#437)`

### Task D3: Feature flag for hiding CUSTOM (Phase 3.B)

- [ ] **Step 1:** Add flag `m6.metric_type.custom.hidden` (default `false`) to `ui/src/lib/flags.ts`.
- [ ] **Step 2:** In `metric-type-select.tsx`, filter the `TYPE_OPTIONS` based on the flag. When hidden, existing CUSTOM metrics still display correctly on edit (the type cell is read-only when editing).
- [ ] **Step 3:** RED — Vitest:
  - Flag off → CUSTOM appears
  - Flag on → CUSTOM does not appear in new-metric form
  - Editing an existing CUSTOM with flag on → form shows CUSTOM as a read-only type
- [ ] **Step 4:** GREEN.
- [ ] **Step 5:** Commit. `feat(ui): feature flag for hiding deprecated CUSTOM metric type (#437)`

---

## Phase E — Production survey + initial migration (operational, post-ship)

Files: scripts under `scripts/migration-phase3/` (new directory) — not in the main code path.

### Task E1: Production survey script

- [ ] **Step 1:** `scripts/migration-phase3/survey.sh` — calls `custom_migrator scan`, dumps CUSTOMs by org. Output is org-keyed JSON.
- [ ] **Step 2:** Document in `docs/runbooks/adr-026-phase-3-migration.md`: how to run the survey, who to send the report to, expected output shape.

### Task E2: Per-org migration proposals

- [ ] **Step 1:** Run `custom_migrator translate` against survey output. Output is per-org proposal JSON + Markdown summary.
- [ ] **Step 2:** Distribute to org owners with the runbook.

### Task E3: Shadow-run period

- [ ] **Step 1:** For orgs that accept proposals: `custom_migrator shadow --duration 7d` queues shadow runs.
- [ ] **Step 2:** M3 runs the shadows alongside the nightly metrics pass for 7 days.

### Task E4: Apply approved migrations

- [ ] **Step 1:** Operator runs `custom_migrator apply --dry-run` to preview; reviews; runs `apply --confirm` to commit.
- [ ] **Step 2:** Audit log: each `apply` produces a `metric_migrations` row + telemetry event.

---

## Phase F — Convergence

### Task F1: Acceptance-criteria mapping

| #437 AC | Test/file location |
|---|---|
| Migration tool: scan + classify + report | A2 corpus parity test + A7 CLI integration test |
| Shadow-run with row-level equivalence | B2 + B3 integration tests |
| M5 `CreateMetric` for CUSTOM emits deprecation warning | C1 handler test |
| M5 `UpdateMetric` warns on change-to-CUSTOM | C1 handler test (second case) |
| M6 CUSTOM labeled "Deprecated" | D1 Vitest |
| Migration guide doc | `docs/runbooks/adr-026-phase-3-migration.md` (Phase E task) |
| Hide CUSTOM from form after sunset (4 weeks) | D3 Vitest with flag toggle |

### Task F2: Full-suite regression

```
cd crates && cargo test --workspace
cd services && go test ./...
cd ui && npm test
just test-metricql-parity   # parity gates from #436 stay green
just test-custom-migration-parity   # new gate for the migration corpus
```

### Task F3: Final commit + PR

`feat(metrics): ADR-026 Phase 3 — CUSTOM migration + deprecation + UI removal (#437)` — push, open PR with `Closes #437`, link the corpus, include the AC mapping table.

---

## Test plan summary

| Phase | Test files | Count target |
|---|---|---|
| A (Rust migrator) | `migration/{mod,classifier,tier1,tier2,report}_test.rs`, `bin/custom_migrator_test.rs` | ~25 unit + 5 integration |
| A2 corpus parity | `migration/custom_corpus_parity.rs` | 30+ corpus fixtures |
| B (Shadow-run) | `services/metrics/internal/shadow/{job,differ}_test.go`, `services/metrics/internal/grpc/shadow_handler_test.go` | ~15 unit + 3 integration |
| C (Deprecation) | `grpc.rs` deprecation tests + Go handler tests | ~6 unit |
| D (UI) | `metric-type-select.test.tsx`, `api.test.ts`, deprecation-toast tests | ~10 unit + 1 Playwright |

---

## Risks + rollback

| Risk | Severity | Mitigation |
|---|---|---|
| Translator produces semantically-wrong MetricQL that bypasses M5's validator (PR #567's known bug class) | **Critical** | L1 mandates AST-based translation; Tasks A4/A5 round-trip every proposal through `validate_metric_definition` before emission; L3 shadow-run catches what static analysis misses |
| Shadow-run shows within-tolerance equivalence but real-world value drifts at edge cases the test corpus didn't anticipate | Medium | L3 mandates 7 consecutive days minimum; aggregation requires ALL (experiment, variant, day) tuples to be within tolerance — a single mismatch fails the whole proposal |
| Deprecation trailer ignored by some client (e.g., old SDK) | Low | M5 also logs the deprecation to telemetry counter; an SDK that drops the trailer still appears in `m5.metric_definition.custom.created` counts |
| Operator clicks `apply --confirm` with stale shadow results | Medium | L7's handler check: `shadow_run_result_id` must resolve to APPROVED at the time of apply; M3 invalidates result_ids when the underlying metric's SQL changes |
| Migration creates orphan rows in `metric_migrations` if M5 crashes mid-transaction | Low | Task C3 mandates single atomic transaction; if M5 crashes mid-transaction, PG rolls back both inserts. No partial state. |
| UI feature flag misconfiguration hides CUSTOM globally before sunset window completes | Medium | Flag default is `false`; per-org override required; documentation calls out the telemetry criteria for flipping (4 weeks + ≤1/wk for 2 weeks) |

**Rollback for any phase:**
- Phase A (migrator): the tool produces artifacts only; no rollback needed (nothing applied)
- Phase B (shadow-run): shadow rows can be deleted from `metric_shadow_runs`; no impact on real metrics
- Phase C (deprecation trailer): remove the trailer insertion code; clients that read trailers ignore absence gracefully
- Phase D (UI relabel): revert the label change
- Phase E (applied migration): the old CUSTOM metric is never deleted; rolling back means setting `metric_migrations.status = 'ROLLED_BACK'` and the operator updates downstream consumers to reference the original metric_id again

---

## Follow-ups

| Item | Trigger | Owner |
|---|---|---|
| **#437.1** — Remove `MetricType::Custom` from the proto enum (breaking change) | After zero new CUSTOMs for 2 release cycles + zero CUSTOM rows in `metric_definitions` for 1 release cycle | agent-3 + agent-5 + agent-6 + all SDK owners |
| **#437.2** — Add patterns to the translator's Tier-1/Tier-2 library based on Phase E survey results | Within 2 weeks of Phase E completion; iterative until coverage ≥ 80% of surveyed CUSTOMs | agent-3 |
| **#437.3** — Add a M6 dashboard for the migration-progress view (% of CUSTOMs migrated per org) | After Phase E has data; only if requested | agent-6 |

---

## Branch + PR conventions

- Branches: `agent-3/feat/adr-026-phase-3-migrator` (Phase A); `agent-3/feat/adr-026-phase-3-shadow` (Phase B); `agent-5/feat/adr-026-phase-3-deprecation` (Phase C); `agent-6/feat/adr-026-phase-3-ui-deprecation` (Phase D). Or one integration branch `agent-3/feat/adr-026-phase-3` per the locked plan's own naming if Multiclaude prefers single-PR execution.
- Total commits: ~24 (one per Task across A1–A7, B1–B3, C1–C4, D1–D3, F1–F3).
- PR `Closes #437` once Phases A–D + F all pass.
- Conventional commits: `feat(management):`, `feat(metrics):`, `feat(ui):`, `test(...)`, `refactor(...)`, `docs:`.

Estimated execution time at hybrid throughput: ~14–18 agent-hours (slightly larger than #436's ~12–16h due to the cross-module shadow-run pipeline and the AST translator's pattern library).
