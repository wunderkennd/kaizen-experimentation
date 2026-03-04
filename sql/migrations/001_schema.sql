-- ============================================================================
-- Experimentation Platform: PostgreSQL Schema
-- Covers M5 (config/lifecycle), M4a (results), M3 (query_log), audit trail
-- ============================================================================

-- Extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ============================================================================
-- SCHEMA: config (owned by M5 Experiment Management Service)
-- ============================================================================

CREATE TABLE experiments (
    experiment_id   UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name            TEXT NOT NULL,
    description     TEXT,
    owner_email     TEXT NOT NULL,
    type            TEXT NOT NULL CHECK (type IN (
        'AB', 'MULTIVARIATE', 'INTERLEAVING', 'SESSION_LEVEL',
        'PLAYBACK_QOE', 'MAB', 'CONTEXTUAL_BANDIT', 'CUMULATIVE_HOLDOUT'
    )),
    state           TEXT NOT NULL DEFAULT 'DRAFT' CHECK (state IN (
        'DRAFT', 'STARTING', 'RUNNING', 'CONCLUDING', 'CONCLUDED', 'ARCHIVED'
    )),
    layer_id            UUID NOT NULL REFERENCES layers(layer_id),
    primary_metric_id   TEXT NOT NULL,
    secondary_metric_ids TEXT[] DEFAULT '{}',
    guardrail_action    TEXT NOT NULL DEFAULT 'AUTO_PAUSE' CHECK (guardrail_action IN ('AUTO_PAUSE', 'ALERT_ONLY')),
    hash_salt           TEXT NOT NULL DEFAULT encode(gen_random_bytes(16), 'hex'),
    targeting_rule_id   UUID REFERENCES targeting_rules(rule_id),
    is_cumulative_holdout BOOLEAN NOT NULL DEFAULT FALSE,

    -- Type-specific config stored as JSONB. Schema validated by M5 at creation.
    -- Contains: interleaving_config, session_config, bandit_config, lifecycle_config, etc.
    type_config     JSONB NOT NULL DEFAULT '{}',

    -- Sequential testing config.
    sequential_method   TEXT CHECK (sequential_method IN ('MSPRT', 'GST_OBF', 'GST_POCOCK')),
    planned_looks       INT,
    overall_alpha       DOUBLE PRECISION DEFAULT 0.05,

    -- Surrogate model linkage.
    surrogate_model_id  UUID REFERENCES surrogate_models(model_id),

    -- Timestamps (M5 manages state transitions).
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at      TIMESTAMPTZ,
    concluded_at    TIMESTAMPTZ,
    archived_at     TIMESTAMPTZ,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_experiments_state ON experiments(state);
CREATE INDEX idx_experiments_owner ON experiments(owner_email);
CREATE INDEX idx_experiments_layer ON experiments(layer_id);
CREATE INDEX idx_experiments_type ON experiments(type);

CREATE TABLE variants (
    variant_id      UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    traffic_fraction DOUBLE PRECISION NOT NULL CHECK (traffic_fraction >= 0 AND traffic_fraction <= 1),
    is_control      BOOLEAN NOT NULL DEFAULT FALSE,
    payload_json    JSONB DEFAULT '{}',
    ordinal         INT NOT NULL DEFAULT 0
);

CREATE INDEX idx_variants_experiment ON variants(experiment_id);
-- Enforce exactly one control per experiment via partial unique index.
CREATE UNIQUE INDEX idx_variants_one_control ON variants(experiment_id) WHERE is_control = TRUE;

CREATE TABLE layers (
    layer_id        UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name            TEXT NOT NULL UNIQUE,
    description     TEXT,
    total_buckets   INT NOT NULL DEFAULT 10000,
    bucket_reuse_cooldown_seconds INT NOT NULL DEFAULT 86400, -- 24 hours
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE layer_allocations (
    allocation_id   UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    layer_id        UUID NOT NULL REFERENCES layers(layer_id),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    start_bucket    INT NOT NULL,
    end_bucket      INT NOT NULL,
    activated_at    TIMESTAMPTZ,
    released_at     TIMESTAMPTZ,
    reusable_after  TIMESTAMPTZ,
    CHECK (start_bucket >= 0 AND end_bucket >= start_bucket)
);

CREATE INDEX idx_allocations_layer ON layer_allocations(layer_id);
CREATE INDEX idx_allocations_reusable ON layer_allocations(reusable_after) WHERE released_at IS NOT NULL;

CREATE TABLE guardrail_configs (
    guardrail_id    UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id) ON DELETE CASCADE,
    metric_id       TEXT NOT NULL,
    threshold       DOUBLE PRECISION NOT NULL,
    consecutive_breaches_required INT NOT NULL DEFAULT 1
);

CREATE INDEX idx_guardrails_experiment ON guardrail_configs(experiment_id);

CREATE TABLE targeting_rules (
    rule_id         UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name            TEXT NOT NULL,
    -- Predicate tree stored as JSONB. Schema: {groups: [{predicates: [{attribute_key, operator, values}]}]}
    rule_definition JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE metric_definitions (
    metric_id       TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT,
    type            TEXT NOT NULL CHECK (type IN ('MEAN', 'PROPORTION', 'RATIO', 'COUNT', 'PERCENTILE', 'CUSTOM')),
    source_event_type       TEXT,
    numerator_event_type    TEXT,
    denominator_event_type  TEXT,
    percentile              DOUBLE PRECISION,
    custom_sql              TEXT,
    lower_is_better         BOOLEAN NOT NULL DEFAULT FALSE,
    is_qoe_metric           BOOLEAN NOT NULL DEFAULT FALSE,
    cuped_covariate_metric_id TEXT,
    minimum_detectable_effect DOUBLE PRECISION,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE surrogate_models (
    model_id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    target_metric_id    TEXT NOT NULL,
    input_metric_ids    TEXT[] NOT NULL,
    observation_window_days INT NOT NULL,
    prediction_horizon_days INT NOT NULL,
    model_type          TEXT NOT NULL CHECK (model_type IN ('LINEAR', 'GRADIENT_BOOSTED', 'NEURAL')),
    calibration_r_squared   DOUBLE PRECISION,
    mlflow_model_uri    TEXT,
    last_calibrated_at  TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- SCHEMA: results (written by M4a Analysis Engine, read by M6 UI)
-- ============================================================================

CREATE TABLE analysis_results (
    result_id       UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    -- Full analysis result as JSONB (mirrors AnalysisResult proto).
    result_data     JSONB NOT NULL,
    -- SRM check.
    srm_p_value     DOUBLE PRECISION,
    srm_is_mismatch BOOLEAN,
    -- Cochran Q for lifecycle heterogeneity.
    cochran_q_p_value DOUBLE PRECISION,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_analysis_experiment ON analysis_results(experiment_id, computed_at DESC);

CREATE TABLE metric_results (
    result_id       UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    metric_id       TEXT NOT NULL,
    variant_id      UUID NOT NULL REFERENCES variants(variant_id),
    control_mean    DOUBLE PRECISION,
    treatment_mean  DOUBLE PRECISION,
    absolute_effect DOUBLE PRECISION,
    relative_effect DOUBLE PRECISION,
    ci_lower        DOUBLE PRECISION,
    ci_upper        DOUBLE PRECISION,
    p_value         DOUBLE PRECISION,
    is_significant  BOOLEAN,
    -- CUPED-adjusted.
    cuped_adjusted_effect   DOUBLE PRECISION,
    cuped_ci_lower          DOUBLE PRECISION,
    cuped_ci_upper          DOUBLE PRECISION,
    variance_reduction_pct  DOUBLE PRECISION,
    -- Sequential testing fields.
    boundary_crossed    BOOLEAN,
    alpha_spent         DOUBLE PRECISION,
    current_look        INT,
    adjusted_p_value    DOUBLE PRECISION,
    -- Session-level clustering.
    naive_se            DOUBLE PRECISION,
    clustered_se        DOUBLE PRECISION,
    design_effect       DOUBLE PRECISION,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_metric_results_experiment ON metric_results(experiment_id, computed_at DESC);
CREATE INDEX idx_metric_results_metric ON metric_results(metric_id, experiment_id);

CREATE TABLE surrogate_projections (
    projection_id   UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    variant_id      UUID NOT NULL REFERENCES variants(variant_id),
    model_id        UUID NOT NULL REFERENCES surrogate_models(model_id),
    projected_effect        DOUBLE PRECISION,
    projection_ci_lower     DOUBLE PRECISION,
    projection_ci_upper     DOUBLE PRECISION,
    calibration_r_squared   DOUBLE PRECISION,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_surr_projections_experiment ON surrogate_projections(experiment_id);

CREATE TABLE novelty_analysis_results (
    result_id       UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    metric_id       TEXT NOT NULL,
    novelty_detected    BOOLEAN NOT NULL,
    raw_treatment_effect    DOUBLE PRECISION,
    projected_steady_state  DOUBLE PRECISION,
    novelty_amplitude       DOUBLE PRECISION,
    decay_constant_days     DOUBLE PRECISION,
    is_stabilized           BOOLEAN,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE interference_analysis_results (
    result_id       UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    interference_detected   BOOLEAN NOT NULL,
    js_divergence           DOUBLE PRECISION,
    jaccard_similarity      DOUBLE PRECISION,
    treatment_gini          DOUBLE PRECISION,
    control_gini            DOUBLE PRECISION,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- SCHEMA: query_log (written by M3 Metric Engine, read by M6 UI)
-- ============================================================================

CREATE TABLE query_log (
    log_id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    metric_id       TEXT,
    -- Full Spark SQL text for reproducibility.
    sql_text        TEXT NOT NULL,
    row_count       BIGINT,
    duration_ms     BIGINT,
    -- Job type: 'daily_metric', 'hourly_guardrail', 'interleaving_score',
    -- 'surrogate_computation', 'lifecycle_segmentation', 'qoe_metric'.
    job_type        TEXT NOT NULL,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_query_log_experiment ON query_log(experiment_id, computed_at DESC);
CREATE INDEX idx_query_log_metric ON query_log(experiment_id, metric_id);

-- ============================================================================
-- SCHEMA: audit (written by M5 on every state transition and config change)
-- ============================================================================

CREATE TABLE audit_trail (
    audit_id        UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    -- Action: 'create', 'start', 'pause', 'resume', 'conclude', 'archive',
    -- 'guardrail_auto_pause', 'guardrail_override', 'config_update'.
    action          TEXT NOT NULL,
    actor_email     TEXT NOT NULL,
    -- Previous and new state for state transitions.
    previous_state  TEXT,
    new_state       TEXT,
    -- Details of the change (e.g., which guardrail breached, what config changed).
    details_json    JSONB DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_experiment ON audit_trail(experiment_id, created_at DESC);
CREATE INDEX idx_audit_action ON audit_trail(action, created_at DESC);

-- ============================================================================
-- SCHEMA: bandit (policy snapshots written by M4b for long-term audit)
-- Note: RocksDB is primary policy store (crash-only). PostgreSQL is archive.
-- ============================================================================

CREATE TABLE policy_snapshots (
    snapshot_id     UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    experiment_id   UUID NOT NULL REFERENCES experiments(experiment_id),
    policy_state    BYTEA NOT NULL,
    total_rewards_processed BIGINT NOT NULL,
    kafka_offset    BIGINT NOT NULL,
    snapshot_at     TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_policy_snapshots_experiment ON policy_snapshots(experiment_id, snapshot_at DESC);
