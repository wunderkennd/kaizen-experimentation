-- ADR-026 Phase 3 (#437): metric migration audit log.
--
-- Background
-- ----------
-- ADR-026 Phase 3 introduces a two-step apply contract (Lock L7) for migrating
-- CUSTOM (raw-SQL) metric definitions to a structured / MetricQL replacement
-- once their shadow run has been APPROVED by M3. The migration tool
-- ("custom_migrator") generates per-metric JSON proposals; the operator applies
-- them via M5's MigrateMetricDefinition RPC. That RPC creates the new metric
-- AND writes a soft-link record into the table introduced here.
--
-- Schema overview
-- ---------------
-- One row per applied migration. The old CUSTOM metric STAYS in
-- metric_definitions (audit + read-compat per ADR-026 Phase 3 non-goals), so
-- queries that need the historical SQL can still find it. This table is the
-- soft-link that says "metric X has been migrated; metric Y is the
-- replacement; the migration was authorised by shadow run Z".
--
-- Relationship to other migrations
-- ---------------------------------
--   Migration 011 (ADR-026 Phase 1): added FILTERED_MEAN / COMPOSITE /
--     WINDOWED_COUNT to metric_definitions.
--   Migration 013 (ADR-026 Phase 2): added metricql_expression to
--     metric_definitions.
--   Migration 015 (ADR-026 Phase 3 — shadow pipeline): added
--     metric_shadow_runs + metric_shadow_run_results.
--   This migration (016) adds the audit log MigrateMetricDefinition writes
--     after a successful APPROVED-gated apply. It does NOT touch
--     metric_definitions; the new structured metric goes in via the normal
--     INSERT path.
--
-- Rationale notes
-- ---------------
-- * uq_metric_migrations_old prevents double-migration of a single CUSTOM.
--   One row per old_metric_id; rollback (e.g., reverting a bad migration) is
--   a manual ops operation — delete the row in metric_migrations + the new
--   metric in metric_definitions. We deliberately do not automate this:
--   ADR-026 Phase 3 commits to non-destructive migration only.
--
-- * No FK on old_metric_id because the old CUSTOM row stays in
--   metric_definitions and is never deleted by this RPC. A FK would also
--   gate rollback (couldn't drop the old metric without first dropping the
--   migration record) which is the wrong default for an audit table.
--
-- * No FK on shadow_run_result_id because M3's metric_shadow_runs lives in
--   the M3-owned database. Cross-service FKs are an anti-pattern in this
--   stack (Lock L3); the gate is enforced at the M5 handler boundary by
--   calling M3's GetShadowResults RPC and verifying status == APPROVED.
--
-- * FK on new_metric_id with ON DELETE RESTRICT is safe: the new metric
--   row is created in the same transaction as this audit row, so the row
--   exists by the time we link to it. The RESTRICT ensures an operator
--   cannot drop the new metric without first reckoning with this audit row.
--
-- Replayability
-- -------------
-- All DDL uses CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS so
-- this migration is safely re-runnable on dev databases that have been
-- partially seeded (matches the style of migrations 011, 013, and 015).

-- ============================================================
-- Table: metric_migrations
-- ============================================================
-- One row per applied migration of a CUSTOM metric to a structured /
-- MetricQL replacement. Written atomically with the new metric_definitions
-- row by MigrateMetricDefinition.
CREATE TABLE IF NOT EXISTS metric_migrations (
    migration_id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    old_metric_id        TEXT        NOT NULL,
    new_metric_id        TEXT        NOT NULL,
    shadow_run_result_id UUID        NOT NULL,
    operator             TEXT        NOT NULL,
    applied_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT fk_metric_migrations_new
        FOREIGN KEY (new_metric_id) REFERENCES metric_definitions(metric_id) ON DELETE RESTRICT,
    CONSTRAINT uq_metric_migrations_old UNIQUE (old_metric_id)
);

-- ============================================================
-- Index: applied_at (btree)
-- ============================================================
-- Operators reading the audit log usually do so by recency ("show me the
-- last 20 migrations"). A btree on applied_at makes that query cheap.
CREATE INDEX IF NOT EXISTS idx_metric_migrations_applied_at
    ON metric_migrations (applied_at);
