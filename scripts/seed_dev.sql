-- ==============================================================================
-- Development Seed Data
-- Run via: make seed
-- Idempotent: Uses ON CONFLICT DO NOTHING where possible.
-- ==============================================================================

BEGIN;

-- ---------------------------------------------------------------------------
-- Layers
-- ---------------------------------------------------------------------------
INSERT INTO layers (layer_id, name, description, total_buckets)
VALUES
    ('a0000000-0000-0000-0000-000000000001', 'default', 'Default traffic layer for general A/B tests', 10000),
    ('a0000000-0000-0000-0000-000000000002', 'recommendations', 'Recommendation algorithm experiments', 10000),
    ('a0000000-0000-0000-0000-000000000003', 'playback', 'Playback and streaming quality experiments', 10000)
ON CONFLICT (name) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Targeting Rules
-- ---------------------------------------------------------------------------
INSERT INTO targeting_rules (rule_id, name, rule_definition)
VALUES
    ('b0000000-0000-0000-0000-000000000001', 'all_users',
     '{"groups": [{"predicates": []}]}'::jsonb),
    ('b0000000-0000-0000-0000-000000000002', 'premium_subscribers',
     '{"groups": [{"predicates": [{"attribute_key": "subscription_tier", "operator": "IN", "values": ["premium", "family"]}]}]}'::jsonb),
    ('b0000000-0000-0000-0000-000000000003', 'us_only',
     '{"groups": [{"predicates": [{"attribute_key": "country", "operator": "EQUALS", "values": ["US"]}]}]}'::jsonb)
ON CONFLICT (rule_id) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Metric Definitions
-- ---------------------------------------------------------------------------
INSERT INTO metric_definitions (metric_id, name, description, type, source_event_type)
VALUES
    ('stream_start_rate',     'Stream Start Rate',          'Proportion of sessions that start a stream',                    'PROPORTION', 'stream_start'),
    ('watch_time_minutes',    'Watch Time (minutes)',        'Average minutes watched per user per day',                      'MEAN',       'heartbeat'),
    ('completion_rate',       'Content Completion Rate',     'Proportion of content items watched to ≥90%',                   'PROPORTION', 'stream_end'),
    ('rebuffer_rate',         'Rebuffer Rate',               'Rebuffer events per hour of playback',                          'RATIO',      'qoe_rebuffer'),
    ('search_success_rate',   'Search Success Rate',         'Proportion of searches that lead to a stream start within 5m',  'PROPORTION', 'search'),
    ('ctr_recommendation',    'Recommendation CTR',          'Click-through rate on recommendation carousels',                'PROPORTION', 'impression'),
    ('revenue_per_user',      'Revenue per User',            'Average daily revenue per user (ads + subscription)',            'MEAN',       'revenue'),
    ('churn_7d',              'Churn (7-day)',               'Proportion of users with zero activity in trailing 7 days',     'PROPORTION', 'session'),
    ('latency_p50_ms',        'Playback Start Latency p50',  'Median time-to-first-frame in milliseconds',                   'PERCENTILE', 'playback_start'),
    ('error_rate',            'Error Rate',                  'Proportion of playback attempts that result in an error',       'PROPORTION', 'playback_error')
ON CONFLICT (metric_id) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Experiment 1: Homepage Recommendation A/B Test (RUNNING)
-- ---------------------------------------------------------------------------
INSERT INTO experiments (
    experiment_id, name, description, owner_email, type, state,
    layer_id, primary_metric_id, secondary_metric_ids,
    guardrail_action, targeting_rule_id,
    sequential_method, planned_looks, overall_alpha,
    started_at
)
VALUES (
    'e0000000-0000-0000-0000-000000000001',
    'homepage_recs_v2',
    'Test new collaborative filtering model for homepage recommendations',
    'data-science@example.com',
    'AB',
    'RUNNING',
    'a0000000-0000-0000-0000-000000000002',
    'ctr_recommendation',
    ARRAY['watch_time_minutes', 'stream_start_rate'],
    'AUTO_PAUSE',
    'b0000000-0000-0000-0000-000000000001',
    'MSPRT',
    5,
    0.05,
    NOW() - INTERVAL '3 days'
)
ON CONFLICT (experiment_id) DO NOTHING;

INSERT INTO variants (variant_id, experiment_id, name, traffic_fraction, is_control, ordinal)
VALUES
    ('f0000000-0000-0000-0000-000000000001', 'e0000000-0000-0000-0000-000000000001', 'control',           0.5, TRUE,  0),
    ('f0000000-0000-0000-0000-000000000002', 'e0000000-0000-0000-0000-000000000001', 'collab_filter_v2',  0.5, FALSE, 1)
ON CONFLICT (variant_id) DO NOTHING;

INSERT INTO layer_allocations (allocation_id, layer_id, experiment_id, start_bucket, end_bucket, activated_at)
VALUES (
    'c0000000-0000-0000-0000-000000000001',
    'a0000000-0000-0000-0000-000000000002',
    'e0000000-0000-0000-0000-000000000001',
    0, 9999,
    NOW() - INTERVAL '3 days'
)
ON CONFLICT (allocation_id) DO NOTHING;

INSERT INTO guardrail_configs (guardrail_id, experiment_id, metric_id, threshold, consecutive_breaches_required)
VALUES
    ('d0000000-0000-0000-0000-000000000001', 'e0000000-0000-0000-0000-000000000001', 'error_rate',    0.05, 2),
    ('d0000000-0000-0000-0000-000000000002', 'e0000000-0000-0000-0000-000000000001', 'rebuffer_rate',  3.0, 3)
ON CONFLICT (guardrail_id) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Experiment 2: Playback QoE Test (DRAFT)
-- ---------------------------------------------------------------------------
INSERT INTO experiments (
    experiment_id, name, description, owner_email, type, state,
    layer_id, primary_metric_id, secondary_metric_ids,
    guardrail_action, targeting_rule_id,
    type_config
)
VALUES (
    'e0000000-0000-0000-0000-000000000002',
    'adaptive_bitrate_v3',
    'Test new ABR algorithm for reduced rebuffering on mobile',
    'streaming-eng@example.com',
    'PLAYBACK_QOE',
    'DRAFT',
    'a0000000-0000-0000-0000-000000000003',
    'rebuffer_rate',
    ARRAY['latency_p50_ms', 'watch_time_minutes'],
    'AUTO_PAUSE',
    'b0000000-0000-0000-0000-000000000002',
    '{"qoe_metrics": ["rebuffer_ratio", "time_to_first_frame", "resolution_switches"], "device_filter": "mobile"}'::jsonb
)
ON CONFLICT (experiment_id) DO NOTHING;

INSERT INTO variants (variant_id, experiment_id, name, traffic_fraction, is_control, ordinal)
VALUES
    ('f0000000-0000-0000-0000-000000000003', 'e0000000-0000-0000-0000-000000000002', 'control',    0.5, TRUE,  0),
    ('f0000000-0000-0000-0000-000000000004', 'e0000000-0000-0000-0000-000000000002', 'abr_v3',     0.5, FALSE, 1)
ON CONFLICT (variant_id) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Experiment 3: Interleaving Test (RUNNING)
-- ---------------------------------------------------------------------------
INSERT INTO experiments (
    experiment_id, name, description, owner_email, type, state,
    layer_id, primary_metric_id, secondary_metric_ids,
    guardrail_action, targeting_rule_id,
    type_config,
    started_at
)
VALUES (
    'e0000000-0000-0000-0000-000000000003',
    'search_ranking_interleave',
    'Team-draft interleaving of current vs. neural search ranking',
    'search-team@example.com',
    'INTERLEAVING',
    'RUNNING',
    'a0000000-0000-0000-0000-000000000001',
    'search_success_rate',
    ARRAY['ctr_recommendation'],
    'ALERT_ONLY',
    'b0000000-0000-0000-0000-000000000003',
    '{"interleaving_method": "TEAM_DRAFT", "list_length": 20}'::jsonb,
    NOW() - INTERVAL '7 days'
)
ON CONFLICT (experiment_id) DO NOTHING;

INSERT INTO variants (variant_id, experiment_id, name, traffic_fraction, is_control, ordinal)
VALUES
    ('f0000000-0000-0000-0000-000000000005', 'e0000000-0000-0000-0000-000000000003', 'current_ranking', 0.5, TRUE,  0),
    ('f0000000-0000-0000-0000-000000000006', 'e0000000-0000-0000-0000-000000000003', 'neural_ranking',  0.5, FALSE, 1)
ON CONFLICT (variant_id) DO NOTHING;

INSERT INTO layer_allocations (allocation_id, layer_id, experiment_id, start_bucket, end_bucket, activated_at)
VALUES (
    'c0000000-0000-0000-0000-000000000003',
    'a0000000-0000-0000-0000-000000000001',
    'e0000000-0000-0000-0000-000000000003',
    0, 4999,
    NOW() - INTERVAL '7 days'
)
ON CONFLICT (allocation_id) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Experiment 4: Contextual Bandit (DRAFT)
-- ---------------------------------------------------------------------------
INSERT INTO experiments (
    experiment_id, name, description, owner_email, type, state,
    layer_id, primary_metric_id,
    guardrail_action, targeting_rule_id,
    type_config
)
VALUES (
    'e0000000-0000-0000-0000-000000000004',
    'cold_start_bandit',
    'Contextual bandit for new content cold-start promotion',
    'content-team@example.com',
    'CONTEXTUAL_BANDIT',
    'DRAFT',
    'a0000000-0000-0000-0000-000000000002',
    'watch_time_minutes',
    'AUTO_PAUSE',
    'b0000000-0000-0000-0000-000000000001',
    '{"bandit_algorithm": "THOMPSON_SAMPLING", "context_features": ["genre", "release_recency", "user_segment"], "update_interval_seconds": 300}'::jsonb
)
ON CONFLICT (experiment_id) DO NOTHING;

INSERT INTO variants (variant_id, experiment_id, name, traffic_fraction, is_control, ordinal)
VALUES
    ('f0000000-0000-0000-0000-000000000007', 'e0000000-0000-0000-0000-000000000004', 'explore',  0.2, FALSE, 0),
    ('f0000000-0000-0000-0000-000000000008', 'e0000000-0000-0000-0000-000000000004', 'exploit',  0.8, FALSE, 1)
ON CONFLICT (variant_id) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Sample Audit Trail Entries
-- ---------------------------------------------------------------------------
INSERT INTO audit_trail (experiment_id, action, actor, details)
VALUES
    ('e0000000-0000-0000-0000-000000000001', 'CREATED',  'data-science@example.com',  '{"source": "api"}'),
    ('e0000000-0000-0000-0000-000000000001', 'STARTED',  'data-science@example.com',  '{"allocation": "0-9999", "layer": "recommendations"}'),
    ('e0000000-0000-0000-0000-000000000003', 'CREATED',  'search-team@example.com',   '{"source": "api"}'),
    ('e0000000-0000-0000-0000-000000000003', 'STARTED',  'search-team@example.com',   '{"allocation": "0-4999", "layer": "default"}');

COMMIT;

-- ---------------------------------------------------------------------------
-- Summary
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    RAISE NOTICE '=== Seed Data Summary ===';
    RAISE NOTICE 'Layers: %',            (SELECT COUNT(*) FROM layers);
    RAISE NOTICE 'Targeting Rules: %',   (SELECT COUNT(*) FROM targeting_rules);
    RAISE NOTICE 'Metric Definitions: %',(SELECT COUNT(*) FROM metric_definitions);
    RAISE NOTICE 'Experiments: %',       (SELECT COUNT(*) FROM experiments);
    RAISE NOTICE 'Variants: %',          (SELECT COUNT(*) FROM variants);
    RAISE NOTICE 'Guardrail Configs: %', (SELECT COUNT(*) FROM guardrail_configs);
    RAISE NOTICE 'Audit Trail: %',       (SELECT COUNT(*) FROM audit_trail);
END $$;
