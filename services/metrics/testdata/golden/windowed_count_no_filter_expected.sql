WITH exposed_users AS (
    SELECT user_id, variant_id,
           MIN(event_timestamp) AS exposure_ts,
           MIN(assignment_probability) AS assignment_probability
    FROM delta.exposures
    WHERE experiment_id = 'exp-001'
    GROUP BY user_id, variant_id
),
windowed_events AS (
    SELECT eu.user_id, eu.variant_id
    FROM delta.metric_events me
    INNER JOIN exposed_users eu ON me.user_id = eu.user_id
    WHERE me.event_type = 'stream_start'
      AND me.event_timestamp >= eu.exposure_ts
      AND me.event_timestamp <  eu.exposure_ts + INTERVAL 24 HOURS
)
SELECT
    'exp-001' AS experiment_id,
    eu.user_id,
    eu.variant_id,
    'all_starts_24h' AS metric_id,
    CAST(COUNT(we.user_id) AS DOUBLE) AS metric_value,
    CAST('2024-01-15' AS DATE) AS computation_date,
    eu.assignment_probability
FROM exposed_users eu
LEFT JOIN windowed_events we ON eu.user_id = we.user_id AND eu.variant_id = we.variant_id
GROUP BY eu.user_id, eu.variant_id, eu.assignment_probability;
