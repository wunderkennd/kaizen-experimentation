// Package jobs provides metric computation job orchestrators.
package jobs

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// JobResult summarizes the outcome of a computation run.
type JobResult struct {
	ExperimentID    string
	MetricsComputed int
	UsersProcessed  int
	CompletedAt     time.Time
}

// StandardJob orchestrates daily metric computation for a single experiment.
type StandardJob struct {
	config   *config.ConfigStore
	renderer *spark.SQLRenderer
	executor spark.SQLExecutor
	queryLog querylog.Writer
}

// NewStandardJob creates a new standard metric computation job.
func NewStandardJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	executor spark.SQLExecutor,
	ql querylog.Writer,
) *StandardJob {
	return &StandardJob{
		config:   cfg,
		renderer: renderer,
		executor: executor,
		queryLog: ql,
	}
}

// Run computes all metrics for the given experiment.
func (j *StandardJob) Run(ctx context.Context, experimentID string) (*JobResult, error) {
	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("jobs: %w", err)
	}

	metrics, err := j.config.GetMetricsForExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("jobs: %w", err)
	}

	computationDate := time.Now().Format("2006-01-02")
	var totalRows int64
	metricsComputed := 0

	const defaultCupedLookbackDays = 7

	controlVariantID := exp.ControlVariantID()

	for _, m := range metrics {
		params := spark.TemplateParams{
			ExperimentID:         exp.ExperimentID,
			MetricID:             m.MetricID,
			SourceEventType:      m.SourceEventType,
			ComputationDate:      computationDate,
			NumeratorEventType:   m.NumeratorEventType,
			DenominatorEventType: m.DenominatorEventType,
		}

		// QoE metrics use a separate template reading from delta.qoe_events.
		var sql string
		var jobType string
		if m.IsQoEMetric {
			params.QoEField = m.QoEField
			rendered, err := j.renderer.RenderQoEMetric(params)
			if err != nil {
				return nil, fmt.Errorf("jobs: render QoE metric %s: %w", m.MetricID, err)
			}
			sql = rendered
			jobType = "qoe_metric"
		} else {
			rendered, err := j.renderer.RenderForType(m.Type, params)
			if err != nil {
				slog.Warn("skipping unsupported metric type",
					"metric_id", m.MetricID, "type", m.Type, "error", err)
				continue
			}
			sql = rendered
			jobType = "daily_metric"
		}

		result, err := j.executor.ExecuteAndWrite(ctx, sql, "delta.metric_summaries")
		if err != nil {
			return nil, fmt.Errorf("jobs: execute metric %s: %w", m.MetricID, err)
		}

		if err := j.queryLog.Log(ctx, querylog.Entry{
			ExperimentID: experimentID,
			MetricID:     m.MetricID,
			SQLText:      sql,
			RowCount:     result.RowCount,
			DurationMs:   result.Duration.Milliseconds(),
			JobType:      jobType,
		}); err != nil {
			return nil, fmt.Errorf("jobs: log query for metric %s: %w", m.MetricID, err)
		}

		totalRows += result.RowCount
		metricsComputed++

		// For RATIO metrics, also compute delta method variance components.
		if strings.ToUpper(m.Type) == "RATIO" {
			deltaSQL, err := j.renderer.RenderRatioDeltaMethod(params)
			if err != nil {
				return nil, fmt.Errorf("jobs: render delta method for %s: %w", m.MetricID, err)
			}

			deltaResult, err := j.executor.ExecuteAndWrite(ctx, deltaSQL, "delta.daily_treatment_effects")
			if err != nil {
				return nil, fmt.Errorf("jobs: execute delta method for %s: %w", m.MetricID, err)
			}

			if err := j.queryLog.Log(ctx, querylog.Entry{
				ExperimentID: experimentID,
				MetricID:     m.MetricID,
				SQLText:      deltaSQL,
				RowCount:     deltaResult.RowCount,
				DurationMs:   deltaResult.Duration.Milliseconds(),
				JobType:      "delta_method",
			}); err != nil {
				return nil, fmt.Errorf("jobs: log delta method query for %s: %w", m.MetricID, err)
			}

			slog.Info("computed delta method inputs",
				"experiment_id", experimentID,
				"metric_id", m.MetricID,
				"rows", deltaResult.RowCount,
			)
		}

		// If metric has a CUPED covariate configured and experiment has a start date,
		// compute the pre-experiment covariate value for variance reduction.
		if m.CupedCovariateMetricID != "" && exp.StartedAt != "" {
			covMetric, err := j.config.GetMetric(m.CupedCovariateMetricID)
			if err != nil {
				return nil, fmt.Errorf("jobs: resolve CUPED covariate metric %s for %s: %w",
					m.CupedCovariateMetricID, m.MetricID, err)
			}

			cupedParams := params
			cupedParams.CupedEnabled = true
			cupedParams.CupedCovariateEventType = covMetric.SourceEventType
			cupedParams.ExperimentStartDate = exp.StartedAt
			cupedParams.CupedLookbackDays = defaultCupedLookbackDays

			cupedSQL, err := j.renderer.RenderCupedCovariate(cupedParams)
			if err != nil {
				return nil, fmt.Errorf("jobs: render CUPED covariate for %s: %w", m.MetricID, err)
			}

			cupedResult, err := j.executor.ExecuteAndWrite(ctx, cupedSQL, "delta.metric_summaries")
			if err != nil {
				return nil, fmt.Errorf("jobs: execute CUPED covariate for %s: %w", m.MetricID, err)
			}

			if err := j.queryLog.Log(ctx, querylog.Entry{
				ExperimentID: experimentID,
				MetricID:     m.MetricID,
				SQLText:      cupedSQL,
				RowCount:     cupedResult.RowCount,
				DurationMs:   cupedResult.Duration.Milliseconds(),
				JobType:      "cuped_covariate",
			}); err != nil {
				return nil, fmt.Errorf("jobs: log CUPED covariate query for %s: %w", m.MetricID, err)
			}

			slog.Info("computed CUPED covariate",
				"experiment_id", experimentID,
				"metric_id", m.MetricID,
				"covariate_metric_id", m.CupedCovariateMetricID,
				"rows", cupedResult.RowCount,
			)
		}

		// Session-level aggregation: if enabled, also compute per-session metrics.
		if exp.SessionLevel && !m.IsQoEMetric {
			slParams := params
			slParams.SessionLevel = true

			slSQL, err := j.renderer.RenderSessionLevelMean(slParams)
			if err != nil {
				return nil, fmt.Errorf("jobs: render session-level metric for %s: %w", m.MetricID, err)
			}

			slResult, err := j.executor.ExecuteAndWrite(ctx, slSQL, "delta.metric_summaries")
			if err != nil {
				return nil, fmt.Errorf("jobs: execute session-level metric for %s: %w", m.MetricID, err)
			}

			if err := j.queryLog.Log(ctx, querylog.Entry{
				ExperimentID: experimentID,
				MetricID:     m.MetricID,
				SQLText:      slSQL,
				RowCount:     slResult.RowCount,
				DurationMs:   slResult.Duration.Milliseconds(),
				JobType:      "session_level_metric",
			}); err != nil {
				return nil, fmt.Errorf("jobs: log session-level metric query for %s: %w", m.MetricID, err)
			}

			slog.Info("computed session-level metric",
				"experiment_id", experimentID,
				"metric_id", m.MetricID,
				"rows", slResult.RowCount,
			)
		}

		// Lifecycle segmentation: if enabled, also compute per-lifecycle-segment metrics.
		if exp.LifecycleStratificationEnabled && !m.IsQoEMetric {
			lcParams := params
			lcParams.LifecycleEnabled = true

			lcSQL, err := j.renderer.RenderLifecycleMean(lcParams)
			if err != nil {
				return nil, fmt.Errorf("jobs: render lifecycle metric for %s: %w", m.MetricID, err)
			}

			lcResult, err := j.executor.ExecuteAndWrite(ctx, lcSQL, "delta.metric_summaries")
			if err != nil {
				return nil, fmt.Errorf("jobs: execute lifecycle metric for %s: %w", m.MetricID, err)
			}

			if err := j.queryLog.Log(ctx, querylog.Entry{
				ExperimentID: experimentID,
				MetricID:     m.MetricID,
				SQLText:      lcSQL,
				RowCount:     lcResult.RowCount,
				DurationMs:   lcResult.Duration.Milliseconds(),
				JobType:      "lifecycle_metric",
			}); err != nil {
				return nil, fmt.Errorf("jobs: log lifecycle metric query for %s: %w", m.MetricID, err)
			}

			slog.Info("computed lifecycle metric",
				"experiment_id", experimentID,
				"metric_id", m.MetricID,
				"rows", lcResult.RowCount,
			)
		}

		slog.Info("computed metric",
			"experiment_id", experimentID,
			"metric_id", m.MetricID,
			"type", m.Type,
			"rows", result.RowCount,
			"duration_ms", result.Duration.Milliseconds(),
		)
	}

	// Post-processing: compute daily treatment effects for each metric.
	if controlVariantID != "" {
		for _, m := range metrics {
			teParams := spark.TemplateParams{
				ExperimentID:     exp.ExperimentID,
				MetricID:         m.MetricID,
				ComputationDate:  computationDate,
				ControlVariantID: controlVariantID,
			}

			teSQL, err := j.renderer.RenderDailyTreatmentEffect(teParams)
			if err != nil {
				return nil, fmt.Errorf("jobs: render daily treatment effect for %s: %w", m.MetricID, err)
			}

			teResult, err := j.executor.ExecuteAndWrite(ctx, teSQL, "delta.daily_treatment_effects")
			if err != nil {
				return nil, fmt.Errorf("jobs: execute daily treatment effect for %s: %w", m.MetricID, err)
			}

			if err := j.queryLog.Log(ctx, querylog.Entry{
				ExperimentID: experimentID,
				MetricID:     m.MetricID,
				SQLText:      teSQL,
				RowCount:     teResult.RowCount,
				DurationMs:   teResult.Duration.Milliseconds(),
				JobType:      "daily_treatment_effect",
			}); err != nil {
				return nil, fmt.Errorf("jobs: log daily treatment effect query for %s: %w", m.MetricID, err)
			}

			slog.Info("computed daily treatment effect",
				"experiment_id", experimentID,
				"metric_id", m.MetricID,
				"rows", teResult.RowCount,
			)
		}
	}

	// Post-processing: compute QoE-engagement correlation for experiments with QoE metrics.
	var qoeMetrics []config.MetricConfig
	var engagementMetrics []config.MetricConfig
	for _, m := range metrics {
		if m.IsQoEMetric {
			qoeMetrics = append(qoeMetrics, m)
		} else if !m.IsQoEMetric && m.Type != "RATIO" {
			engagementMetrics = append(engagementMetrics, m)
		}
	}
	if len(qoeMetrics) > 0 && len(engagementMetrics) > 0 {
		for _, qm := range qoeMetrics {
			for _, em := range engagementMetrics {
				corrParams := spark.TemplateParams{
					ExperimentID:       exp.ExperimentID,
					ComputationDate:    computationDate,
					QoEFieldA:          qm.QoEField,
					EngagementSourceType: em.SourceEventType,
				}

				corrSQL, err := j.renderer.RenderQoEEngagementCorrelation(corrParams)
				if err != nil {
					return nil, fmt.Errorf("jobs: render QoE-engagement correlation (%s × %s): %w", qm.MetricID, em.MetricID, err)
				}

				corrResult, err := j.executor.ExecuteAndWrite(ctx, corrSQL, "delta.daily_treatment_effects")
				if err != nil {
					return nil, fmt.Errorf("jobs: execute QoE-engagement correlation (%s × %s): %w", qm.MetricID, em.MetricID, err)
				}

				if err := j.queryLog.Log(ctx, querylog.Entry{
					ExperimentID: experimentID,
					MetricID:     qm.MetricID + "×" + em.MetricID,
					SQLText:      corrSQL,
					RowCount:     corrResult.RowCount,
					DurationMs:   corrResult.Duration.Milliseconds(),
					JobType:      "qoe_engagement_correlation",
				}); err != nil {
					return nil, fmt.Errorf("jobs: log QoE-engagement correlation query: %w", err)
				}

				slog.Info("computed QoE-engagement correlation",
					"experiment_id", experimentID,
					"qoe_metric", qm.MetricID,
					"engagement_metric", em.MetricID,
					"rows", corrResult.RowCount,
				)
			}
		}
	}

	return &JobResult{
		ExperimentID:    experimentID,
		MetricsComputed: metricsComputed,
		UsersProcessed:  int(totalRows),
		CompletedAt:     time.Now(),
	}, nil
}
