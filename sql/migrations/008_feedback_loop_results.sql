-- ADR-021: Feedback loop detection results storage.
--
-- M4a writes one row per (experiment, metric) pair after running
-- FeedbackLoopDetector.  Results are read by M6 UI and M5 alerting.
--
-- Paired t-test + OLS bias correction results from experimentation-stats
-- crate (feedback_loop.rs FeedbackLoopDetector::detect).

CREATE TABLE feedback_loop_results (
    id                              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id                   UUID        NOT NULL REFERENCES experiments(experiment_id) ON DELETE CASCADE,
    metric_id                       TEXT        NOT NULL,
    -- Number of retraining events analysed.
    n_retraining_events             INT         NOT NULL CHECK (n_retraining_events >= 3),
    -- Whether feedback loop contamination was detected.
    feedback_loop_detected          BOOLEAN     NOT NULL,
    -- Two-sided p-value from paired t-test on (post − pre) effect differences.
    paired_ttest_p_value            DOUBLE PRECISION NOT NULL,
    -- Mean treatment effect in the 7-day window before each retraining event.
    mean_pre_retrain_effect         DOUBLE PRECISION NOT NULL,
    -- Mean treatment effect in the 7-day window after each retraining event.
    mean_post_retrain_effect        DOUBLE PRECISION NOT NULL,
    -- mean(post − pre): signed average shift per retraining event.
    mean_effect_shift               DOUBLE PRECISION NOT NULL,
    -- Pearson r between contamination_fraction and post_retrain_effect.
    contamination_effect_correlation DOUBLE PRECISION NOT NULL,
    -- OLS-estimated bias: β₁ × mean(contamination_fraction).
    bias_estimate                   DOUBLE PRECISION NOT NULL,
    -- OLS intercept β₀: best estimate of effect at zero contamination.
    bias_corrected_effect           DOUBLE PRECISION NOT NULL,
    computed_at                     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX feedback_loop_results_exp_metric_idx
    ON feedback_loop_results (experiment_id, metric_id);

CREATE INDEX feedback_loop_results_experiment_idx
    ON feedback_loop_results (experiment_id);
