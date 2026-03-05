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

// ContentConsumptionResult summarizes the outcome of a content consumption computation.
type ContentConsumptionResult struct {
	ExperimentID string
	RowsWritten  int64
	CompletedAt  time.Time
}

// ContentConsumptionJob computes per-variant, per-content-item aggregations
// for interference analysis (Jaccard, Gini, JS divergence in M4a).
type ContentConsumptionJob struct {
	config   *config.ConfigStore
	renderer *spark.SQLRenderer
	executor spark.SQLExecutor
	queryLog querylog.Writer
}

// NewContentConsumptionJob creates a new content consumption job.
func NewContentConsumptionJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	executor spark.SQLExecutor,
	ql querylog.Writer,
) *ContentConsumptionJob {
	return &ContentConsumptionJob{
		config:   cfg,
		renderer: renderer,
		executor: executor,
		queryLog: ql,
	}
}

// Run computes content consumption distributions for the given experiment.
func (j *ContentConsumptionJob) Run(ctx context.Context, experimentID string) (*ContentConsumptionResult, error) {
	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("jobs: %w", err)
	}

	computationDate := time.Now().Format("2006-01-02")

	params := spark.TemplateParams{
		ExperimentID:    exp.ExperimentID,
		ComputationDate: computationDate,
		ContentIDField:  "content_id",
	}

	sql, err := j.renderer.RenderContentConsumption(params)
	if err != nil {
		return nil, fmt.Errorf("jobs: render content consumption: %w", err)
	}

	result, err := j.executor.ExecuteAndWrite(ctx, sql, "delta.content_consumption")
	if err != nil {
		return nil, fmt.Errorf("jobs: execute content consumption: %w", err)
	}

	if err := j.queryLog.Log(ctx, querylog.Entry{
		ExperimentID: experimentID,
		MetricID:     "",
		SQLText:      sql,
		RowCount:     result.RowCount,
		DurationMs:   result.Duration.Milliseconds(),
		JobType:      "content_consumption",
	}); err != nil {
		return nil, fmt.Errorf("jobs: log content consumption query: %w", err)
	}

	slog.Info("computed content consumption",
		"experiment_id", experimentID,
		"rows", result.RowCount,
	)

	return &ContentConsumptionResult{
		ExperimentID: experimentID,
		RowsWritten:  result.RowCount,
		CompletedAt:  time.Now(),
	}, nil
}
