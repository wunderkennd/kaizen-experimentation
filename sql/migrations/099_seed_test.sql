-- Minimal seed data required by integration tests.
-- Mounted in docker-compose.test.yml as a second initdb script.

INSERT INTO layers (layer_id, name, description, total_buckets)
VALUES ('a0000000-0000-0000-0000-000000000001', 'default', 'Default traffic layer for general A/B tests', 10000)
ON CONFLICT (name) DO NOTHING;

INSERT INTO metric_definitions (metric_id, name, type, source_event_type)
VALUES
    ('watch_time_minutes', 'Watch Time (minutes)', 'MEAN', 'heartbeat'),
    ('metric-1', 'Test Metric 1', 'MEAN', 'test_event')
ON CONFLICT (metric_id) DO NOTHING;
