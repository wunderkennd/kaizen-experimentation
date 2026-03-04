WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
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
        COALESCE(SUM(n.value), 0.0) AS numerator_sum,
        COALESCE(SUM(d.value), 0.0) AS denominator_sum
    FROM exposed_users eu
    LEFT JOIN numerator_data n ON eu.user_id = n.user_id AND eu.variant_id = n.variant_id
    LEFT JOIN denominator_data d ON eu.user_id = d.user_id AND eu.variant_id = d.variant_id
    GROUP BY eu.user_id, eu.variant_id
)
SELECT
    'exp-001' AS experiment_id,
    per_user.variant_id,
    'rebuffer_rate' AS metric_id,
    COUNT(*) AS user_count,
    AVG(per_user.numerator_sum) AS mean_numerator,
    AVG(per_user.denominator_sum) AS mean_denominator,
    VAR_SAMP(per_user.numerator_sum) AS var_numerator,
    VAR_SAMP(per_user.denominator_sum) AS var_denominator,
    COVAR_SAMP(per_user.numerator_sum, per_user.denominator_sum) AS cov_numerator_denominator,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM per_user
GROUP BY per_user.variant_id