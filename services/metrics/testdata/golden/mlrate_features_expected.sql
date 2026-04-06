WITH exposed_users AS (
    SELECT DISTINCT user_id, variant_id
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
),
pre_experiment_events AS (
    SELECT me.user_id, me.event_type, AVG(me.value) AS avg_value
    FROM delta.metric_events me
    WHERE me.event_type IN ('heartbeat', 'stream_start')
      AND me.event_date >= DATE_SUB(CAST('2024-01-08' AS DATE), 14)
      AND me.event_date < CAST('2024-01-08' AS DATE)
    GROUP BY me.user_id, me.event_type
),
user_features AS (
    SELECT
        eu.user_id,
        eu.variant_id,
        COALESCE(MAX(CASE WHEN pee.event_type = 'heartbeat' THEN pee.avg_value END), 0.0) AS feature_heartbeat,
        COALESCE(MAX(CASE WHEN pee.event_type = 'stream_start' THEN pee.avg_value END), 0.0) AS feature_stream_start,
        CAST(ABS(HASH(eu.user_id)) % 5 + 1 AS INT) AS fold_id
    FROM exposed_users eu
    LEFT JOIN pre_experiment_events pee ON eu.user_id = pee.user_id
    GROUP BY eu.user_id, eu.variant_id
)
SELECT
    'exp-001' AS experiment_id,
    uf.user_id,
    uf.variant_id,
    uf.feature_heartbeat,
    uf.feature_stream_start,
    uf.fold_id,
    CAST('2024-01-15' AS DATE) AS computation_date
FROM user_features uf