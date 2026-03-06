package jobs

import (
	"context"
	"fmt"
	"log/slog"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

// InputMetricsProvider fetches aggregated per-variant metric values from Spark SQL.
// In production, this executes the SQL against a Spark cluster and parses the result set.
// In tests, a mock implementation returns predefined values.
type InputMetricsProvider interface {
	Fetch(ctx context.Context, sql string) (surrogate.InputMetrics, error)
}

// SurrogateJob orchestrates surrogate metric computation for a single experiment.
// It renders input aggregation SQL, fetches per-variant metric averages, loads the
// surrogate model, computes projected treatment effects, and writes projections.
type SurrogateJob struct {
	config        *config.ConfigStore
	renderer      *spark.SQLRenderer
	inputProvider InputMetricsProvider
	queryLog      querylog.Writer
	modelLoader   surrogate.ModelLoader
	projWriter    surrogate.ProjectionWriter
}

// NewSurrogateJob creates a new surrogate metric computation job.
func NewSurrogateJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	inputProvider InputMetricsProvider,
	ql querylog.Writer,
	loader surrogate.ModelLoader,
	writer surrogate.ProjectionWriter,
) *SurrogateJob {
	return &SurrogateJob{
		config:        cfg,
		renderer:      renderer,
		inputProvider: inputProvider,
		queryLog:      ql,
		modelLoader:   loader,
		projWriter:    writer,
	}
}

// Run computes surrogate projections for the given experiment.
// Returns nil JobResult (no rows written to metric_summaries) if the experiment
// has no linked surrogate model.
func (j *SurrogateJob) Run(ctx context.Context, experimentID string) (*JobResult, error) {
	smCfg := j.config.GetSurrogateModelForExperiment(experimentID)
	if smCfg == nil {
		slog.Debug("no surrogate model linked", "experiment_id", experimentID)
		return &JobResult{
			ExperimentID:    experimentID,
			MetricsComputed: 0,
			CompletedAt:     time.Now(),
		}, nil
	}

	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("surrogate job: %w", err)
	}

	controlVariantID := exp.ControlVariantID()
	if controlVariantID == "" {
		return nil, fmt.Errorf("surrogate job: experiment %q has no control variant", experimentID)
	}

	computationDate := time.Now().Format("2006-01-02")

	// Step 1: Render input aggregation SQL.
	params := spark.TemplateParams{
		ExperimentID:          experimentID,
		ComputationDate:       computationDate,
		InputMetricIDs:        smCfg.InputMetricIDs,
		ObservationWindowDays: smCfg.ObservationWindowDays,
	}

	inputSQL, err := j.renderer.RenderSurrogateInput(params)
	if err != nil {
		return nil, fmt.Errorf("surrogate job: render input SQL: %w", err)
	}

	// Step 2: Execute SQL to fetch per-variant metric averages.
	start := time.Now()
	inputs, err := j.inputProvider.Fetch(ctx, inputSQL)
	if err != nil {
		return nil, fmt.Errorf("surrogate job: fetch input metrics: %w", err)
	}
	fetchDuration := time.Since(start)

	// Log the input aggregation SQL for transparency.
	if err := j.queryLog.Log(ctx, querylog.Entry{
		ExperimentID: experimentID,
		MetricID:     smCfg.TargetMetricID,
		SQLText:      inputSQL,
		RowCount:     int64(len(inputs)),
		DurationMs:   fetchDuration.Milliseconds(),
		JobType:      "surrogate_input",
	}); err != nil {
		return nil, fmt.Errorf("surrogate job: log input query: %w", err)
	}

	// Step 3: Load the surrogate model.
	model, err := j.modelLoader.Load(smCfg)
	if err != nil {
		return nil, fmt.Errorf("surrogate job: load model %q: %w", smCfg.ModelID, err)
	}

	// Step 4: Compute projected treatment effects.
	projections, err := model.Predict(inputs, controlVariantID)
	if err != nil {
		return nil, fmt.Errorf("surrogate job: predict: %w", err)
	}

	// Step 5: Write projections to PostgreSQL.
	computedAt := time.Now()
	for _, p := range projections {
		record := surrogate.ProjectionRecord{
			ExperimentID:        experimentID,
			VariantID:           p.VariantID,
			ModelID:             smCfg.ModelID,
			ProjectedEffect:     p.ProjectedEffect,
			ProjectionCILower:   p.ProjectionCILower,
			ProjectionCIUpper:   p.ProjectionCIUpper,
			CalibrationRSquared: p.CalibrationRSquared,
			ComputedAt:          computedAt,
		}
		if err := j.projWriter.Write(ctx, record); err != nil {
			return nil, fmt.Errorf("surrogate job: write projection for variant %q: %w", p.VariantID, err)
		}
	}

	slog.Info("computed surrogate projections",
		"experiment_id", experimentID,
		"model_id", smCfg.ModelID,
		"target_metric", smCfg.TargetMetricID,
		"variants_projected", len(projections),
	)

	return &JobResult{
		ExperimentID:    experimentID,
		MetricsComputed: len(projections),
		CompletedAt:     computedAt,
	}, nil
}

// MockInputMetricsProvider returns empty InputMetrics for development use.
// In production, this would execute SQL against a Spark cluster and parse result rows.
type MockInputMetricsProvider struct {
	Inputs surrogate.InputMetrics
}

func (m *MockInputMetricsProvider) Fetch(_ context.Context, _ string) (surrogate.InputMetrics, error) {
	if m.Inputs != nil {
		return m.Inputs, nil
	}
	return surrogate.InputMetrics{}, nil
}
