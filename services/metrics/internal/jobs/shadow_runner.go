package jobs

// shadow_runner.go — nightly shadow-run computation for ADR-026 Phase 3 (#437).
//
// runShadows is called from StandardJob.Run AFTER the regular per-metric loop
// completes.  It is intentionally isolated from the regular compute path so
// that shadow failures never halt the experiment's primary metrics.
//
// Lifecycle (resolves the B1 enum: PENDING / RUNNING / APPROVED / REJECTED / FAILED):
//
//   PENDING  →  RUNNING  (claim via CAS; skip if another M3 instance won the race)
//   RUNNING  →  PENDING  (success — back to PENDING so tomorrow's pass picks it up)
//   RUNNING  →  FAILED   (compute error — terminal; operator sees it via GetShadowResults)
//
// There is no COMPLETED state.  APPROVED / REJECTED are operator-driven terminal
// states set by PromoteShadowResult (B1 RPC) after B3 has accumulated enough
// result rows.

import (
	"context"
	"fmt"
	"log/slog"
	"strings"

	"google.golang.org/protobuf/encoding/protojson"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/shadow"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// runShadows executes all PENDING shadow runs that have not yet been computed
// for computationDate.  Errors from individual runs are absorbed (FAILED
// transition) so that a single bad shadow never blocks the regular pass.
// Only a transient error on ListNeedingComputation itself is propagated — and
// even then it is logged and treated as non-fatal by the Run caller.
func (j *StandardJob) runShadows(ctx context.Context, experimentID, computationDate string) {
	if j.shadowStore == nil {
		return
	}

	runs, err := j.shadowStore.ListNeedingComputation(ctx, experimentID, computationDate)
	if err != nil {
		// Transient store error — log and skip; the regular pass already completed.
		slog.Error("shadow: ListNeedingComputation failed (non-fatal)",
			"experiment_id", experimentID,
			"computation_date", computationDate,
			"err", err,
		)
		return
	}

	for _, run := range runs {
		j.computeOneShadow(ctx, run, experimentID, computationDate)
	}
}

// computeOneShadow claims, computes, and transitions a single shadow run.
// All errors are absorbed: CAS races log at Debug and skip; compute errors
// transition the run to FAILED and log at Warn.
func (j *StandardJob) computeOneShadow(
	ctx context.Context,
	run shadow.Run,
	experimentID, computationDate string,
) {
	shadowIDStr := run.ShadowID.String()

	// Step 1: Claim the row — PENDING → RUNNING (CAS).
	if err := j.shadowStore.Transition(ctx, run.ShadowID, shadow.StatusPending, shadow.StatusRunning, ""); err != nil {
		if shadow.IsCASFailure(err) {
			// Another M3 instance won the race — skip silently.
			slog.Debug("shadow: CAS race lost, skipping",
				"shadow_id", shadowIDStr,
				"computation_date", computationDate,
			)
			return
		}
		// Genuine store error — log, don't try to transition to FAILED (we
		// don't know the current state; best to leave the row as PENDING so
		// tomorrow's pass retries).
		slog.Error("shadow: Transition PENDING→RUNNING failed (non-fatal)",
			"shadow_id", shadowIDStr,
			"computation_date", computationDate,
			"err", err,
		)
		return
	}

	// Step 2: Unmarshal the candidate MetricDefinition.
	var candidate commonv1.MetricDefinition
	if err := protojson.Unmarshal(run.CandidateMetric, &candidate); err != nil {
		reason := fmt.Sprintf("unmarshal candidate: %v", err)
		j.failShadow(ctx, run, computationDate, reason)
		return
	}

	// Step 3: Render the candidate to Spark SQL.  experimentID is propagated
	// so that delta.exposures JOIN filters use the real experiment scope — the
	// original B2 code left ExperimentID empty which caused zero-row output.
	sql, err := j.renderShadowCandidate(experimentID, computationDate, shadowIDStr, &candidate)
	if err != nil {
		j.failShadow(ctx, run, computationDate, err.Error())
		return
	}

	// Step 4: Execute and write to delta.metric_summaries with the shadow_id as
	// the metric_id namespace (ensures no collision with original metric rows).
	result, err := j.executor.ExecuteAndWrite(ctx, sql, "delta.metric_summaries")
	if err != nil {
		j.failShadow(ctx, run, computationDate, fmt.Sprintf("execute: %v", err))
		return
	}

	// Step 5: Write a stub ResultRow to metric_shadow_run_results so that the
	// dedup gate in ListNeedingComputation prevents re-computation for this
	// (shadow_id, experimentID, computationDate) triple within the same nightly
	// pass or on accidental re-runs.  The stub has NULL diff values; B3 will
	// UPDATE them with real computed diffs.  A failure here is treated as fatal
	// for this shadow (the compute already happened, so NOT writing the stub
	// means the dedup is broken and we risk duplicate delta.metric_summaries rows).
	stubRow := shadow.ResultRow{
		ShadowID:        run.ShadowID,
		ExperimentID:    experimentID,
		ComputationDate: computationDate,
		// VariantID left empty for the stub — B3 writes per-variant rows.
		// WithinTolerance defaults to false; B3 sets the real value.
		WithinTolerance: false,
	}
	if err := j.shadowStore.InsertResult(ctx, stubRow); err != nil {
		reason := fmt.Sprintf("insert stub result: %v", err)
		j.failShadow(ctx, run, computationDate, reason)
		return
	}

	// Step 6: Log the query.
	if err := j.queryLog.Log(ctx, querylog.Entry{
		ExperimentID: experimentID,
		MetricID:     shadowIDStr,
		SQLText:      sql,
		RowCount:     result.RowCount,
		DurationMs:   result.Duration.Milliseconds(),
		JobType:      "shadow_run",
	}); err != nil {
		// Query-log failure is non-fatal for the shadow path (mirrors the regular
		// path which returns an error, but shadows must not block the pass).
		slog.Warn("shadow: query log write failed (non-fatal)",
			"shadow_id", shadowIDStr,
			"computation_date", computationDate,
			"err", err,
		)
	}

	// Step 7: Success — RUNNING → PENDING so tomorrow's pass picks it up again.
	if err := j.shadowStore.Transition(ctx, run.ShadowID, shadow.StatusRunning, shadow.StatusPending, ""); err != nil {
		// Log but do not treat as fatal; worst case the row stays RUNNING
		// (the next pass will see it as non-PENDING and skip it).
		slog.Error("shadow: Transition RUNNING→PENDING failed (non-fatal)",
			"shadow_id", shadowIDStr,
			"computation_date", computationDate,
			"err", err,
		)
		return
	}

	// Step 8 (B3): invoke the differ to write per-variant equivalence rows.
	// Only runs when differ is wired; skipping does not affect the compute path.
	// Failure is non-fatal: the compute already succeeded and the dedup stub is
	// written.  The 7-day promotion gate will either re-attempt on the next pass
	// (EvaluatePromotion returns StatusPending when < 7 days of data) or
	// eventually REJECT if no differ rows accumulate — which is the correct safe
	// behaviour (never promote with no diff evidence).
	if j.differ != nil {
		// Resolve the original metric's type so the differ can apply the correct
		// tolerance rule (COUNT/PROPORTION → exact; all others → FP tolerance).
		origMetric, cfgErr := j.config.GetMetric(run.OriginalMetricID)
		if cfgErr != nil {
			// Original metric was deleted or was never in config — log and skip.
			slog.Warn("shadow: original metric not found in config, skipping differ",
				"shadow_id", shadowIDStr,
				"original_metric_id", run.OriginalMetricID,
				"computation_date", computationDate,
				"err", cfgErr,
			)
		} else {
			if differErr := j.differ.Run(ctx, &run, experimentID, computationDate, config.TypeShortName(origMetric.Type)); differErr != nil {
				slog.Error("shadow: differ failed (non-fatal)",
					"shadow_id", shadowIDStr,
					"original_metric_id", run.OriginalMetricID,
					"computation_date", computationDate,
					"err", differErr,
				)
				// Do NOT transition to FAILED — compute succeeded.  The shadow stays
				// PENDING; tomorrow's pass will re-compute and retry the differ.
			}
		}
	}

	slog.Info("shadow: computed candidate metric",
		"shadow_id", shadowIDStr,
		"original_metric_id", run.OriginalMetricID,
		"candidate_type", candidate.GetType().String(),
		"computation_date", computationDate,
		"rows", result.RowCount,
		"duration_ms", result.Duration.Milliseconds(),
	)
}

// renderShadowCandidate converts a candidate MetricDefinition proto into a
// Spark SQL string that writes rows to delta.metric_summaries using shadowID
// as the metric_id literal (namespace isolation from the original metric's rows).
//
// Dispatch:
//   - METRICQL         → metricql.Compile via renderer.RenderMetricql
//   - FILTERED_MEAN    → renderer.RenderForType (uses FilteredMeanConfig fields)
//   - COMPOSITE        → renderer.RenderForType (uses CompositeConfig fields)
//   - WINDOWED_COUNT   → renderer.RenderForType (uses WindowedCountConfig fields)
//   - CUSTOM           → rejected (migrator never proposes CUSTOM→CUSTOM)
//   - all others       → rejected (legacy types should never appear as candidates)
func (j *StandardJob) renderShadowCandidate(
	experimentID, computationDate, shadowID string,
	candidate *commonv1.MetricDefinition,
) (string, error) {
	// Build a TemplateParams that substitutes the shadow_id for metric_id so
	// delta.metric_summaries rows are namespaced under the shadow UUID rather
	// than the original metric's ID.  ExperimentID is propagated so that the
	// delta.exposures JOIN in each SQL template is correctly scoped — without it
	// the WHERE clause would filter on experiment_id = '' and produce zero rows.
	// Each StandardJob.Run call is scoped to one experiment, so the shadow is
	// computed once per (shadow_id, experiment_id) per nightly pass; the stub
	// ResultRow written after success prevents re-computation on the same pair.
	params := spark.TemplateParams{
		ExperimentID:    experimentID,
		MetricID:        shadowID,
		ComputationDate: computationDate,
		// Propagate all candidate fields into TemplateParams.
		SourceEventType:      candidate.GetSourceEventType(),
		NumeratorEventType:   candidate.GetNumeratorEventType(),
		DenominatorEventType: candidate.GetDenominatorEventType(),
		Percentile:           candidate.GetPercentile(),
		CustomSQL:            candidate.GetCustomSql(),
		MetricqlExpression:   candidate.GetMetricqlExpression(),
	}

	// Propagate type-specific config from oneof fields.
	switch candidate.GetType() {
	case commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN:
		if fm := candidate.GetFilteredMean(); fm != nil {
			params.FilterSQL = fm.GetFilterSql()
			params.ValueColumn = fm.GetValueColumn()
		}

	case commonv1.MetricType_METRIC_TYPE_COMPOSITE:
		if comp := candidate.GetComposite(); comp != nil {
			operands := comp.GetOperands()
			sparOps := make([]spark.OperandParam, len(operands))
			for i, op := range operands {
				sparOps[i] = spark.OperandParam{
					MetricID: op.GetMetricId(),
					Weight:   op.GetWeight(),
				}
			}
			params.Operands = sparOps
			params.Operator = compositeOperatorString(comp.GetOperator())
		}

	case commonv1.MetricType_METRIC_TYPE_WINDOWED_COUNT:
		if wc := candidate.GetWindowedCount(); wc != nil {
			params.EventType = wc.GetEventType()
			params.WindowHours = wc.GetWindowHours()
		}

	case commonv1.MetricType_METRIC_TYPE_METRICQL:
		// Handled by RenderForType below.

	case commonv1.MetricType_METRIC_TYPE_CUSTOM:
		// The migrator should never produce a CUSTOM→CUSTOM shadow.
		return "", fmt.Errorf("shadow candidate cannot be CUSTOM type")

	default:
		return "", fmt.Errorf("unsupported shadow candidate type %s", candidate.GetType().String())
	}

	// Translate the proto enum to the string form expected by RenderForType.
	metricTypeStr, err := protoMetricTypeToString(candidate.GetType())
	if err != nil {
		return "", err
	}

	sql, err := j.renderer.RenderForType(metricTypeStr, params)
	if err != nil {
		return "", fmt.Errorf("render shadow candidate (type %s): %w", metricTypeStr, err)
	}
	return sql, nil
}

// failShadow transitions a shadow run from RUNNING → FAILED with the given
// reason and logs a warning.  A transition error here is also logged but not
// returned — shadow failures must not propagate into the regular pass.
func (j *StandardJob) failShadow(
	ctx context.Context,
	run shadow.Run,
	computationDate, reason string,
) {
	shadowIDStr := run.ShadowID.String()
	slog.Warn("shadow: computation failed",
		"shadow_id", shadowIDStr,
		"original_metric_id", run.OriginalMetricID,
		"computation_date", computationDate,
		"reason", reason,
	)
	if err := j.shadowStore.Transition(ctx, run.ShadowID, shadow.StatusRunning, shadow.StatusFailed, reason); err != nil {
		slog.Error("shadow: Transition RUNNING→FAILED failed (non-fatal)",
			"shadow_id", shadowIDStr,
			"computation_date", computationDate,
			"err", err,
		)
	}
}

// protoMetricTypeToString maps a proto MetricType enum to the uppercase string
// accepted by spark.SQLRenderer.RenderForType.  Returns an error for CUSTOM
// (which failShadow handles before we get here) and UNSPECIFIED.
func protoMetricTypeToString(mt commonv1.MetricType) (string, error) {
	switch mt {
	case commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN:
		return "FILTERED_MEAN", nil
	case commonv1.MetricType_METRIC_TYPE_COMPOSITE:
		return "COMPOSITE", nil
	case commonv1.MetricType_METRIC_TYPE_WINDOWED_COUNT:
		return "WINDOWED_COUNT", nil
	case commonv1.MetricType_METRIC_TYPE_METRICQL:
		return "METRICQL", nil
	// Legacy types and CUSTOM were rejected in renderShadowCandidate's switch.
	// This default covers UNSPECIFIED and any future proto enum additions.
	default:
		return "", fmt.Errorf("protoMetricTypeToString: unexpected type %s", mt.String())
	}
}

// compositeOperatorString maps a CompositeOperator proto enum to the uppercase
// string that spark.SQLRenderer.RenderForType / RenderComposite expect.
func compositeOperatorString(op commonv1.CompositeOperator) string {
	switch op {
	case commonv1.CompositeOperator_COMPOSITE_OPERATOR_ADD:
		return "ADD"
	case commonv1.CompositeOperator_COMPOSITE_OPERATOR_SUBTRACT:
		return "SUBTRACT"
	case commonv1.CompositeOperator_COMPOSITE_OPERATOR_MULTIPLY:
		return "MULTIPLY"
	case commonv1.CompositeOperator_COMPOSITE_OPERATOR_DIVIDE:
		return "DIVIDE"
	case commonv1.CompositeOperator_COMPOSITE_OPERATOR_WEIGHTED_SUM:
		return "WEIGHTED_SUM"
	default:
		// UNSPECIFIED — leave blank; RenderForType will reject it.
		return strings.ToUpper(op.String())
	}
}
