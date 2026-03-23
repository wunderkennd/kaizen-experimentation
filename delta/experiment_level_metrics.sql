-- ============================================================================
-- Delta Lake: experiment_level_metrics
-- ADR-014 Provider-Side Metrics: experiment-scoped aggregate metrics.
-- Unlike delta.metric_summaries (per-user), this table holds one row per
-- (experiment, variant, metric_id) for catalog- and provider-level metrics
-- that cannot be meaningfully disaggregated to the user level.
--
-- Written by M3 ProviderMetricsJob. Read by M4a for provider-side analysis.
-- ============================================================================

CREATE TABLE IF NOT EXISTS delta.experiment_level_metrics (
    experiment_id       STRING      NOT NULL,
    variant_id          STRING      NOT NULL,
    metric_id           STRING      NOT NULL,   -- e.g. 'catalog_coverage_rate'
    metric_value        DOUBLE      NOT NULL,
    computation_date    DATE        NOT NULL
)
USING DELTA
PARTITIONED BY (computation_date DATE, experiment_id STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.autoOptimize.autoCompact' = 'true',
    'delta.logRetentionDuration' = 'interval 365 days'
);
