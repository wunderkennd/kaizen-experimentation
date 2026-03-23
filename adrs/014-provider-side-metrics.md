# ADR-014: Provider-Side Metrics

**Status**: Accepted
**Date**: 2026-03-23
**Deciders**: Agent-3 (M3 Metrics)
**Phase**: 5 — Cluster A (Multi-Stakeholder)

---

## Context

The SVOD platform hosts content from multiple providers. Recommendation experiments
can favour certain providers or genres over others, creating second-order effects on
content diversity that are invisible to user-level engagement metrics alone.

Providers and platform trust-and-safety teams need metrics that measure:
- **Catalog coverage**: how much of the available catalog is actually recommended
- **Content inequality**: Gini coefficient and entropy of the impression distribution
- **Provider fairness**: equitable exposure across content providers
- **User diversity**: per-user genre breadth, discovery of new content, and provider
  variety in individual watch histories

Without these metrics, experiments that improve CTR could simultaneously reduce
catalog diversity or concentrate impressions on a handful of providers.

---

## Decision

Implement 10 provider-side metric SQL templates in M3's Spark SQL renderer
(`services/metrics/internal/spark/templates/`), backed by a new Delta Lake table
(`delta.content_catalog`) that carries provider and genre metadata for each
content item.

### New Delta Tables

**`delta.content_catalog`** — written by the content ingestion pipeline, read by M3.
- Key columns: `content_id`, `provider_id`, `genre`, `popularity_rank`, `updated_at`.
- Freshness invariant: `MAX(updated_at)` must be within 24 hours before any provider
  metric computation. Enforced at the Go level in `ProviderMetricsJob.checkCatalogFreshness`.

**`delta.experiment_level_metrics`** — written by M3 `ProviderMetricsJob`, read by M4a.
- Stores aggregate (experiment, variant, metric_id) rows for metrics that cannot be
  disaggregated to the user level (catalog coverage, Gini, entropy, parity).

### Experiment-Level Metrics (→ `delta.experiment_level_metrics`)

| Metric ID | Description |
|---|---|
| `catalog_coverage_rate` | Fraction of catalog items that appear in at least one impression per variant |
| `catalog_gini_coefficient` | Gini coefficient of impression distribution across all catalog items (0 = equal, 1 = monopoly) |
| `catalog_entropy` | Shannon entropy of impression distribution across observed content (nats) |
| `longtail_impression_share` | Share of impressions going to content ranked below the `LongtailThreshold` percentile |
| `provider_exposure_gini` | Gini coefficient of impressions distributed across content providers |
| `provider_exposure_parity` | min_provider_share / max_provider_share (1.0 = perfect parity) |

### User-Level Metrics (→ `delta.metric_summaries`)

| Metric ID | Description |
|---|---|
| `user_genre_entropy` | Per-user Shannon entropy of genre distribution in watch history |
| `user_discovery_rate` | Fraction of experiment-period content that is new-to-user (not seen before experiment start) |
| `user_provider_diversity` | Count of distinct providers in each user's experiment-period watch history |
| `intra_list_distance` | 1 − HHI over genres in user's consumed list (complement of Herfindahl-Hirschman Index) |

### Freshness Validation

Before any provider metric SQL is executed, `ProviderMetricsJob.checkCatalogFreshness`
runs:

```sql
SELECT CAST(1 AS INT) AS is_fresh
FROM (SELECT MAX(updated_at) AS last_updated FROM delta.content_catalog) t
WHERE t.last_updated >= CURRENT_TIMESTAMP() - INTERVAL '24 HOURS'
```

If `RowCount == 0`, the job returns a descriptive error and no queries are logged.
This prevents stale catalog metadata from silently corrupting diversity metrics.

### Query Logging

All provider metric queries are logged to `query_log` with `job_type = "provider_metric"`,
consistent with the existing logging convention in all M3 jobs.

---

## TemplateParams Extensions

Three new fields added to `spark.TemplateParams`:

| Field | Type | Default | Description |
|---|---|---|---|
| `LongtailThreshold` | `float64` | `0.80` | PERCENT_RANK cutoff for longtail classification |
| `ProviderField` | `string` | `"provider_id"` | Column name in `content_catalog` for provider |
| `GenreField` | `string` | `"genre"` | Column name in `content_catalog` for genre |

---

## Consequences

**Positive**
- Providers can verify their content is receiving equitable exposure across experiment variants.
- Platform can detect experiments that improve engagement at the cost of catalog narrowing.
- `user_discovery_rate` and `user_genre_entropy` provide signals for novelty decay analysis (ADR-021).
- Intra-list distance integrates naturally with interleaving and slate bandit (ADR-016) evaluations.

**Negative / Risks**
- Catalog-level Gini templates require a CROSS JOIN between all variants and all catalog items.
  For large catalogs (>10M items), this may be expensive. Use approximate methods or partition
  filtering if this becomes a bottleneck.
- `user_discovery_rate` requires a full pre-experiment history scan of `delta.metric_events`.
  Add a retention-based index or cache pre-experiment content sets if query time is unacceptable.

---

## Implementation

- **ADR owner**: Agent-3 (M3 Metrics)
- **Depends on**: None (self-contained M3 change)
- **Consumed by**: M4a (ADR-021 feedback loop interference uses provider Gini and catalog entropy)
