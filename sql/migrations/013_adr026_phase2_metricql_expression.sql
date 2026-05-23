-- ADR-026 Phase 2 (#435): add metricql_expression column + admit METRICQL as a valid type.
--
-- This migration extends `metric_definitions` for the MetricQL expression
-- language (ADR-026 Phase 2). MetricQL is the hand-rolled DSL covering the
-- ~35% of CUSTOM use cases that the Phase 1 structured types (FILTERED_MEAN,
-- COMPOSITE, WINDOWED_COUNT) cannot express — composed/windowed/filtered
-- metrics with arithmetic and @metric_ref operands.
--
-- Schema changes:
--   1. Add a TEXT `metricql_expression` column holding the raw source.
--      M5 parses + validates at create/update time; M3 re-parses + compiles
--      to Spark SQL at scheduling time. AST is NOT cached — parse-on-render
--      avoids the version-skew tax of storing parsed AST.
--   2. Loosen the type CHECK to admit 'METRICQL' as a valid type tag (parallel
--      to migration 011's pattern for FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT).
--   3. Add a row-level mutual-exclusion CHECK: at most ONE of
--      (custom_sql, type_config, metricql_expression) may be non-null on a
--      single row. M5 and M3 also enforce this in code — DB constraint is
--      defense-in-depth.
--
-- Pre-check: prior to migration 013 there was NO DB-level mutual-exclusion
-- between (custom_sql, type_config). If any row violates the new rule, the
-- ADD CONSTRAINT below would fail with an opaque error. We surface bad rows
-- explicitly so operators can resolve them before re-running.

ALTER TABLE metric_definitions
    ADD COLUMN IF NOT EXISTS metricql_expression TEXT;

-- Extend the CHECK constraint to admit METRICQL.
ALTER TABLE metric_definitions DROP CONSTRAINT IF EXISTS metric_definitions_type_check;
ALTER TABLE metric_definitions ADD CONSTRAINT metric_definitions_type_check
    CHECK (type IN (
        'MEAN','PROPORTION','RATIO','COUNT','PERCENTILE','CUSTOM',
        'FILTERED_MEAN','COMPOSITE','WINDOWED_COUNT',
        'METRICQL'
    ));

-- Enforce mutual exclusion at the row level: at most one of custom_sql,
-- type_config, metricql_expression may be non-null. M5 and M3 also enforce
-- this in code — DB constraint is defense-in-depth.
-- Use DROP-then-ADD with IF EXISTS to make the migration safely replayable
-- (matches the `metric_definitions_type_check` pattern just above and
-- migration 011's style).
ALTER TABLE metric_definitions DROP CONSTRAINT IF EXISTS metric_definitions_single_definition_source;

-- Defensive pre-check: prior to migration 013 there was NO DB-level mutual-exclusion
-- between `custom_sql` and `type_config`. If any existing row violates the new
-- single-source rule, the ADD CONSTRAINT below will fail and roll back the whole
-- migration. Surface the bad rows first so operators see a clear error message
-- before the constraint failure noise.
DO $$
DECLARE
    bad_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO bad_count
    FROM metric_definitions
    WHERE (CASE WHEN custom_sql          IS NOT NULL THEN 1 ELSE 0 END +
           CASE WHEN type_config         IS NOT NULL THEN 1 ELSE 0 END +
           CASE WHEN metricql_expression IS NOT NULL THEN 1 ELSE 0 END) > 1;

    IF bad_count > 0 THEN
        RAISE EXCEPTION 'migration 013: % existing metric_definitions row(s) have more than one of '
            '(custom_sql, type_config, metricql_expression) set. Resolve manually before re-running '
            '(query: SELECT metric_id FROM metric_definitions WHERE '
            '(CASE WHEN custom_sql IS NOT NULL THEN 1 ELSE 0 END + '
            ' CASE WHEN type_config IS NOT NULL THEN 1 ELSE 0 END + '
            ' CASE WHEN metricql_expression IS NOT NULL THEN 1 ELSE 0 END) > 1) '
            'or null out the field that should not be authoritative.', bad_count;
    END IF;
END $$;

ALTER TABLE metric_definitions ADD CONSTRAINT metric_definitions_single_definition_source
    CHECK (
        (CASE WHEN custom_sql           IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN type_config          IS NOT NULL THEN 1 ELSE 0 END +
         CASE WHEN metricql_expression  IS NOT NULL THEN 1 ELSE 0 END) <= 1
    );
