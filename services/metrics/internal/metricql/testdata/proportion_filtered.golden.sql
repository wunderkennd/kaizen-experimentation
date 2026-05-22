WITH exposed_users AS (
    SELECT user_id, variant_id,
           MIN(event_timestamp) AS exposure_ts,
           MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp_test'
    GROUP BY user_id, variant_id
),
event_rows AS (
    SELECT eu.user_id, eu.variant_id
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'stream_start'
      AND (platform = 'mobile')
)
SELECT
    'exp_test' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    'm_proportion_filtered' AS metric_id,
    CASE WHEN COUNT(er.user_id) > 0 THEN 1.0 ELSE 0.0 END AS metric_value,
    CAST('2026-05-18' AS DATE) AS computation_date,
    eu.assignment_probability
FROM exposed_users eu
LEFT JOIN event_rows er ON eu.user_id = er.user_id AND eu.variant_id = er.variant_id
GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability;
