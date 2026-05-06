WITH operand_rows AS (
    SELECT user_id, variant_id, metric_id, metric_value
    FROM delta.metric_summaries
    WHERE experiment_id = 'exp-001'
      AND computation_date = CAST('2024-01-15' AS DATE)
      AND metric_id IN ('m1', 'm2', 'm3')
),
pivoted AS (
    SELECT
        user_id,
        variant_id,
        MAX(CASE WHEN metric_id = 'm1' THEN metric_value END) AS m0,
        MAX(CASE WHEN metric_id = 'm2' THEN metric_value END) AS m1,
        MAX(CASE WHEN metric_id = 'm3' THEN metric_value END) AS m2
    FROM operand_rows
    GROUP BY user_id, variant_id
)
SELECT
    'exp-001' AS experiment_id,
    user_id,
    variant_id,
    'composite_metric' AS metric_id,
    ((0.5 * m0) + (0.3 * m1) + (0.2 * m2)) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date,
    1.0 AS assignment_probability
FROM pivoted;
