package handler

import (
	"context"
	"fmt"
	"time"

	"connectrpc.com/connect"
	"google.golang.org/protobuf/encoding/protojson"

	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"
	"github.com/google/uuid"

	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/shadow"
)

// ScheduleShadowComputation implements MetricComputationServiceHandler.
//
// ADR-026 Phase 3 (#437): accepts the original metric ID and the candidate
// MetricDefinition, serialises the candidate to JSONB, and inserts a PENDING
// shadow run via the shadow.Store.  Returns the UUID so the migration tool can
// poll with GetShadowResults and ultimately call PromoteShadowResult.
func (h *MetricsHandler) ScheduleShadowComputation(
	ctx context.Context,
	req *connect.Request[metricsv1.ScheduleShadowComputationRequest],
) (*connect.Response[metricsv1.ScheduleShadowComputationResponse], error) {
	start := time.Now()
	if h.shadowStore == nil {
		m3metrics.RPCTotal.WithLabelValues("ScheduleShadowComputation", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ScheduleShadowComputation", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeUnavailable,
			fmt.Errorf("shadow store not configured; shadow-run RPCs are not enabled in this deployment"))
	}

	if req.Msg.GetOriginalMetricId() == "" {
		m3metrics.RPCTotal.WithLabelValues("ScheduleShadowComputation", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ScheduleShadowComputation", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("original_metric_id is required"))
	}
	if req.Msg.GetCandidateMetric() == nil {
		m3metrics.RPCTotal.WithLabelValues("ScheduleShadowComputation", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ScheduleShadowComputation", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("candidate_metric is required"))
	}

	// Serialise the candidate MetricDefinition proto to JSON for JSONB storage.
	candidateJSON, err := protojson.Marshal(req.Msg.GetCandidateMetric())
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("ScheduleShadowComputation", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ScheduleShadowComputation", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("candidate_metric: marshal failed: %w", err))
	}

	shadowID, err := h.shadowStore.Schedule(ctx, req.Msg.GetOriginalMetricId(), candidateJSON)
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("ScheduleShadowComputation", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ScheduleShadowComputation", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	m3metrics.RPCTotal.WithLabelValues("ScheduleShadowComputation", "ok").Inc()
	m3metrics.RPCDuration.WithLabelValues("ScheduleShadowComputation", "ok").Observe(time.Since(start).Seconds())
	return connect.NewResponse(&metricsv1.ScheduleShadowComputationResponse{
		ShadowId: shadowID.String(),
	}), nil
}

// GetShadowResults implements MetricComputationServiceHandler.
//
// Returns the current status and all accumulated per-tuple result rows for the
// given shadow run, together with the EvaluatePromotion aggregate counters so
// the caller can decide whether to call PromoteShadowResult.
func (h *MetricsHandler) GetShadowResults(
	ctx context.Context,
	req *connect.Request[metricsv1.GetShadowResultsRequest],
) (*connect.Response[metricsv1.GetShadowResultsResponse], error) {
	start := time.Now()
	if h.shadowStore == nil {
		m3metrics.RPCTotal.WithLabelValues("GetShadowResults", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("GetShadowResults", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeUnavailable,
			fmt.Errorf("shadow store not configured; shadow-run RPCs are not enabled in this deployment"))
	}

	shadowID, err := uuid.Parse(req.Msg.GetShadowId())
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("GetShadowResults", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("GetShadowResults", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("shadow_id: invalid UUID %q: %w", req.Msg.GetShadowId(), err))
	}

	run, err := h.shadowStore.Get(ctx, shadowID)
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("GetShadowResults", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("GetShadowResults", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	if run == nil {
		m3metrics.RPCTotal.WithLabelValues("GetShadowResults", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("GetShadowResults", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeNotFound,
			fmt.Errorf("shadow run %s not found", shadowID))
	}

	rows, err := h.shadowStore.Results(ctx, shadowID)
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("GetShadowResults", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("GetShadowResults", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	_, dwt, totalDays, _ := shadow.EvaluatePromotion(rows)

	protoRows := make([]*metricsv1.ShadowResultRow, len(rows))
	for i, r := range rows {
		protoRows[i] = &metricsv1.ShadowResultRow{
			ExperimentId:    r.ExperimentID,
			VariantId:       r.VariantID,
			ComputationDate: r.ComputationDate,
			OriginalValue:   r.OriginalValue.Float64,
			CandidateValue:  r.CandidateValue.Float64,
			DiffAbs:         r.DiffAbs.Float64,
			DiffRel:         r.DiffRel.Float64,
			WithinTolerance: r.WithinTolerance,
		}
	}

	m3metrics.RPCTotal.WithLabelValues("GetShadowResults", "ok").Inc()
	m3metrics.RPCDuration.WithLabelValues("GetShadowResults", "ok").Observe(time.Since(start).Seconds())
	return connect.NewResponse(&metricsv1.GetShadowResultsResponse{
		ShadowId:            shadowID.String(),
		Status:              string(run.Status),
		Rows:                protoRows,
		DaysWithinTolerance: int32(dwt),
		TotalDays:           int32(totalDays),
	}), nil
}

// PromoteShadowResult implements MetricComputationServiceHandler.
//
// Evaluates the 7-consecutive-days gate and, if passed, atomically transitions
// the shadow run to APPROVED (or REJECTED).  If there is not yet enough data
// (StatusPending), returns the current status without mutating the row.
//
// The migration tool's "apply" subcommand inspects the response status to
// decide whether to proceed with the M5 migration.
func (h *MetricsHandler) PromoteShadowResult(
	ctx context.Context,
	req *connect.Request[metricsv1.PromoteShadowResultRequest],
) (*connect.Response[metricsv1.PromoteShadowResultResponse], error) {
	start := time.Now()
	if h.shadowStore == nil {
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeUnavailable,
			fmt.Errorf("shadow store not configured; shadow-run RPCs are not enabled in this deployment"))
	}

	shadowID, err := uuid.Parse(req.Msg.GetShadowId())
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("shadow_id: invalid UUID %q: %w", req.Msg.GetShadowId(), err))
	}

	// Verify the shadow run exists before evaluating results.  A valid UUID for
	// a non-existent row must return CodeNotFound, not a spurious PENDING status.
	run, err := h.shadowStore.Get(ctx, shadowID)
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	if run == nil {
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeNotFound,
			fmt.Errorf("shadow run %s not found", shadowID))
	}

	rows, err := h.shadowStore.Results(ctx, shadowID)
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	evalStatus, _, _, reason := shadow.EvaluatePromotion(rows)

	switch evalStatus {
	case shadow.StatusPending:
		// Not enough data yet — return current status without transitioning.
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "ok").Observe(time.Since(start).Seconds())
		return connect.NewResponse(&metricsv1.PromoteShadowResultResponse{
			Status: string(shadow.StatusPending),
			Reason: reason,
		}), nil

	case shadow.StatusApproved:
		// Try to transition from PENDING or RUNNING to APPROVED.
		casErr := h.shadowStore.Transition(ctx, shadowID, shadow.StatusPending, shadow.StatusApproved, "")
		if casErr != nil {
			// Try from RUNNING state.
			casErr = h.shadowStore.Transition(ctx, shadowID, shadow.StatusRunning, shadow.StatusApproved, "")
		}
		if casErr != nil {
			m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
			m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
			return nil, connect.NewError(connect.CodeFailedPrecondition,
				fmt.Errorf("shadow run is not in a promotable state"))
		}
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "ok").Observe(time.Since(start).Seconds())
		// ResultId is the approval token for Phase C's MigrateMetricDefinition.
		// Populated only on APPROVED; equals the shadow_id of the promoted run.
		return connect.NewResponse(&metricsv1.PromoteShadowResultResponse{
			Status:   string(shadow.StatusApproved),
			ResultId: shadowID.String(),
		}), nil

	case shadow.StatusRejected:
		// Try to transition from PENDING or RUNNING to REJECTED.
		casErr := h.shadowStore.Transition(ctx, shadowID, shadow.StatusPending, shadow.StatusRejected, reason)
		if casErr != nil {
			casErr = h.shadowStore.Transition(ctx, shadowID, shadow.StatusRunning, shadow.StatusRejected, reason)
		}
		if casErr != nil {
			m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
			m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
			return nil, connect.NewError(connect.CodeFailedPrecondition,
				fmt.Errorf("shadow run is not in a promotable state"))
		}
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "ok").Observe(time.Since(start).Seconds())
		return connect.NewResponse(&metricsv1.PromoteShadowResultResponse{
			Status: string(shadow.StatusRejected),
			Reason: reason,
		}), nil

	default:
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal,
			fmt.Errorf("unexpected evaluation status %s", evalStatus))
	}
}
