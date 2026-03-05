WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
metric_data AS (
    SELECT me.user_id, eu.variant_id, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'qoe_rebuffer'
)
SELECT
    'exp-001' AS experiment_id,
    eu.variant_id,
    'rebuffer_rate' AS metric_id,
    AVG(metric_data.value) AS current_value,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM exposed_users eu
LEFT JOIN metric_data ON eu.user_id = metric_data.user_id AND eu.variant_id = metric_data.variant_id
GROUP BY eu.variant_id