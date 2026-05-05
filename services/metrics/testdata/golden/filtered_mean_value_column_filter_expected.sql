WITH exposed_users AS (
    SELECT user_id, variant_id, MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
    GROUP BY user_id, variant_id
),
filtered_data AS (
    SELECT me.user_id, eu.variant_id, me.duration_ms AS value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'heartbeat'
      AND (duration_ms > 1000)
)
SELECT
    'exp-001' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    'long_watches_only_avg' AS metric_id,
    AVG(fd.value) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date,
    eu.assignment_probability
FROM exposed_users eu
LEFT JOIN filtered_data fd ON eu.user_id = fd.user_id AND eu.variant_id = fd.variant_id
GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability;
