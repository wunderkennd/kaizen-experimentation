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
    'm_composite_with_ratio' AS metric_id,
    (((0.5 * m0) + (0.5 * (CASE WHEN m2 = 0.0 THEN 0.0 ELSE m1 / m2 END)))) AS metric_value,
    CAST('2026-05-18' AS DATE) AS computation_date,
    1.0 AS assignment_probability
FROM pivoted;
