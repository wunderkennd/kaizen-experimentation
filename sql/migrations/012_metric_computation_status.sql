-- sql/migrations/012_metric_computation_status.sql
-- ADR-026 Phase 1 follow-up (#475): record metric computation outcome per (experiment, metric, date)
-- so M4a can distinguish "missing because not scheduled" from "skipped because upstream failed".

CREATE TABLE IF NOT EXISTS metric_computation_status (
    experiment_id     TEXT NOT NULL,
    metric_id         TEXT NOT NULL,
    computation_date  DATE NOT NULL,
    status            TEXT NOT NULL CHECK (status IN (
        'completed',
        'failed',
        'skipped_upstream_failure',
        'skipped_cycle'
    )),
    reason            TEXT,
    recorded_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (experiment_id, metric_id, computation_date)
);

CREATE INDEX IF NOT EXISTS idx_metric_computation_status_lookup
    ON metric_computation_status (experiment_id, computation_date, status);
