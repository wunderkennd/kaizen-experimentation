// Package jobs provides metric computation job orchestrators.
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

	for _, m := range metrics {
		params := spark.TemplateParams{
			ExperimentID:    exp.ExperimentID,
			MetricID:        m.MetricID,
			SourceEventType: m.SourceEventType,
			ComputationDate: computationDate,
		}

		sql, err := j.renderer.RenderForType(m.Type, params)
		if err != nil {
			slog.Warn("skipping unsupported metric type",
				"metric_id", m.MetricID, "type", m.Type, "error", err)
			continue
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
			JobType:      "daily_metric",
		}); err != nil {
			return nil, fmt.Errorf("jobs: log query for metric %s: %w", m.MetricID, err)
		}

		totalRows += result.RowCount
		metricsComputed++

		slog.Info("computed metric",
			"experiment_id", experimentID,
			"metric_id", m.MetricID,
			"type", m.Type,
			"rows", result.RowCount,
			"duration_ms", result.Duration.Milliseconds(),
		)
	}

	return &JobResult{
		ExperimentID:    experimentID,
		MetricsComputed: metricsComputed,
		UsersProcessed:  int(totalRows),
		CompletedAt:     time.Now(),
	}, nil
}
