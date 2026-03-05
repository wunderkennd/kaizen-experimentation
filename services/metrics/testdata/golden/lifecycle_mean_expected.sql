WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id, lifecycle_segment
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
metric_data AS (
    SELECT me.user_id, eu.variant_id, eu.lifecycle_segment, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'heartbeat'
)
SELECT
    'exp-001' AS experiment_id,
    metric_data.user_id,
    metric_data.variant_id,
    metric_data.lifecycle_segment,
    'watch_time_minutes' AS metric_id,
    AVG(metric_data.value) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM metric_data
GROUP BY metric_data.user_id, metric_data.variant_id, metric_data.lifecycle_segment