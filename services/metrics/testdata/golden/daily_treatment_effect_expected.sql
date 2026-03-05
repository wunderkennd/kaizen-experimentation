WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
metric_values AS (
    SELECT ms.user_id, ms.variant_id, ms.metric_value, ms.computation_date
    FROM delta.metric_summaries ms
    WHERE ms.experiment_id = 'exp-001'
      AND ms.metric_id = 'watch_time_minutes'
),
treatment_stats AS (
    SELECT
        mv.computation_date AS effect_date,
        AVG(CASE WHEN mv.variant_id = 'ctrl-001' THEN mv.metric_value END) AS control_mean,
        AVG(CASE WHEN mv.variant_id != 'ctrl-001' THEN mv.metric_value END) AS treatment_mean,
        COUNT(*) AS sample_size
    FROM metric_values mv
    GROUP BY mv.computation_date
)
SELECT
    'exp-001' AS experiment_id,
    'watch_time_minutes' AS metric_id,
    treatment_stats.effect_date,
    treatment_stats.treatment_mean,
    treatment_stats.control_mean,
    treatment_stats.treatment_mean - treatment_stats.control_mean AS absolute_effect,
    treatment_stats.sample_size
FROM treatment_stats
ORDER BY treatment_stats.effect_date