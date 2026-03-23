-- ADR-014: Add stakeholder and aggregation_level to metric_definitions.
--
-- stakeholder: who benefits from this metric (USER / PROVIDER / PLATFORM).
--   Drives multi-stakeholder analysis routing in M4a.
-- aggregation_level: unit of observation for M3 computation (USER / EXPERIMENT / PROVIDER).
--   USER     → standard per-user analysis (required for bandit rewards).
--   EXPERIMENT → whole-experiment aggregation (valid for guardrails only).
--   PROVIDER → per-content-provider aggregation (requires stakeholder=PROVIDER).
--
-- Both columns default to '' so existing rows remain valid during migration;
-- new metrics must supply explicit values (enforced at the application layer).

ALTER TABLE metric_definitions
    ADD COLUMN IF NOT EXISTS stakeholder       TEXT NOT NULL DEFAULT ''
        CHECK (stakeholder IN ('', 'USER', 'PROVIDER', 'PLATFORM')),
    ADD COLUMN IF NOT EXISTS aggregation_level TEXT NOT NULL DEFAULT ''
        CHECK (aggregation_level IN ('', 'USER', 'EXPERIMENT', 'PROVIDER'));
