WITH exposed_users AS (
    SELECT user_id, variant_id, MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
    GROUP BY user_id, variant_id
),
qoe_sessions AS (
    SELECT qe.user_id, eu.variant_id, eu.assignment_probability, qe.ebvs_detected
    FROM delta.qoe_events qe
    INNER JOIN exposed_users eu ON qe.user_id = eu.user_id
)
SELECT
    'exp-001' AS experiment_id,
    qoe_sessions.user_id,
    qoe_sessions.variant_id,
    'ebvs_rate' AS metric_id,
    CAST(SUM(CASE WHEN qoe_sessions.ebvs_detected THEN 1 ELSE 0 END) AS DOUBLE)
        / NULLIF(COUNT(*), 0) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date,
    MIN(qoe_sessions.assignment_probability) AS assignment_probability
FROM qoe_sessions
GROUP BY qoe_sessions.user_id, qoe_sessions.variant_id
