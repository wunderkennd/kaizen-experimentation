WITH operand_rows AS (
    SELECT user_id, variant_id, metric_id, metric_value
    FROM delta.metric_summaries
    WHERE experiment_id = 'exp_test'
      AND computation_date = CAST('2026-05-18' AS DATE)
      AND metric_id IN ('a', 'b')
),
pivoted AS (
    SELECT
        user_id,
        variant_id,
        MAX(CASE WHEN metric_id = 'a' THEN metric_value END) AS m0,
        MAX(CASE WHEN metric_id = 'b' THEN metric_value END) AS m1
    FROM operand_rows
    GROUP BY user_id, variant_id
)
SELECT
    'exp_test' AS experiment_id,
    user_id,
    variant_id,
    'm_nested_parens' AS metric_id,
    ((((0.5 * m0) + (0.5 * m1)) * 2)) AS metric_value,
    CAST('2026-05-18' AS DATE) AS computation_date,
    1.0 AS assignment_probability
FROM pivoted;
