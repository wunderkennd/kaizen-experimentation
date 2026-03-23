package jobs

import (
	"context"
	"fmt"
	"log/slog"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

const (
	// catalogFreshnessSQL returns 1 row when content_catalog.updated_at is within 24 h,
	// 0 rows when stale. Used by checkCatalogFreshness.
	catalogFreshnessSQL = `SELECT CAST(1 AS INT) AS is_fresh
FROM (SELECT MAX(updated_at) AS last_updated FROM delta.content_catalog) t
WHERE t.last_updated >= CURRENT_TIMESTAMP() - INTERVAL '24 HOURS'`

	// defaultProviderField is the column name in content_catalog for the provider.
	defaultProviderField = "provider_id"
	// defaultGenreField is the column name in content_catalog for the genre.
	defaultGenreField = "genre"
	// defaultLongtailThreshold is the PERCENT_RANK cutoff for longtail content.
	defaultLongtailThreshold = 0.80
)

// ProviderMetricsResult summarises the outcome of a provider metrics run.
type ProviderMetricsResult struct {
	ExperimentID      string
	MetricsComputed   int
	RowsWritten       int64
	CompletedAt       time.Time
}

// providerMetricSpec describes a single provider-side metric and which Delta
// table its output should be written to.
type providerMetricSpec struct {
	metricID    string
	jobType     string
	targetTable string
	render      func(*spark.SQLRenderer, spark.TemplateParams) (string, error)
}

// experimentLevelSpecs returns the specs for metrics that produce one row per
// (experiment, variant) and land in delta.experiment_level_metrics.
func experimentLevelSpecs(r *spark.SQLRenderer) []providerMetricSpec {
	return []providerMetricSpec{
		{
			metricID:    "catalog_coverage_rate",
			jobType:     "provider_metric",
			targetTable: "delta.experiment_level_metrics",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderCatalogCoverageRate(p) },
		},
		{
			metricID:    "catalog_gini_coefficient",
			jobType:     "provider_metric",
			targetTable: "delta.experiment_level_metrics",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderCatalogGiniCoefficient(p) },
		},
		{
			metricID:    "catalog_entropy",
			jobType:     "provider_metric",
			targetTable: "delta.experiment_level_metrics",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderCatalogEntropy(p) },
		},
		{
			metricID:    "longtail_impression_share",
			jobType:     "provider_metric",
			targetTable: "delta.experiment_level_metrics",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderLongtailImpressionShare(p) },
		},
		{
			metricID:    "provider_exposure_gini",
			jobType:     "provider_metric",
			targetTable: "delta.experiment_level_metrics",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderProviderExposureGini(p) },
		},
		{
			metricID:    "provider_exposure_parity",
			jobType:     "provider_metric",
			targetTable: "delta.experiment_level_metrics",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderProviderExposureParity(p) },
		},
	}
}

// userLevelSpecs returns the specs for metrics that produce one row per
// (experiment, user, variant) and land in delta.metric_summaries.
func userLevelSpecs(r *spark.SQLRenderer) []providerMetricSpec {
	return []providerMetricSpec{
		{
			metricID:    "user_genre_entropy",
			jobType:     "provider_metric",
			targetTable: "delta.metric_summaries",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderUserGenreEntropy(p) },
		},
		{
			metricID:    "user_discovery_rate",
			jobType:     "provider_metric",
			targetTable: "delta.metric_summaries",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderUserDiscoveryRate(p) },
		},
		{
			metricID:    "user_provider_diversity",
			jobType:     "provider_metric",
			targetTable: "delta.metric_summaries",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderUserProviderDiversity(p) },
		},
		{
			metricID:    "intra_list_distance",
			jobType:     "provider_metric",
			targetTable: "delta.metric_summaries",
			render:      func(rr *spark.SQLRenderer, p spark.TemplateParams) (string, error) { return rr.RenderIntraListDistance(p) },
		},
	}
}

// ProviderMetricsJob computes ADR-014 provider-side metrics for a single
// experiment. It validates content_catalog freshness before running any SQL.
type ProviderMetricsJob struct {
	config   *config.ConfigStore
	renderer *spark.SQLRenderer
	executor spark.SQLExecutor
	queryLog querylog.Writer
}

// NewProviderMetricsJob creates a new provider metrics job.
func NewProviderMetricsJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	executor spark.SQLExecutor,
	ql querylog.Writer,
) *ProviderMetricsJob {
	return &ProviderMetricsJob{
		config:   cfg,
		renderer: renderer,
		executor: executor,
		queryLog: ql,
	}
}

// Run computes all provider-side metrics for the given experiment.
// Returns an error if content_catalog is stale (updated_at > 24h ago).
func (j *ProviderMetricsJob) Run(ctx context.Context, experimentID string) (*ProviderMetricsResult, error) {
	if err := j.checkCatalogFreshness(ctx); err != nil {
		return nil, fmt.Errorf("jobs: provider metrics aborted for %s: %w", experimentID, err)
	}

	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("jobs: %w", err)
	}

	computationDate := time.Now().Format("2006-01-02")

	baseParams := spark.TemplateParams{
		ExperimentID:         exp.ExperimentID,
		SourceEventType:      "impression", // default event type for impression-based provider metrics
		ComputationDate:      computationDate,
		ExperimentStartDate:  exp.StartedAt,
		ProviderField:        defaultProviderField,
		GenreField:           defaultGenreField,
		LongtailThreshold:    defaultLongtailThreshold,
	}

	var totalRows int64
	metricsComputed := 0

	allSpecs := append(experimentLevelSpecs(j.renderer), userLevelSpecs(j.renderer)...)
	for _, spec := range allSpecs {
		params := baseParams
		params.MetricID = spec.metricID

		sql, err := spec.render(j.renderer, params)
		if err != nil {
			return nil, fmt.Errorf("jobs: render provider metric %s: %w", spec.metricID, err)
		}

		result, err := j.executor.ExecuteAndWrite(ctx, sql, spec.targetTable)
		if err != nil {
			return nil, fmt.Errorf("jobs: execute provider metric %s: %w", spec.metricID, err)
		}

		if err := j.queryLog.Log(ctx, querylog.Entry{
			ExperimentID: experimentID,
			MetricID:     spec.metricID,
			SQLText:      sql,
			RowCount:     result.RowCount,
			DurationMs:   result.Duration.Milliseconds(),
			JobType:      spec.jobType,
		}); err != nil {
			return nil, fmt.Errorf("jobs: log provider metric %s: %w", spec.metricID, err)
		}

		totalRows += result.RowCount
		metricsComputed++

		slog.Info("computed provider metric",
			"experiment_id", experimentID,
			"metric_id", spec.metricID,
			"target_table", spec.targetTable,
			"rows", result.RowCount,
			"duration_ms", result.Duration.Milliseconds(),
		)
	}

	return &ProviderMetricsResult{
		ExperimentID:    experimentID,
		MetricsComputed: metricsComputed,
		RowsWritten:     totalRows,
		CompletedAt:     time.Now(),
	}, nil
}

// checkCatalogFreshness validates that delta.content_catalog was updated within
// the last 24 hours. Returns a descriptive error if the catalog is stale.
func (j *ProviderMetricsJob) checkCatalogFreshness(ctx context.Context) error {
	result, err := j.executor.ExecuteSQL(ctx, catalogFreshnessSQL)
	if err != nil {
		return fmt.Errorf("catalog freshness check failed: %w", err)
	}
	if result.RowCount == 0 {
		return fmt.Errorf("content_catalog staleness violation: updated_at is older than 24 hours — provider metrics require fresh catalog data")
	}
	return nil
}
