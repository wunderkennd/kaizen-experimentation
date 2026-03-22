WITH exposed_users AS (
    SELECT user_id, variant_id, MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
    GROUP BY user_id, variant_id
),
numerator_data AS (
    SELECT me.user_id, eu.variant_id, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'rebuffer_event'
),
denominator_data AS (
    SELECT me.user_id, eu.variant_id, me.value
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'playback_minute'
),
per_user AS (
    SELECT
        eu.user_id,
        eu.variant_id,
        eu.assignment_probability,
        COALESCE(SUM(n.value), 0.0) AS numerator_sum,
        COALESCE(SUM(d.value), 0.0) AS denominator_sum
    FROM exposed_users eu
    LEFT JOIN numerator_data n ON eu.user_id = n.user_id AND eu.variant_id = n.variant_id
    LEFT JOIN denominator_data d ON eu.user_id = d.user_id AND eu.variant_id = d.variant_id
    GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability
)
SELECT
    'exp-001' AS experiment_id,
    per_user.user_id,
    per_user.variant_id,
    'rebuffer_rate' AS metric_id,
    CASE
        WHEN per_user.denominator_sum = 0.0 THEN 0.0
        ELSE per_user.numerator_sum / per_user.denominator_sum
    END AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date,
    per_user.assignment_probability
FROM per_user
