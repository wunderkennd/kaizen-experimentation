-- ADR-026 Phase 3 (#437): shadow-run pipeline for CUSTOM-metric migration.
--
-- Background
-- ----------
-- ADR-026 Phase 3 introduces a mechanism to safely migrate CUSTOM (raw-SQL)
-- metric definitions to one of the Phase 1/2 structured types
-- (FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT, METRICQL) without disrupting
-- running experiments. The migration tool ("custom_migrator") submits a
-- candidate MetricDefinition; M3 runs it in parallel alongside the original
-- during the standard nightly pass; a differ records per-(experiment, variant,
-- day) equivalence; and PromoteShadowResult enforces a
-- 7-consecutive-days-within-tolerance gate before M5 accepts the promotion.
--
-- Schema overview
-- ---------------
-- Two new tables are introduced:
--
--   metric_shadow_runs
--     One row per migration attempt.  The migration tool calls
--     ScheduleShadowComputation, which inserts a PENDING row here.  M3's
--     nightly pass picks up PENDING rows, transitions them to RUNNING, executes
--     both the original and candidate definitions, writes per-tuple results to
--     metric_shadow_run_results, then transitions back to PENDING (awaiting the
--     7-day gate) or to FAILED on error.  The migration tool's "apply"
--     subcommand calls PromoteShadowResult, which reads the result aggregate and
--     transitions to APPROVED or REJECTED.
--
--   metric_shadow_run_results
--     One row per (shadow_id, experiment_id, variant_id, computation_date)
--     tuple produced by the differ (B3).  The promotion evaluator (B1
--     EvaluatePromotion) reads these rows and counts how many distinct dates
--     have ALL tuples within tolerance.
--
-- Relationship to other migrations
-- ---------------------------------
--   Migration 011 (ADR-026 Phase 1): added FILTERED_MEAN / COMPOSITE /
--     WINDOWED_COUNT to metric_definitions.
--   Migration 013 (ADR-026 Phase 2): added metricql_expression to
--     metric_definitions.
--   This migration (015) adds the shadow-run tables needed for the promotion
--     gate.  It does NOT touch metric_definitions — shadow runs only track
--     the migration workflow, not the definition itself.
--
-- Replayability
-- -------------
-- All DDL uses CREATE TABLE IF NOT EXISTS and CREATE INDEX IF NOT EXISTS so
-- this migration is safely re-runnable on dev databases that have been
-- partially seeded (matches the style of migrations 011 and 013).

-- ============================================================
-- Table 1: metric_shadow_runs
-- ============================================================
-- Tracks the lifecycle of a single shadow-run migration attempt.
--
-- status CHECK values mirror Status constants in
-- services/metrics/internal/shadow/shadow.go and must stay in sync:
--   PENDING   — created by ScheduleShadowComputation; waiting for first nightly pass.
--   RUNNING   — M3 nightly pass has picked up this shadow run and is executing it.
--   APPROVED  — PromoteShadowResult: 7+ consecutive days within tolerance.
--   REJECTED  — PromoteShadowResult: ≥1 day outside tolerance.
--   FAILED    — M3 nightly pass encountered an execution error.
CREATE TABLE IF NOT EXISTS metric_shadow_runs (
    shadow_id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    original_metric_id  TEXT        NOT NULL,
    candidate_metric    JSONB       NOT NULL,  -- full MetricDefinition (JSON-serialised proto)
    scheduled_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status              TEXT        NOT NULL CHECK (status IN ('PENDING','RUNNING','APPROVED','REJECTED','FAILED')),
    rejection_reason    TEXT        -- populated for REJECTED/FAILED; NULL otherwise
);

-- ============================================================
-- Table 2: metric_shadow_run_results
-- ============================================================
-- One row per (shadow_id, experiment_id, variant_id, computation_date) tuple.
-- Written by the differ (B3) after each nightly pass.  Read by
-- EvaluatePromotion (B1) and GetShadowResults.
--
-- original_value / candidate_value / diff_abs / diff_rel are NULLABLE because
-- M3 may fail to compute one side (e.g., the original metric's source data is
-- unavailable for a specific date).  The differ sets within_tolerance = false
-- for any tuple where either value is NULL — the NOT NULL constraint on
-- within_tolerance preserves the gate's integrity even when computation is
-- partial.
--
-- diff_abs  = ABS(candidate_value - original_value)
-- diff_rel  = diff_abs / ABS(original_value)   [NULL when original_value = 0]
-- Both are written by B3; EvaluatePromotion consumes within_tolerance directly.
CREATE TABLE IF NOT EXISTS metric_shadow_run_results (
    result_id           UUID             PRIMARY KEY DEFAULT gen_random_uuid(),
    shadow_id           UUID             NOT NULL REFERENCES metric_shadow_runs(shadow_id),
    experiment_id       TEXT             NOT NULL,
    variant_id          TEXT             NOT NULL,
    computation_date    DATE             NOT NULL,
    original_value      DOUBLE PRECISION,  -- NULL if original computation failed for this tuple
    candidate_value     DOUBLE PRECISION,  -- NULL if candidate computation failed for this tuple
    diff_abs            DOUBLE PRECISION,  -- NULL when either side is NULL
    diff_rel            DOUBLE PRECISION,  -- NULL when original_value is NULL or 0
    within_tolerance    BOOLEAN          NOT NULL  -- false when either value is NULL (B3 enforces)
);

-- ============================================================
-- Index: shadow_id + computation_date (btree)
-- ============================================================
-- The differ (B3) and EvaluatePromotion (B1) always filter by shadow_id and
-- optionally aggregate per computation_date.  A btree on (shadow_id,
-- computation_date) gives index-only access for the GROUP BY pattern used by
-- EvaluatePromotion to count distinct passing days.
CREATE INDEX IF NOT EXISTS idx_metric_shadow_run_results_shadow_date
    ON metric_shadow_run_results (shadow_id, computation_date);
