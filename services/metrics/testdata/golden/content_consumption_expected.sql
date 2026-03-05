WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
content_events AS (
    SELECT me.user_id, eu.variant_id, me.content_id AS content_id, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.content_id IS NOT NULL
)
SELECT
    'exp-001' AS experiment_id,
    content_events.variant_id,
    content_events.content_id,
    SUM(content_events.value) AS watch_time_seconds,
    COUNT(*) AS view_count,
    COUNT(DISTINCT content_events.user_id) AS unique_viewers,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM content_events
GROUP BY content_events.variant_id, content_events.content_id