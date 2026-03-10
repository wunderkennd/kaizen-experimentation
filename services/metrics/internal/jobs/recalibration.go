package jobs

import (
	"context"
	"fmt"
	"log/slog"
	"math"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

// RecalibrationResult summarizes the outcome of a surrogate model recalibration.
type RecalibrationResult struct {
	ModelID         string
	ExperimentID    string
	NewRSquared     float64
	PreviousRSquared float64
	DataPoints      int
	CompletedAt     time.Time
}

// RecalibrationJob compares projected vs actual treatment effects and updates
// a surrogate model's calibration R². This closes the feedback loop: after the
// prediction horizon elapses and actual long-term outcomes are observed, we
// evaluate how accurate the surrogate's projections were.
type RecalibrationJob struct {
	config        *config.ConfigStore
	renderer      *spark.SQLRenderer
	inputProvider InputMetricsProvider
	queryLog      querylog.Writer
	projReader    surrogate.ProjectionReader
	calibUpdater  surrogate.CalibrationUpdater
}

// NewRecalibrationJob creates a new surrogate recalibration job.
func NewRecalibrationJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	inputProvider InputMetricsProvider,
	ql querylog.Writer,
	projReader surrogate.ProjectionReader,
	calibUpdater surrogate.CalibrationUpdater,
) *RecalibrationJob {
	return &RecalibrationJob{
		config:        cfg,
		renderer:      renderer,
		inputProvider: inputProvider,
		queryLog:      ql,
		projReader:    projReader,
		calibUpdater:  calibUpdater,
	}
}

// effectPair holds a projected and actual treatment effect for R² computation.
type effectPair struct {
	projected float64
	actual    float64
}

// Run performs recalibration for the given experiment. It fetches actual
// target metric values, compares them with past projections, computes R²,
// and updates the model's calibration.
func (j *RecalibrationJob) Run(ctx context.Context, experimentID string) (*RecalibrationResult, error) {
	// Step 1: Look up surrogate model config.
	smCfg := j.config.GetSurrogateModelForExperiment(experimentID)
	if smCfg == nil {
		slog.Debug("no surrogate model linked", "experiment_id", experimentID)
		return nil, nil
	}

	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("recalibration job: %w", err)
	}

	controlVariantID := exp.ControlVariantID()
	if controlVariantID == "" {
		return nil, fmt.Errorf("recalibration job: experiment %q has no control variant", experimentID)
	}

	// Step 2: Render SQL to fetch actual target metric values.
	computationDate := time.Now().Format("2006-01-02")
	params := spark.TemplateParams{
		ExperimentID:          experimentID,
		ComputationDate:       computationDate,
		InputMetricIDs:        []string{smCfg.TargetMetricID},
		ObservationWindowDays: smCfg.PredictionHorizonDays,
	}

	inputSQL, err := j.renderer.RenderSurrogateInput(params)
	if err != nil {
		return nil, fmt.Errorf("recalibration job: render actual SQL: %w", err)
	}

	// Step 3: Fetch actual values.
	start := time.Now()
	actuals, err := j.inputProvider.Fetch(ctx, inputSQL)
	if err != nil {
		return nil, fmt.Errorf("recalibration job: fetch actual metrics: %w", err)
	}
	fetchDuration := time.Since(start)

	// Log the SQL for transparency.
	if err := j.queryLog.Log(ctx, querylog.Entry{
		ExperimentID: experimentID,
		MetricID:     smCfg.TargetMetricID,
		SQLText:      inputSQL,
		RowCount:     int64(len(actuals)),
		DurationMs:   fetchDuration.Milliseconds(),
		JobType:      "surrogate_recalibration_actual",
	}); err != nil {
		return nil, fmt.Errorf("recalibration job: log query: %w", err)
	}

	// Step 4: Read past projections.
	projections, err := j.projReader.ReadForExperiment(ctx, experimentID)
	if err != nil {
		return nil, fmt.Errorf("recalibration job: read projections: %w", err)
	}

	if len(projections) == 0 {
		slog.Debug("no past projections for recalibration", "experiment_id", experimentID)
		return &RecalibrationResult{
			ModelID:          smCfg.ModelID,
			ExperimentID:     experimentID,
			NewRSquared:      smCfg.CalibrationRSquared,
			PreviousRSquared: smCfg.CalibrationRSquared,
			DataPoints:       0,
			CompletedAt:      time.Now(),
		}, nil
	}

	// Step 5: Compute (projected, actual) effect pairs.
	controlActual, hasControl := actuals[controlVariantID]
	if !hasControl {
		return nil, fmt.Errorf("recalibration job: no actual values for control variant %q", controlVariantID)
	}
	controlValue := controlActual[smCfg.TargetMetricID]

	var pairs []effectPair
	for _, proj := range projections {
		variantActual, ok := actuals[proj.VariantID]
		if !ok {
			continue
		}
		actualEffect := variantActual[smCfg.TargetMetricID] - controlValue
		pairs = append(pairs, effectPair{
			projected: proj.ProjectedEffect,
			actual:    actualEffect,
		})
	}

	// Step 6: Compute R² and update calibration.
	previousR2 := smCfg.CalibrationRSquared
	newR2 := computeRSquared(pairs)

	if len(pairs) >= 2 {
		if err := j.calibUpdater.UpdateCalibration(ctx, smCfg.ModelID, newR2); err != nil {
			return nil, fmt.Errorf("recalibration job: update calibration: %w", err)
		}
	} else {
		newR2 = previousR2
	}

	slog.Info("completed surrogate recalibration",
		"experiment_id", experimentID,
		"model_id", smCfg.ModelID,
		"previous_r_squared", previousR2,
		"new_r_squared", newR2,
		"data_points", len(pairs),
	)

	return &RecalibrationResult{
		ModelID:          smCfg.ModelID,
		ExperimentID:     experimentID,
		NewRSquared:      newR2,
		PreviousRSquared: previousR2,
		DataPoints:       len(pairs),
		CompletedAt:      time.Now(),
	}, nil
}

// computeRSquared computes R² = 1 - SS_res/SS_tot for a set of (projected, actual) pairs.
// Returns 0 if fewer than 2 data points or if all actual values are identical (SS_tot == 0).
func computeRSquared(pairs []effectPair) float64 {
	n := len(pairs)
	if n < 2 {
		return 0
	}

	// Mean of actual values.
	var sumActual float64
	for _, p := range pairs {
		sumActual += p.actual
	}
	meanActual := sumActual / float64(n)

	// SS_res and SS_tot.
	var ssRes, ssTot float64
	for _, p := range pairs {
		ssRes += (p.actual - p.projected) * (p.actual - p.projected)
		ssTot += (p.actual - meanActual) * (p.actual - meanActual)
	}

	if ssTot == 0 {
		// All actual values are the same. If predictions match exactly, R² = 1.
		if ssRes == 0 {
			return 1
		}
		return 0
	}

	r2 := 1 - ssRes/ssTot
	// Clamp to valid range: R² can be negative for very poor models.
	if math.IsNaN(r2) || math.IsInf(r2, 0) {
		return 0
	}
	return r2
}
