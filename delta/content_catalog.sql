-- ============================================================================
-- Delta Lake: content_catalog
-- ADR-014 Provider-Side Metrics: catalog of all available content with
-- provider and genre metadata. Written by the content ingestion pipeline.
-- Read by M3 provider-metric SQL templates.
--
-- Freshness invariant: MAX(updated_at) must be within 24 hours before any
-- provider-side metric computation. Enforced by ProviderMetricsJob.
-- ============================================================================

CREATE TABLE IF NOT EXISTS delta.content_catalog (
    content_id          STRING      NOT NULL,
    provider_id         STRING      NOT NULL,
    title               STRING,
    genre               STRING,         -- Primary genre label (e.g. "Drama", "Comedy")
    subgenre            STRING,         -- Optional secondary genre
    content_type        STRING,         -- 'movie', 'series', 'episode', 'short'
    duration_seconds    BIGINT,
    release_year        INT,
    country_of_origin   STRING,
    language            STRING,
    is_original         BOOLEAN,        -- Platform original (TRUE) vs licensed (FALSE)
    is_licensed         BOOLEAN,
    popularity_rank     BIGINT,         -- Global rank by rolling 30-day impressions (1 = most popular)
    updated_at          TIMESTAMP   NOT NULL   -- Freshness anchor for M3 staleness check
)
USING DELTA
PARTITIONED BY (provider_id STRING)
TBLPROPERTIES (
    'delta.autoOptimize.optimizeWrite' = 'true',
    'delta.autoOptimize.autoCompact' = 'true',
    'delta.logRetentionDuration' = 'interval 90 days',
    'delta.deletedFileRetentionDuration' = 'interval 7 days'
);

CREATE INDEX IF NOT EXISTS idx_content_catalog_content
    ON delta.content_catalog (content_id);

CREATE INDEX IF NOT EXISTS idx_content_catalog_genre
    ON delta.content_catalog (genre);
