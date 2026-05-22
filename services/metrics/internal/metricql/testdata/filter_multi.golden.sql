WITH exposed_users AS (
    SELECT user_id, variant_id,
           MIN(event_timestamp) AS exposure_ts,
           MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp_test'
    GROUP BY user_id, variant_id
),
event_rows AS (
    SELECT eu.user_id, eu.variant_id, me.amount AS value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'purchase'
      AND (country = 'us' AND tier != 'free')
)
SELECT
    'exp_test' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    'm_filter_multi' AS metric_id,
    COALESCE(SUM(er.value), 0.0) AS metric_value,
    CAST('2026-05-18' AS DATE) AS computation_date,
    eu.assignment_probability
FROM exposed_users eu
LEFT JOIN event_rows er ON eu.user_id = er.user_id AND eu.variant_id = er.variant_id
GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability;
