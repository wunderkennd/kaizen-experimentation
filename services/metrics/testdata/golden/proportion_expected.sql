WITH exposed_users AS (
    SELECT user_id, variant_id, MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
    GROUP BY user_id, variant_id
),
metric_data AS (
    SELECT me.user_id, eu.variant_id, eu.assignment_probability, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'impression'
)
SELECT
    'exp-001' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    'ctr_recommendation' AS metric_id,
    CASE WHEN COUNT(md.value) > 0 THEN 1.0 ELSE 0.0 END AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date,
    eu.assignment_probability
FROM exposed_users eu
LEFT JOIN metric_data md ON eu.user_id = md.user_id AND eu.variant_id = md.variant_id
GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability
