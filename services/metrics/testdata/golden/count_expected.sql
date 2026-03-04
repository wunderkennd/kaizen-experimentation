WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
metric_data AS (
    SELECT me.user_id, eu.variant_id, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'stream_start'
)
SELECT
    'exp-001' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    'stream_start_count' AS metric_id,
    CAST(COUNT(md.value) AS DOUBLE) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM exposed_users eu
LEFT JOIN metric_data md ON eu.user_id = md.user_id AND eu.variant_id = md.variant_id
GROUP BY eu.user_id, eu.variant_id