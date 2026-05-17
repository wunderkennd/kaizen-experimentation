-- Minimal seed data required by integration tests.
-- Mounted in docker-compose.test.yml as a second initdb script.

INSERT INTO layers (layer_id, name, description, total_buckets)
VALUES ('a0000000-0000-0000-0000-000000000001', 'default', 'Default traffic layer for general A/B tests', 10000)
ON CONFLICT (name) DO NOTHING;

INSERT INTO metric_definitions (metric_id, name, type, source_event_type, stakeholder, aggregation_level)
VALUES
    ('watch_time_minutes', 'Watch Time (minutes)', 'MEAN', 'heartbeat', 'USER', 'USER'),
    ('metric-1', 'Test Metric 1', 'MEAN', 'test_event', 'USER', 'USER')
ON CONFLICT (metric_id) DO NOTHING;

-- ADR-026 Phase 1 (#433): seed one row per new structured metric type so the
-- migration's CHECK constraint and JSONB type_config persistence are exercised
-- end-to-end in integration tests. type_config payloads mirror the proto messages
-- (FilteredMeanConfig, CompositeConfig, WindowedCountConfig).
INSERT INTO metric_definitions (metric_id, name, type, source_event_type, stakeholder, aggregation_level, type_config)
VALUES
    (
        'mobile_watch_time_filtered',
        'Mobile Watch Time (FILTERED_MEAN)',
        'FILTERED_MEAN',
        'heartbeat',
        'USER',
        'USER',
        '{"filter_sql": "platform = ''mobile'' AND duration_ms > 5000", "value_column": "duration_ms"}'::jsonb
    ),
    (
        'engagement_composite',
        'Engagement Composite (COMPOSITE)',
        'COMPOSITE',
        NULL,
        'USER',
        'USER',
        '{"operator": "COMPOSITE_OPERATOR_WEIGHTED_SUM", "operands": [{"metric_id": "watch_time_minutes", "weight": 0.7}, {"metric_id": "metric-1", "weight": 0.3}]}'::jsonb
    ),
    (
        'signup_24h_count',
        'Signups within 24h (WINDOWED_COUNT)',
        'WINDOWED_COUNT',
        NULL,
        'USER',
        'USER',
        '{"event_type": "signup_completed", "filter_sql": "", "window_hours": 24}'::jsonb
    )
ON CONFLICT (metric_id) DO NOTHING;
