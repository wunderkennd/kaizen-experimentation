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

// InterleavingResult summarizes the outcome of an interleaving score computation.
type InterleavingResult struct {
	ExperimentID string
	RowsWritten  int64
	CompletedAt  time.Time
}

// InterleavingJob computes per-user interleaving scores by joining exposure
// provenance (which algorithm contributed each item) with engagement events,
// and applying a configurable credit assignment method.
type InterleavingJob struct {
	config   *config.ConfigStore
	renderer *spark.SQLRenderer
	executor spark.SQLExecutor
	queryLog querylog.Writer
}

// NewInterleavingJob creates a new interleaving score computation job.
func NewInterleavingJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	executor spark.SQLExecutor,
	ql querylog.Writer,
) *InterleavingJob {
	return &InterleavingJob{
		config:   cfg,
		renderer: renderer,
		executor: executor,
		queryLog: ql,
	}
}

// Run computes interleaving scores for the given experiment.
// Only runs for INTERLEAVING-type experiments with credit_assignment configured.
func (j *InterleavingJob) Run(ctx context.Context, experimentID string) (*InterleavingResult, error) {
	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("jobs: %w", err)
	}

	if exp.Type != "INTERLEAVING" {
		return &InterleavingResult{
			ExperimentID: experimentID,
			RowsWritten:  0,
			CompletedAt:  time.Now(),
		}, nil
	}

	creditAssignment := exp.CreditAssignment
	if creditAssignment == "" {
		creditAssignment = "proportional" // default
	}

	engagementEventType := exp.EngagementEventType
	if engagementEventType == "" {
		engagementEventType = "click" // default
	}

	computationDate := time.Now().Format("2006-01-02")

	params := spark.TemplateParams{
		ExperimentID:        exp.ExperimentID,
		ComputationDate:     computationDate,
		CreditAssignment:    creditAssignment,
		EngagementEventType: engagementEventType,
	}

	sql, err := j.renderer.RenderInterleavingScore(params)
	if err != nil {
		return nil, fmt.Errorf("jobs: render interleaving score: %w", err)
	}

	result, err := j.executor.ExecuteAndWrite(ctx, sql, "delta.interleaving_scores")
	if err != nil {
		return nil, fmt.Errorf("jobs: execute interleaving score: %w", err)
	}

	if err := j.queryLog.Log(ctx, querylog.Entry{
		ExperimentID: experimentID,
		MetricID:     "",
		SQLText:      sql,
		RowCount:     result.RowCount,
		DurationMs:   result.Duration.Milliseconds(),
		JobType:      "interleaving_score",
	}); err != nil {
		return nil, fmt.Errorf("jobs: log interleaving score query: %w", err)
	}

	slog.Info("computed interleaving scores",
		"experiment_id", experimentID,
		"credit_assignment", creditAssignment,
		"rows", result.RowCount,
	)

	return &InterleavingResult{
		ExperimentID: experimentID,
		RowsWritten:  result.RowCount,
		CompletedAt:  time.Now(),
	}, nil
}
