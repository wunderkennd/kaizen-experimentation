-- ============================================================================
-- Migration 009: M5 Phase 2 / ADR-025 schema extensions
-- Adds PAUSED state, Phase 5 experiment types, paused_at timestamp,
-- and pause_reason for guardrail auto-pause audit.
-- ============================================================================

-- PAUSED state + Phase 5 types require re-creating the CHECK constraints.
-- PostgreSQL does not support ALTER CONSTRAINT easily, so we drop and re-add.

ALTER TABLE experiments
    DROP CONSTRAINT IF EXISTS experiments_state_check;

ALTER TABLE experiments
    ADD CONSTRAINT experiments_state_check
    CHECK (state IN (
        'DRAFT', 'STARTING', 'RUNNING', 'PAUSED',
        'CONCLUDING', 'CONCLUDED', 'ARCHIVED'
    ));

ALTER TABLE experiments
    DROP CONSTRAINT IF EXISTS experiments_type_check;

ALTER TABLE experiments
    ADD CONSTRAINT experiments_type_check
    CHECK (type IN (
        'AB', 'MULTIVARIATE', 'INTERLEAVING', 'SESSION_LEVEL',
        'PLAYBACK_QOE', 'MAB', 'CONTEXTUAL_BANDIT', 'CUMULATIVE_HOLDOUT',
        'META', 'SWITCHBACK', 'QUASI'
    ));

-- Timestamps for PAUSED state management.
ALTER TABLE experiments
    ADD COLUMN IF NOT EXISTS paused_at        TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS resumed_at       TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS pause_reason     TEXT;

-- AVLM in sequential_method (ADR-015).
ALTER TABLE experiments
    DROP CONSTRAINT IF EXISTS experiments_sequential_method_check;

ALTER TABLE experiments
    ADD CONSTRAINT experiments_sequential_method_check
    CHECK (sequential_method IN ('MSPRT', 'GST_OBF', 'GST_POCOCK', 'AVLM'));

-- Index for PAUSED state queries (guardrail dashboard).
CREATE INDEX IF NOT EXISTS idx_experiments_paused
    ON experiments(paused_at DESC)
    WHERE state = 'PAUSED';
