// Package jobs provides metric computation job orchestrators.
package jobs

import (
	"context"
	"fmt"
	"log/slog"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// MLRATEResult summarizes the outcome of a cross-fitting run.
type MLRATEResult struct {
	ExperimentID string
	MetricID     string
	Folds        int
	UsersScored  int64
	CompletedAt  time.Time
}

// MLRATEJob orchestrates LightGBM K-fold cross-fitted prediction generation
// for AVLM Phase 2 covariate consumption (ADR-015 Phase 2).
//
// Pipeline:
//  1. Prepare per-user features from pre-experiment data with fold assignment
//     → writes to delta.mlrate_features
//  2. For each fold k=1..K, generate out-of-fold predictions using the
//     MLflow-registered model trained on all folds except k
//     → writes to delta.metric_summaries (as mlrate_covariate column)
type MLRATEJob struct {
	renderer *spark.SQLRenderer
	executor spark.SQLExecutor
	queryLog querylog.Writer
}

// NewMLRATEJob creates a new MLRATE cross-fitting job.
func NewMLRATEJob(
	renderer *spark.SQLRenderer,
	executor spark.SQLExecutor,
	ql querylog.Writer,
) *MLRATEJob {
	return &MLRATEJob{
		renderer: renderer,
		executor: executor,
		queryLog: ql,
	}
}

// Run executes the K-fold cross-fitting pipeline for a single metric.
func (j *MLRATEJob) Run(
	ctx context.Context,
	exp *config.ExperimentConfig,
	metric *config.MetricConfig,
	computationDate string,
) (*MLRATEResult, error) {
	folds := exp.MLRATEFoldsOrDefault()
	lookbackDays := metric.MLRATELookbackDaysOrDefault()

	if len(metric.MLRATEFeatureEventTypes) == 0 {
		return nil, fmt.Errorf("mlrate: metric %q has no mlrate_feature_event_types configured", metric.MetricID)
	}
	if metric.MLRATEModelURI == "" {
		return nil, fmt.Errorf("mlrate: metric %q has no mlrate_model_uri configured", metric.MetricID)
	}
	if exp.StartedAt == "" {
		return nil, fmt.Errorf("mlrate: experiment %q has no started_at date", exp.ExperimentID)
	}

	// Step 1: Prepare features with fold assignment.
	featParams := spark.TemplateParams{
		ExperimentID:            exp.ExperimentID,
		ComputationDate:         computationDate,
		ExperimentStartDate:     exp.StartedAt,
		MLRATEFolds:             folds,
		MLRATEFeatureEventTypes: metric.MLRATEFeatureEventTypes,
		MLRATELookbackDays:      lookbackDays,
	}

	featSQL, err := j.renderer.RenderMLRATEFeatures(featParams)
	if err != nil {
		return nil, fmt.Errorf("mlrate: render features for %s: %w", metric.MetricID, err)
	}

	featResult, err := j.executor.ExecuteAndWrite(ctx, featSQL, "delta.mlrate_features")
	if err != nil {
		return nil, fmt.Errorf("mlrate: execute features for %s: %w", metric.MetricID, err)
	}
	m3metrics.SparkQueryDuration.WithLabelValues("mlrate_features").Observe(featResult.Duration.Seconds())
	m3metrics.SparkQueryRows.WithLabelValues("mlrate_features").Observe(float64(featResult.RowCount))

	if err := j.queryLog.Log(ctx, querylog.Entry{
		ExperimentID: exp.ExperimentID,
		MetricID:     metric.MetricID,
		SQLText:      featSQL,
		RowCount:     featResult.RowCount,
		DurationMs:   featResult.Duration.Milliseconds(),
		JobType:      "mlrate_features",
	}); err != nil {
		return nil, fmt.Errorf("mlrate: log features query for %s: %w", metric.MetricID, err)
	}

	slog.Info("computed MLRATE features",
		"experiment_id", exp.ExperimentID,
		"metric_id", metric.MetricID,
		"folds", folds,
		"lookback_days", lookbackDays,
		"rows", featResult.RowCount,
	)

	// Step 2: For each fold, generate cross-fitted predictions.
	var totalPredictionRows int64
	for foldID := 1; foldID <= folds; foldID++ {
		predParams := spark.TemplateParams{
			ExperimentID:            exp.ExperimentID,
			MetricID:                metric.MetricID,
			ComputationDate:         computationDate,
			MLRATEFolds:             folds,
			MLRATEFeatureEventTypes: metric.MLRATEFeatureEventTypes,
			MLRATEModelURI:          metric.MLRATEModelURI,
			MLRATEFoldID:            foldID,
		}

		predSQL, err := j.renderer.RenderMLRATECrossFitPredict(predParams)
		if err != nil {
			return nil, fmt.Errorf("mlrate: render prediction fold %d for %s: %w", foldID, metric.MetricID, err)
		}

		predResult, err := j.executor.ExecuteAndWrite(ctx, predSQL, "delta.metric_summaries")
		if err != nil {
			return nil, fmt.Errorf("mlrate: execute prediction fold %d for %s: %w", foldID, metric.MetricID, err)
		}
		m3metrics.SparkQueryDuration.WithLabelValues("mlrate_crossfit").Observe(predResult.Duration.Seconds())
		m3metrics.SparkQueryRows.WithLabelValues("mlrate_crossfit").Observe(float64(predResult.RowCount))

		if err := j.queryLog.Log(ctx, querylog.Entry{
			ExperimentID: exp.ExperimentID,
			MetricID:     metric.MetricID,
			SQLText:      predSQL,
			RowCount:     predResult.RowCount,
			DurationMs:   predResult.Duration.Milliseconds(),
			JobType:      "mlrate_crossfit",
		}); err != nil {
			return nil, fmt.Errorf("mlrate: log prediction fold %d query for %s: %w", foldID, metric.MetricID, err)
		}

		totalPredictionRows += predResult.RowCount

		slog.Info("computed MLRATE cross-fit prediction",
			"experiment_id", exp.ExperimentID,
			"metric_id", metric.MetricID,
			"fold", foldID,
			"rows", predResult.RowCount,
		)
	}

	return &MLRATEResult{
		ExperimentID: exp.ExperimentID,
		MetricID:     metric.MetricID,
		Folds:        folds,
		UsersScored:  totalPredictionRows,
		CompletedAt:  time.Now(),
	}, nil
}
