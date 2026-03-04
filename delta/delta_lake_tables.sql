-- ============================================================================
-- Experimentation Platform: Delta Lake Table Definitions
-- All tables stored on S3/GCS. Partitioned for query efficiency.
-- Written by M2 (events) and M3 (metric summaries). Read by M4a.
-- ============================================================================

-- ============================================================================
-- EVENTS: written by M2 Event Ingestion from Kafka topics
-- ============================================================================

-- Exposure events: records that a user was assigned and shown a variant.
-- Source: exposures Kafka topic.
-- Consumers: M3 (metric computation joins), M4a (SRM checks).
CREATE TABLE IF NOT EXISTS delta.exposures (
    event_id                STRING      NOT NULL,
    experiment_id           STRING      NOT NULL,
    user_id                 STRING      NOT NULL,
    variant_id              STRING      NOT NULL,
    platform                STRING,
    session_id              STRING,
    assignment_probability  DOUBLE,         -- For IPW analysis (bandit experiments)
    interleaving_provenance MAP<STRING, STRING>, -- item_id -> algorithm_id
    bandit_context_json     STRING,
    lifecycle_segment       STRING,         -- TRIAL, NEW, ESTABLISHED, MATURE, AT_RISK, WINBACK
    event_timestamp         TIMESTAMP   NOT NULL,
    ingested_at             TIMESTAMP   NOT NULL DEFAULT CURRENT_TIMESTAMP()
)
USING DELTA
PARTITIONED BY (date_partition STRING)  -- GENERATED ALWAYS AS (CAST(event_timestamp AS DATE))
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.autoOptimize.autoCompact' = 'true',
    'delta.logRetentionDuration' = 'interval 90 days',
    'delta.deletedFileRetentionDuration' = 'interval 30 days'
);

-- Metric events: user actions and system observations.
-- Source: metric_events Kafka topic.
-- Consumers: M3 (aggregation into per-user metric summaries).
CREATE TABLE IF NOT EXISTS delta.metric_events (
    event_id        STRING      NOT NULL,
    user_id         STRING      NOT NULL,
    event_type      STRING      NOT NULL,   -- Maps to MetricDefinition.source_event_type
    value           DOUBLE,
    content_id      STRING,
    session_id      STRING,
    properties      MAP<STRING, STRING>,
    event_timestamp TIMESTAMP   NOT NULL,
    ingested_at     TIMESTAMP   NOT NULL DEFAULT CURRENT_TIMESTAMP()
)
USING DELTA
PARTITIONED BY (date_partition STRING, event_type STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.autoOptimize.autoCompact' = 'true',
    'delta.logRetentionDuration' = 'interval 90 days'
);

-- Reward events: bandit reward observations.
-- Source: reward_events Kafka topic.
-- Consumers: M3 (bandit metric computation), M4b (policy updates via Kafka directly).
CREATE TABLE IF NOT EXISTS delta.reward_events (
    event_id        STRING      NOT NULL,
    experiment_id   STRING      NOT NULL,
    user_id         STRING      NOT NULL,
    arm_id          STRING      NOT NULL,
    reward          DOUBLE      NOT NULL,
    context_json    STRING,
    event_timestamp TIMESTAMP   NOT NULL,
    ingested_at     TIMESTAMP   NOT NULL DEFAULT CURRENT_TIMESTAMP()
)
USING DELTA
PARTITIONED BY (date_partition STRING, experiment_id STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.logRetentionDuration' = 'interval 180 days'
);

-- QoE playback events: video quality telemetry.
-- Source: qoe_events Kafka topic.
-- Consumers: M3 (QoE metric aggregation).
CREATE TABLE IF NOT EXISTS delta.qoe_events (
    event_id                STRING      NOT NULL,
    session_id              STRING      NOT NULL,
    content_id              STRING      NOT NULL,
    user_id                 STRING      NOT NULL,
    time_to_first_frame_ms  BIGINT,
    rebuffer_count          INT,
    rebuffer_ratio          DOUBLE,
    avg_bitrate_kbps        INT,
    resolution_switches     INT,
    peak_resolution_height  INT,
    startup_failure_rate    DOUBLE,
    playback_duration_ms    BIGINT,
    cdn_provider            STRING,
    abr_algorithm           STRING,
    encoding_profile        STRING,
    event_timestamp         TIMESTAMP   NOT NULL,
    ingested_at             TIMESTAMP   NOT NULL DEFAULT CURRENT_TIMESTAMP()
)
USING DELTA
PARTITIONED BY (date_partition STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.logRetentionDuration' = 'interval 90 days'
);

-- ============================================================================
-- METRIC SUMMARIES: written by M3 Spark jobs. Read by M4a for analysis.
-- ============================================================================

-- Per-user metric summaries. One row per (experiment, user, metric, segment).
-- M4a reads these for treatment effect computation.
CREATE TABLE IF NOT EXISTS delta.metric_summaries (
    experiment_id       STRING      NOT NULL,
    user_id             STRING      NOT NULL,
    variant_id          STRING      NOT NULL,
    metric_id           STRING      NOT NULL,
    lifecycle_segment   STRING,             -- NULL if lifecycle stratification disabled
    metric_value        DOUBLE      NOT NULL,
    -- Pre-experiment covariate value (for CUPED). NULL if no covariate configured.
    cuped_covariate     DOUBLE,
    session_count       INT,                -- For session-level: number of sessions
    computation_date    DATE        NOT NULL
)
USING DELTA
PARTITIONED BY (computation_date DATE, experiment_id STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.autoOptimize.autoCompact' = 'true',
    'delta.logRetentionDuration' = 'interval 365 days'
);

-- Interleaving scores. One row per (experiment, user).
-- Computed by M3 from interleaving provenance + engagement events.
-- Read by M4a for sign test and Bradley-Terry.
CREATE TABLE IF NOT EXISTS delta.interleaving_scores (
    experiment_id       STRING      NOT NULL,
    user_id             STRING      NOT NULL,
    algorithm_scores    MAP<STRING, DOUBLE> NOT NULL, -- algorithm_id -> credit score
    winning_algorithm_id STRING,
    total_engagements   INT         NOT NULL,
    computation_date    DATE        NOT NULL
)
USING DELTA
PARTITIONED BY (computation_date DATE, experiment_id STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.logRetentionDuration' = 'interval 365 days'
);

-- Content consumption distributions. One row per (experiment, variant, content).
-- Used by M4a for interference analysis (Jaccard, Gini, JS divergence).
CREATE TABLE IF NOT EXISTS delta.content_consumption (
    experiment_id       STRING      NOT NULL,
    variant_id          STRING      NOT NULL,
    content_id          STRING      NOT NULL,
    watch_time_seconds  DOUBLE      NOT NULL,
    view_count          BIGINT      NOT NULL,
    unique_viewers      BIGINT      NOT NULL,
    computation_date    DATE        NOT NULL
)
USING DELTA
PARTITIONED BY (computation_date DATE, experiment_id STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.logRetentionDuration' = 'interval 365 days'
);

-- Daily treatment effect time series. Used by M4a for novelty detection.
-- One row per (experiment, metric, day).
CREATE TABLE IF NOT EXISTS delta.daily_treatment_effects (
    experiment_id       STRING      NOT NULL,
    metric_id           STRING      NOT NULL,
    effect_date         DATE        NOT NULL,
    treatment_mean      DOUBLE      NOT NULL,
    control_mean        DOUBLE      NOT NULL,
    absolute_effect     DOUBLE      NOT NULL,
    sample_size         BIGINT      NOT NULL
)
USING DELTA
PARTITIONED BY (experiment_id STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.logRetentionDuration' = 'interval 365 days'
);
