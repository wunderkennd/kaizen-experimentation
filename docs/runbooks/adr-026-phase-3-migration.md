# ADR-026 Phase 3 — CUSTOM Metric Migration Runbook

## What this runbook covers

ADR-026 Phase 3 retires legacy `METRIC_TYPE_CUSTOM` metrics in favour of the
Phase 1 structured types (`FILTERED_MEAN`, `COMPOSITE`, `WINDOWED_COUNT`) and
the Phase 2 MetricQL expression language. This runbook is for operators
driving that migration against a real org's metrics: scan the inventory,
translate eligible CUSTOMs, shadow-run the candidates against M3 until
row-level equivalence is established, and apply the migrations atomically
through M5. Survey is operational guidance, not a prerequisite (Lock L8) —
you may run survey + translate ad-hoc; the migration tool's pattern library
is conservative-by-default, so anything that cannot be auto-translated is
surfaced as a Tier 3 entry for human review.

## Workflow at a glance

```
scan  →  translate  →  (distribute proposals to owners)  →  shadow (7+ days)
                                                              ↓
                                                       apply --dry-run
                                                              ↓
                                                       apply --confirm
```

| Step | Input | Output | Guarantees |
|------|-------|--------|------------|
| `scan` | M5 gRPC URL | `scan.json` (raw `MetricDefinition`s, `type = CUSTOM`) | Read-only; pages through `ListMetricDefinitions` until exhaustion |
| `translate` | `scan.json` | `proposals.json` + `summary.md` | AST-based; only patterns that round-trip 100% are auto-classified Tier 1/2 |
| `shadow` | `proposals.json`, M3 gRPC URL | `shadow.json` | M3 enforces 7 consecutive days within tolerance before approving |
| `apply --dry-run` | `shadow.json` | stdout table | No M5 writes |
| `apply --confirm` | `shadow.json` | M5 mutations + audit sidecar | Atomic txn per migration; old CUSTOM row preserved |

---

## Prerequisites

| Requirement | Why |
|-------------|-----|
| M5 (`experimentation-management`) reachable at a known gRPC URL | `scan` and `apply` both call M5 RPCs (`ListMetricDefinitions`, `MigrateMetricDefinition`) |
| M3 (metrics service) reachable at a known gRPC URL | `shadow` schedules + polls via `ScheduleShadowComputation`, `PromoteShadowResult`, `GetShadowResults` |
| `custom_migrator` binary built or available via `cargo run` | All four subcommands live in this binary; build with `cargo build --release -p experimentation-management --bin custom_migrator` or set `$CUSTOM_MIGRATOR_BIN` to a prebuilt path |
| Operator identity (email or service-account id) | Required for `apply --confirm --operator <id>`; written into the `metric_migrations.operator` audit column |
| Read access to the relevant org's metric definitions | Whoever the M5 caller authenticates as needs to be able to list / read the CUSTOM rows being migrated |
| `jq` (recommended, optional) | The `survey.sh` wrapper uses `jq` for its closing summary; otherwise counts display as `?` |

ADR-026 Phase 3 ships migration `016_adr026_phase3_metric_migrations.sql`
(the `metric_migrations` audit table). Ensure migrations 001–016 are
applied against the M5 PostgreSQL database before running `apply --confirm`.

---

## Step 1 — Scan + translate (the survey)

### What it does

`custom_migrator scan` calls `M5::ListMetricDefinitions(type = CUSTOM)` and
serialises every returned `MetricDefinition` into a JSON array.

`custom_migrator translate` reads that array and, per metric, runs the AST
classifier. Each metric resolves to one of five outcomes (each is a tier of
the classification enum in
`crates/experimentation-management/src/migration/classifier.rs`):

| Tier | Meaning | Apply-eligible? |
|------|---------|-----------------|
| `tier1_filtered_mean` | Auto-translatable to `FILTERED_MEAN` | Yes |
| `tier1_composite` | Auto-translatable to `COMPOSITE` | Yes |
| `tier1_windowed_count` | Auto-translatable to `WINDOWED_COUNT` | Yes |
| `tier2_metricql` | Auto-translatable to a MetricQL expression | Yes |
| `tier3_untranslatable` | Not auto-translatable; needs human design | No |

The translator round-trips every proposal through
`validate_metric_definition` before emission (Lock L1) so the classifier
never produces semantically-invalid output.

### How to run

**Quick path (recommended)** — uses the `survey.sh` wrapper, which calls
both subcommands and prints a one-line tier breakdown at the end:

```bash
./scripts/migration-phase3/survey.sh \
    --m5-addr http://localhost:50055 \
    --output-dir ./migration-phase3-output \
    --org acme
```

The wrapper writes:

- `./migration-phase3-output/acme-scan.json` — raw `MetricDefinition` list
- `./migration-phase3-output/acme-proposals.json` — proposals (input for `shadow`)
- `./migration-phase3-output/acme-summary.md` — Markdown summary for review

Run `./scripts/migration-phase3/survey.sh --help` for the full flag list and
environment variables.

**Direct invocation** — drives `custom_migrator` yourself:

```bash
custom_migrator scan \
    --m5-addr http://localhost:50055 \
    --output ./scan.json

custom_migrator translate \
    --report ./scan.json \
    --output ./proposals.json \
    --markdown ./summary.md
```

### What to do with the output

- Send `summary.md` to the metric owners — it groups proposals by tier and
  lists each metric's `original_metric_id`, the destination type, and the
  human-readable reason from the classifier.
- Collect go / no-go answers per proposal. Tier 3 entries always require a
  follow-up — they are not shadow-runnable in their current form and need
  the metric owner to decide whether to redesign as a structured / MetricQL
  metric or leave it on CUSTOM until further notice.
- `proposals.json` (machine-readable) is the binding input for the
  `shadow` subcommand. Trim any owner-rejected proposals out of the file
  before proceeding (or carry them through and rely on the operator review
  at `apply --dry-run` to filter them out — both work, the former is
  cleaner).

---

## Step 2 — Shadow run

### What it does

`custom_migrator shadow` reads the proposals JSON, filters to Tier 1 + Tier
2 (Tier 3 is skipped with a warning), and for each surviving proposal:

1. Calls `M3::ScheduleShadowComputation(original_metric_id, candidate_metric)`.
2. Polls `M3::PromoteShadowResult` on a cadence (default 60s, override via
   `CUSTOM_MIGRATOR_POLL_INTERVAL_SECS`) until terminal status or
   `--duration` exhausts.
3. Snapshots `days_within_tolerance` / `total_days` from
   `M3::GetShadowResults` for the audit trail.

M3 owns the 7-consecutive-days-within-tolerance gate (PR #580, Lock L3) —
`PromoteShadowResult` will not return `APPROVED` until a run accumulates 7
clean days. The migrator does not double-check the gate; it trusts M3's
verdict.

### How to run

```bash
custom_migrator shadow \
    --proposals ./proposals.json \
    --m3-addr http://localhost:50056 \
    --duration 14d \
    --output ./shadow.json
```

### Why 14d, not 7d

7 days is the minimum M3 will accept. In practice, a (metric, experiment,
variant, day) tuple with no data on either side does not count toward the
7. Budget extra days for partial-data windows so a real-world run reaches
APPROVED inside the polling budget instead of expiring at `PENDING`.

### Outcome statuses

The shadow output is a `ShadowOutput` JSON with one entry per proposal.
The `status` field is one of:

| Status | Meaning | Operator action |
|--------|---------|-----------------|
| `APPROVED` | 7+ consecutive days within tolerance; M3 returned an APPROVED `result_id` | Carry into `apply` |
| `REJECTED` | At least one (experiment, variant, day) tuple failed the tolerance check | Investigate via `GetShadowResults`; do NOT re-run blindly |
| `PENDING` | `--duration` exhausted before terminal status | Re-run with longer `--duration`; check M3 health |
| `SCHEDULING_FAILED` | `ScheduleShadowComputation` rejected the candidate (validator failure, M3 unreachable, etc.) | Read the error reason; fix the proposal or M3, then re-run |
| `FAILED` | A transient infra error during polling | Re-run; check M3 + network |

### When to repeat

- `PENDING` with budget exhausted → re-run with longer `--duration`, or
  verify the original metric is actually computing during the shadow
  window (no data = no progress).
- `REJECTED` → DO NOT re-run blindly. Pull the per-tuple diff rows via
  `M3::GetShadowResults(shadow_run_id)` and inspect `diff_abs` / `diff_rel`
  / `within_tolerance`. The classifier may have produced a semantically
  correct but numerically different proposal (e.g., off-by-one boundary on
  a windowed count); the metric owner needs to either accept the drift or
  redesign the candidate.

---

## Step 3 — Apply

Apply is a two-step process by design (Lock L7): always dry-run first,
then confirm.

### Dry-run

```bash
custom_migrator apply \
    --shadow-results ./shadow.json \
    --m5-addr http://localhost:50055 \
    --dry-run
```

Prints the planned migrations as a table to stdout. Filters automatically
to `status == APPROVED` and a non-empty `result_id`. No M5 mutations.

### Confirm

```bash
custom_migrator apply \
    --shadow-results ./shadow.json \
    --m5-addr http://localhost:50055 \
    --confirm \
    --operator alice@example.com
```

`--operator` is required when `--confirm` is set; it lands in the
`metric_migrations.operator` audit column as plain text.

### What apply guarantees (Lock L7)

- **Old CUSTOM row preserved.** Apply is forward-only and NEVER destructive
  in place. The original `metric_definitions` row stays queryable; new
  experiments simply reference the new `metric_id`.
- **Single atomic transaction per migration.** The new `metric_definitions`
  row and the corresponding `metric_migrations` audit row commit together
  inside one PostgreSQL transaction. A crash mid-transaction rolls back
  both — no partial state.
- **Apply-as-much-as-possible.** Per-outcome failures do NOT abort the run;
  the migrator records them and continues with the next outcome. The
  process exits with code `1` if any outcome failed, `0` if all succeeded
  or there was nothing to apply.

### Audit trail

Every successful migration writes a row to `metric_migrations` (audit
table from migration `016_adr026_phase3_metric_migrations.sql`). The row
includes the operator string, both metric IDs, the M3 `shadow_run_result_id`
that was promoted, and the wall-clock timestamp.

The migrator also writes a sidecar `ApplyOutput` JSON for each `--confirm`
run. By default this lands next to the shadow-results file with an
`.apply.json` suffix (e.g. `shadow.json` → `shadow.json.apply.json`); pass
`--output <path>` to override. Dry-run mode does not emit a sidecar.

### Exit codes

| Code | Meaning |
|------|---------|
| `0`  | All APPROVED outcomes applied successfully, or there was nothing to apply |
| `1`  | At least one outcome failed; sidecar audit lists the per-outcome status |
| `2`  | Fatal — could not read input file, could not connect to M5, mutually-exclusive flags violated, etc. |

---

## Step 4 — Rollback

Apply is **forward-only by design.** There is no `custom_migrator rollback`
subcommand. Per Lock L7, apply is never destructive in place: the old
CUSTOM row is preserved at all times.

### To "undo" a migration

1. Pause or end any experiments that reference the new `metric_id`.
2. Edit those experiments to point back at the original CUSTOM
   `metric_id` (still present in `metric_definitions`).
3. Optionally delete the new `metric_definitions` row via direct DB
   access — leave the `metric_migrations` audit row in place as history.

### Why no automated rollback

A migrated metric may already be the basis of post-migration experiment
analysis. An automated rollback that flipped the new row out from under
those experiments would silently invalidate every analysis already
computed against it. Rollback requires operator judgment about which
downstream consumers are affected and how to communicate the change; the
tool deliberately does not attempt that judgment.

---

## Acceptance-criteria mapping (F1)

The following table maps each #437 acceptance criterion to the test or
file where it is enforced.

| #437 AC | Test / file location | Notes |
|---------|----------------------|-------|
| Migration tool: scan + classify + report | Classifier unit tests in `crates/experimentation-management/src/migration/classifier.rs` (15 `#[test]`s in `mod tests`); Tier 1 / Tier 2 builders in `migration/tier1.rs` + `migration/tier2.rs`; report rendering in `migration/report.rs` (`mod tests`); corpus parity in `crates/experimentation-management/tests/custom_corpus_parity.rs` against `test-vectors/custom_migration_corpus.json` | Phase A (#577) |
| Shadow-run with row-level equivalence | Differ unit tests in `services/metrics/internal/shadow/differ_test.go` (19 `func Test*`); promote-gate in `services/metrics/internal/shadow/promote_test.go`; shadow handler in `services/metrics/internal/handler/shadow_handler_test.go` (15 `func Test*`); runner integration in `services/metrics/internal/jobs/shadow_runner_test.go` (14 `func Test*`) | Phase B (#580) |
| M5 `CreateMetric` for CUSTOM emits deprecation warning | `crates/experimentation-management/tests/metric_deprecation_e2e_test.rs::create_custom_emits_deprecation_header` (plus the negative-case companions `create_mean_does_not_emit_deprecation_header` and `create_filtered_mean_does_not_emit_deprecation_header`); handler at `crates/experimentation-management/src/grpc.rs` (search for `"x-kaizen-deprecation"`) | Phase C (#578) |
| M5 `UpdateMetric` warns on change-to-CUSTOM | _Not applicable — `UpdateMetricDefinition` RPC does not exist; metrics are immutable by design (see ADR-026 § "Implementation status" and the `m5-metric-definitions.md` runbook). The deprecation surface is owned entirely by `CreateMetricDefinition` (row above)._ | Phase C (#578) |
| M6 CUSTOM labeled "Deprecated" | `ui/src/components/metrics/metric-type-select.test.tsx` (5 tests covering the deprecated label in the option list, the warning icon when CUSTOM is selected, the negative case for non-CUSTOM types, and the onChange callback) | Phase D (#578) |
| M6 deprecation toast on create-page mount | `ui/src/__tests__/metric-custom-deprecation-toast.test.tsx` (3 `describe` blocks: `shouldShowCustomDeprecationToast`, `DEPRECATION_TOAST_MESSAGE`, and `NewMetricPage deprecation toast`); helper at `ui/src/lib/metric-deprecation.ts` | Phase D (#578) |
| Migration guide doc | `docs/runbooks/adr-026-phase-3-migration.md` (this file) | This PR (#437) |
| Hide CUSTOM from form after sunset (4 weeks) | _Deferred — the M6 feature flag `m6.metric_type.custom.hidden` (L6 Phase 3.B) is documented as a follow-up. The Phase D label + toast tests above are the shipped subset; the flag-toggle hide is queued for the post-sunset PR once telemetry confirms zero new CUSTOMs for 2 consecutive weeks._ | Phase D follow-up |
| **`MigrateMetricDefinition` RPC + `metric_migrations` audit table** | `crates/experimentation-management/tests/metric_migration_e2e_test.rs` (14 `#[tokio::test]`s — happy path, the four L7 precondition gates, atomic-transaction rollback, unique-constraint conflicts, precondition ordering); migration `sql/migrations/016_adr026_phase3_metric_migrations.sql`; handler at `crates/experimentation-management/src/grpc.rs::migrate_metric_definition` | This PR (T1, c259513) |
| **`custom_migrator shadow` subcommand** | `crates/experimentation-management/src/bin/custom_migrator.rs` `mod tests` shadow group (5 `#[tokio::test]`s: `happy_path_two_proposals_both_approved`, `rejected_outcome_propagates_reason_and_empty_result_id`, `pending_outcome_when_budget_exhausts`, `scheduling_failure_is_recorded_but_does_not_abort_remaining_proposals`, `tier3_proposals_are_skipped_with_warning`) plus 7 sync helpers (`parse_duration_*`, `is_shadow_eligible_tier_*`, `log_summary_counts_only_approved`, `extract_shadow_candidates_*`) | This PR (T2, bea93e6) |
| **`custom_migrator apply` subcommand** | `crates/experimentation-management/src/bin/custom_migrator.rs` `mod tests` apply group (6 `#[tokio::test]`s: `apply_dry_run_prints_plan_and_makes_no_rpc_calls`, `apply_confirm_applies_all_approved_outcomes`, `apply_confirm_continues_on_partial_failure_and_exits_nonzero`, `apply_confirm_without_operator_fails_before_any_rpc`, `apply_with_no_approved_outcomes_is_a_noop_exit_zero`, `apply_confirm_with_custom_output_path_writes_audit_there`) plus 4 sync helpers (`partition_outcomes_*`, `describe_candidate_type_*`, `default_audit_path_*`, `cli_rejects_dry_run_and_confirm_together`) | This PR (T3, 4855514) |

---

## Troubleshooting

### Shadow REJECTED — what does the diff look like

Read the per-tuple rows via M3's `GetShadowResults(shadow_run_id)`. Each
row carries `diff_abs`, `diff_rel`, and `within_tolerance`. Group by
(experiment, variant, day) and look at the rows where `within_tolerance =
false`:

- If the absolute diffs cluster near zero but `within_tolerance` is still
  false, the tolerance is tight (e.g., proportion metrics tolerate
  zero difference); confirm the metric type matches the proposal.
- If `diff_abs` is large and clusters by day, suspect a windowing
  off-by-one (a common Tier 1 windowed_count failure mode).
- If a single tuple has a huge diff and the rest are clean, there may be
  a data-quality issue with the underlying events on that day; check
  whether the original CUSTOM query was silently filtering nulls.

### Shadow PENDING with budget exhausted

The migrator polled for `--duration` without M3 reaching APPROVED /
REJECTED. Common causes:

- M3 hasn't accumulated 7 days of clean data yet. Re-run with longer
  `--duration` (e.g., `21d`).
- The original CUSTOM metric is not actually computing during the shadow
  window — no data on the "left" side means no progress. Check the
  original metric's recent computation status in M3.
- M3 polling fell behind. Inspect M3 logs for shadow scheduler stalls.

### `apply --confirm` returns `AlreadyExists`

The unique constraint `uq_metric_migrations_old` rejects a second
migration of the same `original_metric_id`. The metric has already been
migrated; query the existing `metric_migrations` row to see when and by
whom:

```sql
SELECT migration_id, operator, applied_at, new_metric_id
FROM metric_migrations
WHERE old_metric_id = '<id>';
```

If the previous migration was wrong, follow "Step 4 — Rollback" above
before authoring a new migration to a different `new_metric_id`.

### `apply --confirm` returns `FailedPrecondition`

The M5 handler re-validates the `shadow_run_result_id` with M3 at apply
time (Lock L7 precondition (d)). If M3 has since invalidated the result
(typically because the underlying metric SQL has changed since the shadow
was approved), the handler rejects the apply with `FailedPrecondition`.
Re-run `custom_migrator shadow` against the current proposal to obtain a
fresh `result_id`.

### Operator made a typo in `--operator`

The `metric_migrations.operator` column is plain text and immutable; the
audit row records exactly what was passed. There is no in-band correction.
Use the `migration_id` from the sidecar `ApplyOutput` JSON in any
subsequent communications, and consider adding a follow-up audit note in
your team's ticket tracker rather than mutating the row.

---

## References

- ADR-026: `docs/adrs/026-custom-metrics-layer.md`
- Locked plan: `docs/superpowers/plans/2026-05-30-adr-026-phase-3-custom-migration.md`
- M5 Custom Metric Definitions (Phase 1 operator runbook): `docs/runbooks/m5-metric-definitions.md`
- M3 Analysis runbook: `docs/runbooks/m4a-analysis.md`
- Migration tool entry point: `crates/experimentation-management/src/bin/custom_migrator.rs`
- Migration classifier: `crates/experimentation-management/src/migration/classifier.rs`
- M5 `MigrateMetricDefinition` handler: `crates/experimentation-management/src/grpc.rs` (search for `migrate_metric_definition`)
- M3 shadow scheduler: `services/metrics/internal/shadow/` (Go) + `services/metrics/internal/handler/shadow_handler.go`
- Audit migration: `sql/migrations/016_adr026_phase3_metric_migrations.sql`
- Corpus fixtures: `test-vectors/custom_migration_corpus.json`
- Survey wrapper: `scripts/migration-phase3/survey.sh`
- Issues: `#437` (this work), `#577` (Phase A), `#578` (Phase C / D), `#580` (Phase B)
