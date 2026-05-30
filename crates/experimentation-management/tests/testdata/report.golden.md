# CUSTOM Metric Migration Report

## Summary

- **Total CUSTOM metrics analyzed:** 5
- **Tier 1 (structured):**
  - FILTERED_MEAN: 1
  - COMPOSITE: 1
  - WINDOWED_COUNT: 1
- **Tier 2 (METRICQL):** 1
- **Tier 3 (un-translatable):** 1

## Entries

### mobile_watch_time → FILTERED_MEAN

**Tier:** tier1_filtered_mean

**Reason:** matches FILTERED_MEAN shape with column-allowlist filter

**Proposal:**

```json
{
  "filtered_mean": {
    "filter_sql": "platform = 'mobile'",
    "value_column": "duration_ms"
  },
  "metric_id": "mobile_watch_time",
  "name": "Mobile Watch Time (migrated)",
  "source_event_type": "video_play",
  "type": 7
}
```

### engagement_lift → COMPOSITE

**Tier:** tier1_composite

**Reason:** operand references resolved, no cycle detected

**Proposal:**

```json
{
  "composite": {
    "operands": [
      {
        "metric_id": "sessions",
        "weight": 0.0
      },
      {
        "metric_id": "watch_time",
        "weight": 0.0
      }
    ],
    "operator": 1
  },
  "metric_id": "engagement_lift",
  "name": "Engagement Lift (migrated)",
  "type": 8
}
```

### rebuffers_7d → WINDOWED_COUNT

**Tier:** tier1_windowed_count

**Reason:** matches WINDOWED_COUNT shape with valid window_hours=168

**Proposal:**

```json
{
  "metric_id": "rebuffers_7d",
  "name": "Rebuffers in 7 Days (migrated)",
  "type": 9,
  "windowed_count": {
    "event_type": "rebuffer_event",
    "filter_sql": "",
    "window_hours": 168
  }
}
```

### revenue_per_user → METRICQL

**Tier:** tier2_metricql

**Reason:** translatable to MetricQL with two metric refs

**Proposal:**

```json
{
  "metric_id": "revenue_per_user",
  "metricql_expression": "@total_revenue / @user_count",
  "name": "Revenue Per User (migrated)",
  "type": 10
}
```

### custom_metric → Tier 3 (un-translatable)

**Tier:** tier3_untranslatable

**Reason:** matches no known translator shape; manual review required

