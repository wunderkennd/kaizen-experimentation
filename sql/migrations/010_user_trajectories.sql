-- ============================================================================
-- Migration 010: user_trajectories Delta Lake table for ORL (ADR-017 Phase 2)
--
-- MDP trajectory data for offline RL estimation. Constructed by M3 by joining
-- exposures, metrics, and assignments along the time axis per user.
-- Only computed for ORL-enabled experiments. Adds ~30 min to daily metric job.
-- ============================================================================

CREATE TABLE IF NOT EXISTS user_trajectories (
    -- Experiment this trajectory belongs to.
    experiment_id TEXT NOT NULL,
    -- User who generated this trajectory.
    user_id TEXT NOT NULL,
    -- Time step within the experiment (0-indexed).
    trajectory_step INT NOT NULL,
    -- User state feature vector at this step (engagement history, consumption pattern).
    -- Encoded as a JSON array of doubles for portability; Delta Lake stores as ARRAY<DOUBLE>.
    state_features JSONB NOT NULL,
    -- Recommendation action taken at this step.
    action_id TEXT NOT NULL,
    -- Immediate reward observed (e.g., engagement metric).
    reward DOUBLE PRECISION NOT NULL,
    -- User state feature vector at the next step (after transition).
    -- NULL for terminal steps.
    next_state_features JSONB,
    -- P(action | state) under the logging (behavior) policy.
    -- Required for importance weighting in DR-OPE. Must be in (0, 1].
    logging_probability DOUBLE PRECISION NOT NULL,
    -- Timestamp of this trajectory step.
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Composite primary key: one row per (experiment, user, step).
    PRIMARY KEY (experiment_id, user_id, trajectory_step)
);

-- Index for M4a queries: fetch all trajectories for an experiment.
CREATE INDEX IF NOT EXISTS idx_user_trajectories_experiment
    ON user_trajectories (experiment_id);

-- Index for per-user trajectory lookups (M3 incremental updates).
CREATE INDEX IF NOT EXISTS idx_user_trajectories_user
    ON user_trajectories (experiment_id, user_id);

-- Constraint: logging_probability must be in (0, 1].
ALTER TABLE user_trajectories
    ADD CONSTRAINT user_trajectories_logging_probability_check
    CHECK (logging_probability > 0.0 AND logging_probability <= 1.0);

-- Constraint: trajectory_step must be non-negative.
ALTER TABLE user_trajectories
    ADD CONSTRAINT user_trajectories_step_check
    CHECK (trajectory_step >= 0);

COMMENT ON TABLE user_trajectories IS
    'MDP trajectory data for offline RL estimation (ADR-017 Phase 2). '
    'Partitioned by experiment_id in Delta Lake. '
    'Constructed by M3 joining exposures, metrics, and assignments per user.';
