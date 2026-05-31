// Package jobs provides metric computation job orchestrators.
package jobs

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/shadow"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/status"
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
	config       *config.ConfigStore
	renderer     *spark.SQLRenderer
	executor     spark.SQLExecutor
	queryLog     querylog.Writer
	statusWriter status.Writer  // ADR-026 #475: per-metric outcome flushed to PG; nil → flush is a no-op.
	shadowStore  shadow.Store   // ADR-026 Phase 3 (#437): shadow-run computation; nil → shadow pass is a no-op.
}

// StandardJobOption configures optional StandardJob behavior (ADR-026 #475).
// Functional options keep the legacy 4-arg constructor wire-compatible while
// letting cmd/main.go inject the production status.PgWriter.
type StandardJobOption func(*StandardJob)

// WithStatusWriter wires a status.Writer for per-metric computation outcome
// recording. When unset (the default for existing tests), status flushes are
// no-ops — the topo-order scheduling and skip-on-upstream-failure semantics
// still apply, but nothing is persisted.
func WithStatusWriter(w status.Writer) StandardJobOption {
	return func(j *StandardJob) { j.statusWriter = w }
}

// WithShadowStore wires a shadow.Store so the nightly pass also computes
// PENDING shadow runs (ADR-026 Phase 3 #437).  When unset (nil), the shadow
// iteration is a no-op — all existing behaviour is preserved for sites that
// use the legacy 4-arg constructor.
func WithShadowStore(s shadow.Store) StandardJobOption {
	return func(j *StandardJob) { j.shadowStore = s }
}

// NewStandardJob creates a new standard metric computation job. Options are
// optional and additive; the 4-arg form is preserved for backwards-compatibility
// with the dozen+ test sites still using it.
func NewStandardJob(
	cfg *config.ConfigStore,
	renderer *spark.SQLRenderer,
	executor spark.SQLExecutor,
	ql querylog.Writer,
	opts ...StandardJobOption,
) *StandardJob {
	j := &StandardJob{
		config:   cfg,
		renderer: renderer,
		executor: executor,
		queryLog: ql,
	}
	for _, opt := range opts {
		opt(j)
	}
	return j
}

// Run computes all metrics for the given experiment.
//
// ADR-026 #475: metrics are executed in topological order so that COMPOSITE
// metrics run after every operand they reference. If any operand fails (or is
// missing from this scheduling pass), the dependent COMPOSITE is marked
// SkippedUpstreamFailure rather than attempted — preserving the operator
// expectation that a COMPOSITE never silently aggregates incomplete inputs.
//
// Fail-fast semantics for non-COMPOSITE errors are preserved: render/execute
// failures still return the first wrapped error to the caller (the chaos suite
// asserts this). The deferred status flush ensures statuses recorded up to the
// failure are still persisted.
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

	// ADR-026 #475: topo-order scheduling for COMPOSITE metrics.
	// metrics is []config.MetricConfig (by value); TopologicalOrder takes
	// []*config.MetricConfig — adapt with pointer slice.
	ordered := make([]*config.MetricConfig, len(metrics))
	for i := range metrics {
		ordered[i] = &metrics[i]
	}
	sortedMetrics, cycleNodes, failedParse, err := TopologicalOrder(ordered)
	if err != nil {
		return nil, fmt.Errorf("jobs: topological sort: %w", err)
	}

	sm := newStatusMap()
	// Record cycle-skipped nodes upfront so the deferred flush has them.
	for id := range cycleNodes {
		sm.markSkippedCycle(id)
		slog.Warn("metric skipped (cycle)",
			"experiment_id", experimentID,
			"metric_id", id,
			"computation_date", computationDate,
		)
	}

	// ADR-026 Phase 2 (#435): pre-mark METRICQL parse failures as status.Failed
	// so downstream gates (blockerFor for COMPOSITE, blockerForRefs for METRICQL)
	// treat them identically to executor failures. Without this, a parse failure
	// would land the metric in `sorted` with no recorded status, and its
	// dependents would be marked SkippedUpstreamFailure for what's actually a
	// parse error -- the wrong observable.
	for id, parseErr := range failedParse {
		sm.markFailed(id, "metricql: parse: "+parseErr.Error())
		slog.Warn("metric failed (metricql parse)",
			"experiment_id", experimentID,
			"metric_id", id,
			"computation_date", computationDate,
			"err", parseErr,
		)
	}

	// Flush statuses to PG after the loop (or after an early-return failure).
	defer func() {
		// ADR-026 #475: if Run early-returns on a non-COMPOSITE failure, any
		// downstream COMPOSITE in `sortedMetrics` was never visited and thus
		// has no entry in `sm` — but topo order guarantees operands run before
		// COMPOSITEs, so the COMPOSITE *would* have been gated by `blockerFor`
		// had we reached it. Mirror that gate here so the status table reflects
		// SkippedUpstreamFailure rather than silently omitting the row (which
		// M4a would interpret as "missing", not "failed-upstream").
		markUnvisitedDependentsAsSkipped(sortedMetrics, sm)

		if flushErr := j.flushStatus(ctx, experimentID, computationDate, sm); flushErr != nil {
			slog.Error("status flush failed (non-fatal)",
				"experiment_id", experimentID,
				"computation_date", computationDate,
				"err", flushErr,
			)
		}
	}()

	for _, mPtr := range sortedMetrics {
		// COMPOSITE: gate on operand status BEFORE attempting execution.
		if strings.ToUpper(mPtr.Type) == "COMPOSITE" {
			if blocker, blocked := sm.blockerFor(mPtr.Operands); blocked {
				sm.markSkippedUpstream(mPtr.MetricID, blocker)
				slog.Warn("composite skipped (operand failed or missing)",
					"experiment_id", experimentID,
					"metric_id", mPtr.MetricID,
					"blocker", blocker,
					"computation_date", computationDate,
				)
				continue
			}
		}

		// ADR-026 Phase 2 (#435) — METRICQL: symmetric gate on @metric_ref
		// status BEFORE attempting compile/execute. Without this, an
		// expression like "0.7 * @watch_time + 0.3 * @ctr" would compile
		// and execute against delta.metric_summaries rows that don't exist
		// (or are stale from a prior pass) when watch_time/ctr failed --
		// silently wrong numbers, not an error. Round-4 BUG-0002 fix.
		// blockerForRefs is shared with markUnvisitedDependentsAsSkipped
		// so "blocked" means the same thing in both places.
		if strings.ToUpper(mPtr.Type) == "METRICQL" {
			refs, parseErr := operandIDs(mPtr)
			if parseErr != nil {
				// Parse failure -- already recorded as Failed via the
				// failedParse pre-mark; defensive re-mark in case the
				// expression text changed between DAG build and gate.
				sm.markFailed(mPtr.MetricID, "metricql: parse: "+parseErr.Error())
				slog.Warn("metricql skipped (parse failure)",
					"experiment_id", experimentID,
					"metric_id", mPtr.MetricID,
					"err", parseErr,
					"computation_date", computationDate,
				)
				continue
			}
			if blocker := sm.blockerForRefs(refs); blocker != "" {
				sm.markSkippedUpstream(mPtr.MetricID, blocker)
				slog.Warn("metricql skipped (ref failed or missing)",
					"experiment_id", experimentID,
					"metric_id", mPtr.MetricID,
					"blocker", blocker,
					"computation_date", computationDate,
				)
				continue
			}
		}

		// Dereference into a local value so the existing loop body — written
		// against a value `m` — keeps working unchanged below.
		m := *mPtr

		params := spark.TemplateParams{
			ExperimentID:         exp.ExperimentID,
			MetricID:             m.MetricID,
			SourceEventType:      m.SourceEventType,
			ComputationDate:      computationDate,
			NumeratorEventType:   m.NumeratorEventType,
			DenominatorEventType: m.DenominatorEventType,
			CustomSQL:            m.CustomSQL,
			Percentile:           m.Percentile,
			// ADR-026 Phase 1
			FilterSQL:   m.FilterSQL,
			ValueColumn: m.ValueColumn,
			Operator:    m.Operator,
			EventType:   m.EventType,
			WindowHours: m.WindowHours,
			Operands:    toSparkOperands(m.Operands),
			// ADR-026 Phase 2 (#435)
			MetricqlExpression: m.MetricqlExpression,
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
				// Record the render failure in the status table so M4a can
				// distinguish it from "metric was never scheduled". Mirrors the
				// markFailed call on the execute-error path below. Devin
				// observability finding on #556.
				sm.markFailed(m.MetricID, fmt.Sprintf("render: %v", err))
				slog.Warn("skipping metric: render error",
					"metric_id", m.MetricID, "type", m.Type, "error", err)
				continue
			}
			sql = rendered
			jobType = "daily_metric"
		}

		result, err := j.executor.ExecuteAndWrite(ctx, sql, "delta.metric_summaries")
		if err != nil {
			sm.markFailed(m.MetricID, fmt.Sprintf("execute: %v", err))
			return nil, fmt.Errorf("jobs: execute metric %s: %w", m.MetricID, err)
		}
		m3metrics.SparkQueryDuration.WithLabelValues(jobType).Observe(result.Duration.Seconds())
		m3metrics.SparkQueryRows.WithLabelValues(jobType).Observe(float64(result.RowCount))

		if err := j.queryLog.Log(ctx, querylog.Entry{
			ExperimentID: experimentID,
			MetricID:     m.MetricID,
			SQLText:      sql,
			RowCount:     result.RowCount,
			DurationMs:   result.Duration.Milliseconds(),
			JobType:      jobType,
		}); err != nil {
			sm.markFailed(m.MetricID, fmt.Sprintf("query_log: %v", err))
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
			m3metrics.SparkQueryDuration.WithLabelValues("delta_method").Observe(deltaResult.Duration.Seconds())
			m3metrics.SparkQueryRows.WithLabelValues("delta_method").Observe(float64(deltaResult.RowCount))

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
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping CUPED covariate: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
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
				m3metrics.SparkQueryDuration.WithLabelValues("cuped_covariate").Observe(cupedResult.Duration.Seconds())
				m3metrics.SparkQueryRows.WithLabelValues("cuped_covariate").Observe(float64(cupedResult.RowCount))

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
		}

		// MLRATE cross-fitting: if experiment has MLRATE enabled and metric has
		// feature config, generate K-fold cross-fitted predictions as AVLM covariates.
		if exp.MLRATEEnabled && len(m.MLRATEFeatureEventTypes) > 0 && m.MLRATEModelURI != "" && exp.StartedAt != "" {
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping MLRATE cross-fit: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
				mlrateJob := NewMLRATEJob(j.renderer, j.executor, j.queryLog)
				mlrateResult, err := mlrateJob.Run(ctx, exp, &m, computationDate)
				if err != nil {
					return nil, fmt.Errorf("jobs: MLRATE cross-fit for %s: %w", m.MetricID, err)
				}

				slog.Info("computed MLRATE cross-fitted predictions",
					"experiment_id", experimentID,
					"metric_id", m.MetricID,
					"folds", mlrateResult.Folds,
					"users_scored", mlrateResult.UsersScored,
				)
			}
		}

		// Session-level aggregation: if enabled, also compute per-session metrics.
		if exp.SessionLevel && !m.IsQoEMetric {
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping session-level metric: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
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
				m3metrics.SparkQueryDuration.WithLabelValues("session_level_metric").Observe(slResult.Duration.Seconds())
				m3metrics.SparkQueryRows.WithLabelValues("session_level_metric").Observe(float64(slResult.RowCount))

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
		}

		// Lifecycle segmentation: if enabled, also compute per-lifecycle-segment metrics.
		if exp.LifecycleStratificationEnabled && !m.IsQoEMetric {
			if !isLegacyStyle(m.Type) {
				slog.Info("skipping lifecycle metric: legacy column convention not supported for this metric type",
					"metric_id", m.MetricID,
					"type", m.Type,
				)
			} else {
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
				m3metrics.SparkQueryDuration.WithLabelValues("lifecycle_metric").Observe(lcResult.Duration.Seconds())
				m3metrics.SparkQueryRows.WithLabelValues("lifecycle_metric").Observe(float64(lcResult.RowCount))

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
		}

		sm.markCompleted(m.MetricID)
		slog.Info("computed metric",
			"experiment_id", experimentID,
			"metric_id", m.MetricID,
			"type", m.Type,
			"rows", result.RowCount,
			"duration_ms", result.Duration.Milliseconds(),
		)
	}

	// Filter to metrics that actually wrote rows to delta.metric_summaries.
	// Post-processing (daily treatment effects + QoE-engagement correlation)
	// reads from that table — running it against skipped / cycle-excluded /
	// failed COMPOSITE metrics would write empty or stale results to
	// delta.daily_treatment_effects. Before #475 this was rare (render errors
	// only); the COMPOSITE skip path makes it a normal operational outcome.
	// Devin BUG-0002 on #556.
	completedMetrics := make([]config.MetricConfig, 0, len(metrics))
	for _, m := range metrics {
		if sm.entries[m.MetricID] == status.Completed {
			completedMetrics = append(completedMetrics, m)
		}
	}

	// Post-processing: compute daily treatment effects for each metric.
	if controlVariantID != "" {
		for _, m := range completedMetrics {
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
			m3metrics.SparkQueryDuration.WithLabelValues("daily_treatment_effect").Observe(teResult.Duration.Seconds())
			m3metrics.SparkQueryRows.WithLabelValues("daily_treatment_effect").Observe(float64(teResult.RowCount))

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
	// Uses `completedMetrics` for the same reason as daily treatment effects above —
	// correlations against a skipped metric read empty rows from delta.metric_summaries.
	var qoeMetrics []config.MetricConfig
	var engagementMetrics []config.MetricConfig
	for _, m := range completedMetrics {
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
					ExperimentID:         exp.ExperimentID,
					ComputationDate:      computationDate,
					QoEFieldA:            qm.QoEField,
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
				m3metrics.SparkQueryDuration.WithLabelValues("qoe_engagement_correlation").Observe(corrResult.Duration.Seconds())
				m3metrics.SparkQueryRows.WithLabelValues("qoe_engagement_correlation").Observe(float64(corrResult.RowCount))

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

	// ADR-026 Phase 3 (#437): shadow-run computation.
	// Runs AFTER the regular per-metric loop so a shadow failure can never
	// interrupt the experiment's primary metrics.  runShadows absorbs all
	// per-shadow errors internally; it is always safe to call here.
	j.runShadows(ctx, experimentID, computationDate)

	return &JobResult{
		ExperimentID:    experimentID,
		MetricsComputed: metricsComputed,
		UsersProcessed:  int(totalRows),
		CompletedAt:     time.Now(),
	}, nil
}

// isLegacyStyle reports whether a MetricType uses the legacy MEAN-style
// column convention (me.value, me.event_type = '{{.SourceEventType}}')
// that the post-processing templates (cuped_covariate, session_level_mean,
// lifecycle_mean, mlrate_*) assume.
//
// ADR-026 Phase 1 types (FILTERED_MEAN, COMPOSITE, WINDOWED_COUNT) use
// different column conventions and must skip post-processing until those
// templates grow per-type variants (issue #511 option b).
func isLegacyStyle(metricType string) bool {
	switch strings.ToUpper(metricType) {
	case "MEAN", "PROPORTION", "COUNT", "RATIO", "PERCENTILE", "CUSTOM":
		return true
	}
	return false
}

// flushStatus persists every recorded statusMap entry via the injected
// status.Writer. When no writer is configured (i.e., the legacy 4-arg ctor was
// used by existing tests), this is a no-op so behaviour stays unchanged.
// Individual write failures are surfaced to the caller; the caller logs them
// as non-fatal — losing a status row must not fail the whole pass.
func (j *StandardJob) flushStatus(
	ctx context.Context,
	experimentID, computationDate string,
	sm *statusMap,
) error {
	if j.statusWriter == nil {
		return nil
	}
	for id, st := range sm.entries {
		entry := status.Entry{
			ExperimentID:    experimentID,
			MetricID:        id,
			ComputationDate: computationDate,
			Status:          st,
			Reason:          sm.reasonOf(id),
			RecordedAt:      time.Now(),
		}
		if err := j.statusWriter.Write(ctx, entry); err != nil {
			return err
		}
	}
	return nil
}

// markUnvisitedDependentsAsSkipped scans the topo-ordered metric list and, for
// any dependent metric (COMPOSITE or METRICQL) whose status is unrecorded in
// `sm`, marks it SkippedUpstreamFailure if its dependencies include at least
// one non-Completed status. Called from the deferred flush so early-return
// paths (fail-fast on an unrelated failure) still surface downstream dependents
// to M4a — otherwise the dependent would have no row in
// metric_computation_status, indistinguishable from a metric that was never
// scheduled.
//
// Generalized from the pre-ADR-026-Phase-2 markUnvisitedCompositesAsSkipped
// (round-4 BUG-0001): the loop guard is now dependency-shaped, not type-shaped.
// COMPOSITE deps come from m.Operands; METRICQL deps come from parsed
// @metric_refs via operandIDs. Both feed `blockerForRefs` for the blocker check.
//
// Dependents whose dependencies all completed (e.g., the early-return happened
// on an unrelated metric later in topo order) are intentionally left
// unrecorded: "not yet computed" is the right signal for M4a in that case,
// not "skipped".
func markUnvisitedDependentsAsSkipped(sortedMetrics []*config.MetricConfig, sm *statusMap) {
	for _, mPtr := range sortedMetrics {
		if _, recorded := sm.entries[mPtr.MetricID]; recorded {
			// Already Completed / Failed / SkippedUpstreamFailure / SkippedCycle.
			continue
		}
		refs, err := operandIDs(mPtr)
		if err != nil {
			// METRICQL parse failure encountered here would normally already
			// be recorded via the pre-loop pre-mark from failedParse; if not,
			// surface it as Failed (defense-in-depth).
			sm.markFailed(mPtr.MetricID, "metricql: parse: "+err.Error())
			continue
		}
		if len(refs) == 0 {
			// Leaf metric -- no dependencies to fail on; nothing to skip.
			continue
		}
		if blocker := sm.blockerForRefs(refs); blocker != "" {
			sm.markSkippedUpstream(mPtr.MetricID, blocker)
		}
	}
}

// toSparkOperands converts config-layer OperandConfig values to the
// spark.OperandParam shape consumed by the renderer's composite template.
// Returns nil for an empty/nil input (matches Go's zero-value convention).
func toSparkOperands(in []config.OperandConfig) []spark.OperandParam {
	if len(in) == 0 {
		return nil
	}
	out := make([]spark.OperandParam, len(in))
	for i, op := range in {
		out[i] = spark.OperandParam{
			MetricID: op.MetricID,
			Weight:   op.Weight,
		}
	}
	return out
}
