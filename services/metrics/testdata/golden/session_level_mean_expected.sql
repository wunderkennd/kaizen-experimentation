WITH exposed_users AS (
    SELECT user_id, variant_id, session_id, MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
      AND session_id IS NOT NULL
    GROUP BY user_id, variant_id, session_id
),
metric_data AS (
    SELECT me.user_id, me.session_id, eu.variant_id, eu.assignment_probability, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id AND me.session_id = eu.session_id
    WHERE me.event_type = 'heartbeat'
)
SELECT
    'exp-001' AS experiment_id,
    metric_data.user_id,
    metric_data.session_id,
    metric_data.variant_id,
    'watch_time_minutes' AS metric_id,
    AVG(metric_data.value) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date,
    MIN(metric_data.assignment_probability) AS assignment_probability
FROM metric_data
GROUP BY metric_data.user_id, metric_data.session_id, metric_data.variant_id
