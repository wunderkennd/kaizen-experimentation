-- Migration 009: Online FDR Controller state table (ADR-018, ADR-025 Phase 3)
--
-- Stores the singleton OnlineFdrController state for the platform.
-- The controller maintains the e-value alpha wealth budget and rejection count
-- across all experiment conclusions. This is a singleton (one row per platform).
--
-- alpha_wealth tracks the remaining budget for future discoveries.
-- Wealth starts at alpha_0 and decreases when discoveries are claimed via
-- the e-BH procedure. It can grow back when experiments conclude with large
-- e-values (GROW martingale).
--
-- Reference: Ramdas & Wang (2024) "Hypothesis Testing with E-values" §6.

CREATE TABLE IF NOT EXISTS fdr_controller_state (
    id              INTEGER PRIMARY KEY DEFAULT 1,  -- singleton row
    alpha_0         DOUBLE PRECISION NOT NULL,       -- initial alpha wealth
    alpha_wealth    DOUBLE PRECISION NOT NULL,       -- current alpha wealth
    rejection_count INTEGER NOT NULL DEFAULT 0,     -- cumulative rejections
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Ensure only one row.
    CONSTRAINT fdr_controller_singleton CHECK (id = 1)
);

-- Audit log: one row per experiment conclusion evaluated by the controller.
CREATE TABLE IF NOT EXISTS fdr_controller_audit (
    audit_id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    experiment_id   UUID NOT NULL,
    e_value         DOUBLE PRECISION NOT NULL,
    log_e_value     DOUBLE PRECISION NOT NULL,
    rejected        BOOLEAN NOT NULL,
    alpha_wealth_before DOUBLE PRECISION NOT NULL,
    alpha_wealth_after  DOUBLE PRECISION NOT NULL,
    rejection_count_after INTEGER NOT NULL,
    evaluated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS fdr_controller_audit_exp_idx
    ON fdr_controller_audit (experiment_id);
CREATE INDEX IF NOT EXISTS fdr_controller_audit_evaluated_at_idx
    ON fdr_controller_audit (evaluated_at DESC);
