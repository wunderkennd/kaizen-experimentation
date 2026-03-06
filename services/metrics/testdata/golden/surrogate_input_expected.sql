SELECT
    ms.variant_id,
    ms.metric_id,
    AVG(ms.metric_value) AS avg_value,
    COUNT(DISTINCT ms.user_id) AS user_count
FROM delta.metric_summaries ms
WHERE ms.experiment_id = 'exp-001'
  AND ms.metric_id IN ('watch_time_minutes', 'stream_start_rate')
  AND ms.computation_date >= DATE_SUB(CAST('2024-01-15' AS DATE), 7)
GROUP BY ms.variant_id, ms.metric_id
ORDER BY ms.variant_id, ms.metric_id