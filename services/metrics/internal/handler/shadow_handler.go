package handler

import (
	"context"
	"database/sql"
	"fmt"
	"time"

	"connectrpc.com/connect"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/types/known/wrapperspb"

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
			OriginalValue:   nullFloat64ToDoubleValue(r.OriginalValue),
			CandidateValue:  nullFloat64ToDoubleValue(r.CandidateValue),
			DiffAbs:         nullFloat64ToDoubleValue(r.DiffAbs),
			DiffRel:         nullFloat64ToDoubleValue(r.DiffRel),
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
// Idempotency: if the run is already in a terminal state (APPROVED, REJECTED,
// or FAILED), the call returns immediately with the stored status and reason
// without re-evaluating or re-transitioning.
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

	// I5: Early-return for terminal statuses — idempotent, no re-evaluation.
	switch run.Status {
	case shadow.StatusApproved:
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "ok").Observe(time.Since(start).Seconds())
		return connect.NewResponse(&metricsv1.PromoteShadowResultResponse{
			Status:   string(shadow.StatusApproved),
			ResultId: shadowID.String(),
		}), nil
	case shadow.StatusRejected:
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "ok").Observe(time.Since(start).Seconds())
		return connect.NewResponse(&metricsv1.PromoteShadowResultResponse{
			Status: string(shadow.StatusRejected),
			Reason: run.RejectionReason,
		}), nil
	case shadow.StatusFailed:
		m3metrics.RPCTotal.WithLabelValues("PromoteShadowResult", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("PromoteShadowResult", "ok").Observe(time.Since(start).Seconds())
		return connect.NewResponse(&metricsv1.PromoteShadowResultResponse{
			Status: string(shadow.StatusFailed),
			Reason: run.RejectionReason,
		}), nil
	}
	// StatusPending and StatusRunning fall through to the evaluate-then-transition flow.

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
			// I7: distinguish CAS failure from transient store errors.
			if shadow.IsCASFailure(casErr) {
				return nil, connect.NewError(connect.CodeFailedPrecondition, casErr)
			}
			return nil, connect.NewError(connect.CodeInternal, casErr)
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
			// I7: distinguish CAS failure from transient store errors.
			if shadow.IsCASFailure(casErr) {
				return nil, connect.NewError(connect.CodeFailedPrecondition, casErr)
			}
			return nil, connect.NewError(connect.CodeInternal, casErr)
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

// nullFloat64ToDoubleValue converts a sql.NullFloat64 to a *wrapperspb.DoubleValue.
// Returns nil (proto NULL) when Valid is false, preserving the distinction between
// a genuine SQL NULL and a legitimate 0.0 value.  Callers can't tell the difference
// between NULL and 0.0 using a plain proto3 double field; wrapper types solve this.
func nullFloat64ToDoubleValue(f sql.NullFloat64) *wrapperspb.DoubleValue {
	if !f.Valid {
		return nil
	}
	return &wrapperspb.DoubleValue{Value: f.Float64}
}
