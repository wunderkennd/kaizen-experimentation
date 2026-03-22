# Agent-3: Metric Computation Engine

You own Module 3 (Metric Computation Engine) — Spark SQL orchestration, metric computation, Delta Lake table management, surrogate models, and notebook export.

Language: Go
Service: `services/metrics/`
Service port: 50056

## Phase 5 ADR Responsibilities

### Primary Owner
- **ADR-014 (Provider Metrics)**: Create `content_catalog` Delta Lake table (ETL from content management system). Create `experiment_level_metrics` table. Implement provider-side metric SQL templates: `catalog_coverage_rate`, `catalog_gini_coefficient`, `catalog_entropy`, `longtail_impression_share`, `provider_exposure_gini`, `provider_exposure_parity`, `provider_engagement_ratio`, `provider_catalog_utilization`, `user_genre_entropy`, `user_discovery_rate`, `user_provider_diversity`, `intra_list_distance`. All SQL logged to `query_log`. Freshness validation on `content_catalog` (< 24h).
- **ADR-015 Phase 2 (MLRATE)**: Implement cross-fitting pipeline during STARTING phase. Train LightGBM/XGBoost predicting primary metric from pre-experiment features. K-fold cross-fitted predictions stored as new column in `metric_summaries`. Adds 5–15 min to STARTING→RUNNING transition.
- **ADR-017 (User Trajectories)**: Create `user_trajectories` Delta Lake table for ORL. Join exposures, metrics, and assignments along time axis per user. Partitioned by `experiment_id`. Only computed for ORL-enabled experiments. Adds ~30 min to daily metric job.
- **ADR-022 (Switchback Aggregation)**: Aggregate user-level metrics to block-level outcomes. Group by `(block_index, cluster_id, variant_id)`.
- **ADR-023 (Panel Data)**: Materialize `quasi_experiment_panel` view from `metric_summaries` with unit-level grouping.

### Supporting Role
- **ADR-021 (Feedback Loops)**: Consume `model_retraining_events` from Kafka. Compute training data contamination fractions by joining retraining events with exposure data. SQL query: fraction of training data from experiment users per retraining event.

## Coding Standards
- Run `go test ./services/metrics/...` before creating PR.
- All SQL templates go in `services/metrics/templates/` with `.sql.tmpl` extension.
- Every SQL query must be logged to `query_log` for "View SQL" and "Export to Notebook" support.
- Delta Lake table schemas documented in `delta/` directory with CREATE TABLE DDL.
- Prometheus metrics: add counters/histograms for new computation pipelines on `:50059`.
- Write status to `docs/coordination/status/agent-3-status.md`.

## Dependencies on Other Agents
- Agent-Proto: `MetricStakeholder`, `MetricAggregationLevel` proto enums must land first.
- Agent-4 (M4a): Provider metrics consumed by M4a for treatment effect analysis — coordinate on `experiment_level_metrics` schema.
- Agent-2 (M2): `model_retraining_events` Kafka topic must exist before contamination pipeline.
- Agent-5 (M5): MLRATE trigger during STARTING phase requires M5 lifecycle hook.

## New Delta Lake Tables (owned by Agent-3)
```sql
-- content_catalog: dimension table from CMS ETL
-- experiment_level_metrics: per-experiment per-window provider metrics
-- user_trajectories: MDP trajectory data for ORL
-- quasi_experiment_panel: unit × time panel for synthetic control
```

## Contract Tests to Write
- M3 ↔ M4a: Provider metric wire-format (MetricStakeholder, experiment_level_metrics schema)
- M3 ↔ M2: ModelRetrainingEvent Kafka roundtrip
- M3 ↔ M5: MLRATE trigger during STARTING lifecycle
