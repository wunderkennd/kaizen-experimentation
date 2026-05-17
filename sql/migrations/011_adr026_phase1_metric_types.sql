-- ADR-026 Phase 1: admit structured custom metric types (#433).
--
-- This migration extends `metric_definitions` to support the three Tier 1
-- structured types introduced by ADR-026:
--   FILTERED_MEAN  — AVG(value_column) over rows matching a Spark SQL filter.
--   COMPOSITE      — arithmetic combination of other already-computed metrics.
--   WINDOWED_COUNT — count of events in [exposure_ts, exposure_ts + window_hours).
--
-- Schema changes:
--   1. Loosen the inline `type` CHECK so the new enum values are admitted at the
--      DB layer. The inline constraint in 001_schema.sql is auto-named
--      `metric_definitions_type_check` by PostgreSQL; we drop and re-add it with
--      the expanded value set. The legacy 6 types remain valid; structural
--      validation (operand existence, cycle detection, filter SQL allowlist)
--      lives in the M5 application layer (`experimentation-management`).
--   2. Add a JSONB `type_config` column to persist the per-type oneof payload
--      (`FilteredMeanConfig`, `CompositeConfig`, `WindowedCountConfig`).
--      The legacy 6 types continue to use their flat columns (`source_event_type`,
--      `numerator_event_type`, `custom_sql`, etc.) as the authoritative source.
--   3. Add a GIN index on `type_config->'operands'` so the COMPOSITE cycle
--      detector can resolve operand graphs without table scans.
--
-- Behavioral notes:
--   - Existing rows continue to satisfy the new CHECK (their `type` values are
--     a subset of the new allow-list) and have NULL `type_config`, which is
--     the correct shape for the legacy 6 types.
--   - `IF NOT EXISTS` / `IF EXISTS` clauses make this migration safely
--     replayable for dev databases that have already been partially seeded.

ALTER TABLE metric_definitions
    DROP CONSTRAINT IF EXISTS metric_definitions_type_check;

ALTER TABLE metric_definitions
    ADD CONSTRAINT metric_definitions_type_check
        CHECK (type IN (
            'MEAN', 'PROPORTION', 'RATIO', 'COUNT', 'PERCENTILE', 'CUSTOM',
            'FILTERED_MEAN', 'COMPOSITE', 'WINDOWED_COUNT'
        ));

ALTER TABLE metric_definitions
    ADD COLUMN IF NOT EXISTS type_config JSONB;

CREATE INDEX IF NOT EXISTS idx_metric_definitions_type_config_operands
    ON metric_definitions USING gin ((type_config -> 'operands'));
