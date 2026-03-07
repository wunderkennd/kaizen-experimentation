-- ============================================================================
-- Flag-Experiment Linkage: M7 Phase 3
-- Tracks which experiment was created from a flag via PromoteToExperiment,
-- enabling auto-resolution when the experiment concludes.
-- ============================================================================

-- Track the promoted experiment for each flag.
ALTER TABLE feature_flags
    ADD COLUMN promoted_experiment_id UUID,
    ADD COLUMN promoted_at TIMESTAMPTZ;

-- Index for looking up which flag produced a given experiment.
CREATE INDEX idx_feature_flags_promoted_experiment
    ON feature_flags(promoted_experiment_id) WHERE promoted_experiment_id IS NOT NULL;

-- Index for dependency tracking: find all flags using a targeting rule.
CREATE INDEX idx_feature_flags_targeting_rule
    ON feature_flags(targeting_rule_id) WHERE targeting_rule_id IS NOT NULL;
