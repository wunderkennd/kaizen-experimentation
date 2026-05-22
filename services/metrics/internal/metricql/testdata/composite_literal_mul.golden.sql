WITH operand_rows AS (
    SELECT user_id, variant_id, metric_id, metric_value
    FROM delta.metric_summaries
    WHERE experiment_id = 'exp_test'
      AND computation_date = CAST('2026-05-18' AS DATE)
      AND metric_id IN ('watch_time')
),
pivoted AS (
    SELECT
        user_id,
        variant_id,
        MAX(CASE WHEN metric_id = 'watch_time' THEN metric_value END) AS m0
    FROM operand_rows
    GROUP BY user_id, variant_id
)
SELECT
    'exp_test' AS experiment_id,
    user_id,
    variant_id,
    'm_lit_mul' AS metric_id,
    ((m0 * 2.5)) AS metric_value,
    CAST('2026-05-18' AS DATE) AS computation_date,
    1.0 AS assignment_probability
FROM pivoted;
