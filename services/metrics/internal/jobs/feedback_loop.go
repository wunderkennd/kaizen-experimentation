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

// FeedbackLoopResult summarises the outcome of a feedback loop contamination run.
type FeedbackLoopResult struct {
	ExperimentID string
	MetricID     string
	RowsWritten  int64
	CompletedAt  time.Time
}

// FeedbackLoopJob computes per-retraining-event pre/post treatment effects
// and contamination fractions for the ADR-021 feedback loop detection pipeline.
//
// Output is written to delta.feedback_loop_contamination and consumed by
// M4a's FeedbackLoopDetector (experimentation-stats::feedback_loop).
type FeedbackLoopJob struct {
	config   *config.ConfigStore
	renderer *spark.SQLRenderer
	executor spark.SQLExecutor
	queryLog querylog.Writer
}

// NewFeedbackLoopJob creates a new feedback loop contamination job.
func NewFeedbackLoopJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	executor spark.SQLExecutor,
	ql querylog.Writer,
) *FeedbackLoopJob {
	return &FeedbackLoopJob{
		config:   cfg,
		renderer: renderer,
		executor: executor,
		queryLog: ql,
	}
}

// Run computes feedback loop contamination data for the given experiment and metric.
//
// The job joins model_retraining_events (filtered to those active during this
// experiment) with delta.metric_summaries to produce one row per retraining
// event containing:
//   - treatment_contamination_fraction (from ModelRetrainingEvent)
//   - pre_retrain_effect  (mean effect in the 7 days before retraining)
//   - post_retrain_effect (mean effect in the 7 days after retraining)
//
// Retraining events with insufficient metric data in either window are
// excluded (NULL-filtered in the SQL template).
func (j *FeedbackLoopJob) Run(ctx context.Context, experimentID, metricID string) (*FeedbackLoopResult, error) {
	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("jobs: feedback loop: %w", err)
	}

	computationDate := time.Now().Format("2006-01-02")

	params := spark.TemplateParams{
		ExperimentID:    exp.ExperimentID,
		MetricID:        metricID,
		ControlVariantID: exp.ControlVariantID(),
		ComputationDate: computationDate,
	}

	sql, err := j.renderer.RenderFeedbackLoopContamination(params)
	if err != nil {
		return nil, fmt.Errorf("jobs: render feedback loop contamination: %w", err)
	}

	result, err := j.executor.ExecuteAndWrite(ctx, sql, "delta.feedback_loop_contamination")
	if err != nil {
		return nil, fmt.Errorf("jobs: execute feedback loop contamination: %w", err)
	}

	if err := j.queryLog.Log(ctx, querylog.Entry{
		ExperimentID: experimentID,
		MetricID:     metricID,
		SQLText:      sql,
		RowCount:     result.RowCount,
		DurationMs:   result.Duration.Milliseconds(),
		JobType:      "feedback_loop_contamination",
	}); err != nil {
		return nil, fmt.Errorf("jobs: log feedback loop contamination query: %w", err)
	}

	slog.Info("computed feedback loop contamination",
		"experiment_id", experimentID,
		"metric_id", metricID,
		"rows", result.RowCount,
	)

	return &FeedbackLoopResult{
		ExperimentID: experimentID,
		MetricID:     metricID,
		RowsWritten:  result.RowCount,
		CompletedAt:  time.Now(),
	}, nil
}
