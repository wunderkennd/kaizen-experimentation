-- ============================================================================
-- Feature Flags: M7 Feature Flag Service tables
-- ============================================================================

CREATE TABLE feature_flags (
    flag_id             UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name                TEXT NOT NULL UNIQUE,
    description         TEXT NOT NULL DEFAULT '',
    type                TEXT NOT NULL DEFAULT 'BOOLEAN'
                        CHECK (type IN ('BOOLEAN', 'STRING', 'NUMERIC', 'JSON')),
    default_value       TEXT NOT NULL DEFAULT 'false',
    enabled             BOOLEAN NOT NULL DEFAULT FALSE,
    rollout_percentage  DOUBLE PRECISION NOT NULL DEFAULT 0.0
                        CHECK (rollout_percentage >= 0.0 AND rollout_percentage <= 1.0),
    salt                TEXT NOT NULL DEFAULT encode(gen_random_bytes(16), 'hex'),
    targeting_rule_id   UUID REFERENCES targeting_rules(rule_id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_feature_flags_name ON feature_flags(name);
CREATE INDEX idx_feature_flags_enabled ON feature_flags(enabled);

CREATE TABLE flag_variants (
    variant_id       UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    flag_id          UUID NOT NULL REFERENCES feature_flags(flag_id) ON DELETE CASCADE,
    value            TEXT NOT NULL,
    traffic_fraction DOUBLE PRECISION NOT NULL
                     CHECK (traffic_fraction >= 0.0 AND traffic_fraction <= 1.0),
    ordinal          INT NOT NULL DEFAULT 0
);

CREATE INDEX idx_flag_variants_flag ON flag_variants(flag_id);
