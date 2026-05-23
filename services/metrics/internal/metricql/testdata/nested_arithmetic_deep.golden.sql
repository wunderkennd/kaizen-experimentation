WITH operand_rows AS (
    SELECT user_id, variant_id, metric_id, metric_value
    FROM delta.metric_summaries
    WHERE experiment_id = 'exp_test'
      AND computation_date = CAST('2026-05-18' AS DATE)
      AND metric_id IN ('a', 'b', 'c')
),
pivoted AS (
    SELECT
        user_id,
        variant_id,
        MAX(CASE WHEN metric_id = 'a' THEN metric_value END) AS m0,
        MAX(CASE WHEN metric_id = 'b' THEN metric_value END) AS m1,
        MAX(CASE WHEN metric_id = 'c' THEN metric_value END) AS m2
    FROM operand_rows
    GROUP BY user_id, variant_id
)
SELECT
    'exp_test' AS experiment_id,
    user_id,
    variant_id,
    'm_nested_deep' AS metric_id,
    ((((m0 + m1) * m2) / NULLIF(m2, 0))) AS metric_value,
    CAST('2026-05-18' AS DATE) AS computation_date,
    1.0 AS assignment_probability
FROM pivoted;
