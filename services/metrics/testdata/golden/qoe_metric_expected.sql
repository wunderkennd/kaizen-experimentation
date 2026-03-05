WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
qoe_data AS (
    SELECT qe.user_id, eu.variant_id, qe.time_to_first_frame_ms AS value
    FROM delta.qoe_events qe
    INNER JOIN exposed_users eu ON qe.user_id = eu.user_id
)
SELECT
    'exp-001' AS experiment_id,
    qoe_data.user_id,
    qoe_data.variant_id,
    'ttff_mean' AS metric_id,
    AVG(qoe_data.value) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM qoe_data
GROUP BY qoe_data.user_id, qoe_data.variant_id