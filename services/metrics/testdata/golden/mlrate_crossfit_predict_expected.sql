WITH fold_data AS (
    SELECT user_id, variant_id,
        feature_heartbeat,
        feature_stream_start,
        fold_id
    FROM delta.mlrate_features
    WHERE experiment_id = 'exp-001'
      AND computation_date = CAST('2024-01-15' AS DATE)
      AND fold_id = 2
)
SELECT
    'exp-001' AS experiment_id,
    fd.user_id,
    fd.variant_id,
    'watch_time_minutes' AS metric_id,
    ai_predict(
        'models:/mlrate-watch-time/fold_2',
        NAMED_STRUCT('heartbeat', fd.feature_heartbeat, 'stream_start', fd.feature_stream_start)
    ) AS mlrate_covariate,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM fold_data fd