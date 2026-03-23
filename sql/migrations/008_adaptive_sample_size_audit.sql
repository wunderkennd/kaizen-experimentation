-- ============================================================================
-- ADR-020: Adaptive Sample Size Recalculation
--
-- Tracks every interim analysis trigger and the resulting zone classification
-- / extension decision. One row per trigger event (not per experiment).
-- ============================================================================

CREATE TABLE adaptive_sample_size_audit (
    id                  BIGSERIAL PRIMARY KEY,
    experiment_id       UUID    NOT NULL REFERENCES experiments(experiment_id) ON DELETE CASCADE,

    -- When the interim check fired.
    triggered_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Inputs to the conditional-power computation.
    interim_fraction    DOUBLE PRECISION NOT NULL,
    n_interim_per_arm   DOUBLE PRECISION NOT NULL,
    n_max_per_arm       DOUBLE PRECISION NOT NULL,
    observed_effect     DOUBLE PRECISION NOT NULL,
    blinded_variance    DOUBLE PRECISION NOT NULL,

    -- Outputs of the computation.
    conditional_power   DOUBLE PRECISION NOT NULL CHECK (conditional_power BETWEEN 0 AND 1),
    zone                TEXT NOT NULL CHECK (zone IN ('favorable', 'promising', 'futile')),

    -- For promising zone: recommended extension.
    recommended_n_max   DOUBLE PRECISION,   -- NULL when zone != 'promising'
    extended            BOOLEAN NOT NULL DEFAULT FALSE,

    -- Who/what triggered this (e.g. "adaptive_n_scheduler").
    actor               TEXT NOT NULL DEFAULT 'adaptive_n_scheduler'
);

CREATE INDEX idx_adaptive_n_audit_experiment_id
    ON adaptive_sample_size_audit (experiment_id, triggered_at DESC);
