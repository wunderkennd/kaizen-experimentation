WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
pre_experiment_data AS (
    SELECT me.user_id, me.value
    FROM delta.metric_events me
    WHERE me.event_type = 'heartbeat'
      AND me.event_date >= DATE_SUB(CAST('2024-01-08' AS DATE), 7)
      AND me.event_date < CAST('2024-01-08' AS DATE)
)
SELECT
    'exp-001' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    'watch_time_minutes' AS metric_id,
    COALESCE(AVG(ped.value), 0.0) AS cuped_covariate,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM exposed_users eu
LEFT JOIN pre_experiment_data ped ON eu.user_id = ped.user_id
GROUP BY eu.user_id, eu.variant_id