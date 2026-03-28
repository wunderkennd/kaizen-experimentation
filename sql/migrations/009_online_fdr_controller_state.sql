-- ============================================================================
-- ADR-018 Phase 2: Online FDR Controller State
--
-- Platform-level singleton for e-LOND Online FDR Control (Xu and Ramdas,
-- AISTATS 2024). A single row persists the controller state across process
-- restarts. The row is locked FOR UPDATE on each Test() call so concurrent
-- conclude operations serialize correctly.
--
-- fdr_decisions records the per-experiment outcome of each FDR test,
-- providing a full audit trail and enabling controller state reconstruction.
-- ============================================================================

CREATE TABLE online_fdr_controller_state (
    id              BIGSERIAL PRIMARY KEY,

    -- FDR level (target upper bound on false discovery rate).
    -- Default 0.05.
    alpha           DOUBLE PRECISION NOT NULL DEFAULT 0.05
                        CHECK (alpha > 0 AND alpha < 1),

    -- Geometric decay parameter for the gamma sequence.
    -- gamma_t = (1 - gamma_decay) * gamma_decay^(t-1), summing to 1.
    -- Controls how quickly alpha budget is spent across consecutive tests.
    -- Default 0.9 (slow decay, suitable for platforms running many experiments).
    gamma_decay     DOUBLE PRECISION NOT NULL DEFAULT 0.9
                        CHECK (gamma_decay > 0 AND gamma_decay < 1),

    -- Number of hypotheses (experiments) tested since the controller was created.
    num_tested      BIGINT NOT NULL DEFAULT 0 CHECK (num_tested >= 0),

    -- Number of rejections so far.
    num_rejected    BIGINT NOT NULL DEFAULT 0 CHECK (num_rejected >= 0),

    -- Current alpha wealth: remaining budget for future rejections.
    -- Starts at alpha, decreases with each test, replenished (+alpha) on
    -- each rejection.
    alpha_wealth    DOUBLE PRECISION NOT NULL CHECK (alpha_wealth >= 0),

    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed the singleton row (id = 1).
-- alpha_wealth starts equal to alpha.
INSERT INTO online_fdr_controller_state (alpha, gamma_decay, alpha_wealth)
VALUES (0.05, 0.9, 0.05);

COMMENT ON TABLE online_fdr_controller_state IS
    'Platform-level e-LOND Online FDR controller state (ADR-018 Phase 2). Singleton (id=1).';

COMMENT ON COLUMN online_fdr_controller_state.gamma_decay IS
    'Geometric decay ratio r. gamma_t = (1-r)*r^(t-1). Closer to 1 = slower decay = more power for long experiment streams.';

COMMENT ON COLUMN online_fdr_controller_state.alpha_wealth IS
    'Current alpha wealth W_t. Starts at alpha. Decreased by alpha_t on each test; replenished by alpha on each rejection.';

-- ============================================================================
-- Per-experiment FDR decisions (audit trail).
-- ============================================================================

CREATE TABLE fdr_decisions (
    id              BIGSERIAL PRIMARY KEY,
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id) ON DELETE CASCADE,

    -- The e-value submitted for this experiment.
    e_value         DOUBLE PRECISION NOT NULL,

    -- Alpha level allocated to this test: alpha_wealth * gamma_t.
    -- Reject when e_value >= 1 / alpha_allocated.
    alpha_allocated DOUBLE PRECISION NOT NULL,

    -- Whether the null hypothesis was rejected.
    rejected        BOOLEAN NOT NULL,

    -- Wealth snapshot before and after this decision.
    wealth_before   DOUBLE PRECISION NOT NULL,
    wealth_after    DOUBLE PRECISION NOT NULL,

    -- Controller state at decision time (for reconstruction / audit).
    num_tested_at   BIGINT NOT NULL,
    num_rejected_at BIGINT NOT NULL,

    decided_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_fdr_decisions_experiment
    ON fdr_decisions (experiment_id);

CREATE INDEX idx_fdr_decisions_decided_at
    ON fdr_decisions (decided_at DESC);

COMMENT ON TABLE fdr_decisions IS
    'Per-experiment e-LOND FDR decisions (ADR-018 Phase 2). Full audit trail; controller state can be reconstructed from this table.';
