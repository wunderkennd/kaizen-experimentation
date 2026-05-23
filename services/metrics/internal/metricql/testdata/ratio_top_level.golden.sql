WITH operand_rows AS (
    SELECT user_id, variant_id, metric_id, metric_value
    FROM delta.metric_summaries
    WHERE experiment_id = 'exp_test'
      AND computation_date = CAST('2026-05-18' AS DATE)
      AND metric_id IN ('total_revenue', 'total_sessions')
),
pivoted AS (
    SELECT
        user_id,
        variant_id,
        MAX(CASE WHEN metric_id = 'total_revenue'   THEN metric_value END) AS numerator,
        MAX(CASE WHEN metric_id = 'total_sessions' THEN metric_value END) AS denominator
    FROM operand_rows
    GROUP BY user_id, variant_id
)
SELECT
    'exp_test' AS experiment_id,
    user_id,
    variant_id,
    'm_ratio' AS metric_id,
    CASE WHEN denominator = 0.0 THEN 0.0 ELSE numerator / denominator END AS metric_value,
    CAST('2026-05-18' AS DATE) AS computation_date,
    1.0 AS assignment_probability
FROM pivoted;
