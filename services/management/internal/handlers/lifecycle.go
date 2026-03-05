package handlers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/allocation"
	"github.com/org/experimentation-platform/services/management/internal/store"
)

// rollbackToDraft attempts to roll back an experiment from STARTING to DRAFT,
// logging any errors that occur during rollback.
func (s *ExperimentService) rollbackToDraft(ctx context.Context, id, reason string) {
	txRb, rbErr := s.store.BeginTx(ctx)
	if rbErr != nil {
		slog.Error("rollback: begin tx", "error", rbErr)
		return
	}
	if _, err := s.store.TransitionState(ctx, txRb, id, "STARTING", "DRAFT", ""); err != nil {
		txRb.Rollback(ctx)
		slog.Error("rollback: transition to DRAFT", "error", err)
		return
	}
	details, _ := json.Marshal(map[string]string{"reason": reason})
	s.audit.Insert(ctx, txRb, store.AuditEntry{
		ExperimentID:  id,
		Action:        "start_failed",
		ActorEmail:    "system",
		PreviousState: "STARTING",
		NewState:      "DRAFT",
		DetailsJSON:   details,
	})
	if err := txRb.Commit(ctx); err != nil {
		slog.Error("rollback: commit", "error", err)
	}
}

// StartExperiment transitions DRAFT -> STARTING -> RUNNING with bucket allocation
// and audit trail entries. Both transitions are audited. If allocation fails in
// STARTING, rolls back to DRAFT.
func (s *ExperimentService) StartExperiment(
	ctx context.Context,
	req *connect.Request[mgmtv1.StartExperimentRequest],
) (*connect.Response[commonv1.Experiment], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	// Transition 1: DRAFT -> STARTING
	tx1, err := s.store.BeginTx(ctx)
	if err != nil {
		return nil, internalError("begin tx1", err)
	}
	defer tx1.Rollback(ctx)

	_, err = s.store.TransitionState(ctx, tx1, id, "DRAFT", "STARTING", "")
	if err != nil {
		tx1.Rollback(ctx)
		return nil, preconditionError("experiment must be in DRAFT state to start")
	}

	if err := s.audit.Insert(ctx, tx1, store.AuditEntry{
		ExperimentID:  id,
		Action:        "start",
		ActorEmail:    "system",
		PreviousState: "DRAFT",
		NewState:      "STARTING",
		DetailsJSON:   json.RawMessage(`{"phase":"validation_begin"}`),
	}); err != nil {
		return nil, internalError("audit start-1", err)
	}

	if err := tx1.Commit(ctx); err != nil {
		return nil, internalError("commit tx1", err)
	}

	// Validate that all referenced metrics exist before allocating buckets.
	if err := s.validateMetricsForStart(ctx, id); err != nil {
		s.rollbackToDraft(ctx, id, "metric validation failed")
		return nil, err
	}

	// Validate type-specific config references (e.g., bandit reward_metric_id).
	if err := s.validateTypeConfigForStart(ctx, id); err != nil {
		s.rollbackToDraft(ctx, id, "type config validation failed")
		return nil, err
	}
	slog.Info("starting experiment: validation passed", "id", id)

	// Transition 2: STARTING -> RUNNING (with bucket allocation)
	tx2, err := s.store.BeginTx(ctx)
	if err != nil {
		return nil, internalError("begin tx2", err)
	}
	defer tx2.Rollback(ctx)

	// Re-read experiment under lock to get layer_id and type_config.
	expRow, err := s.store.GetByIDForUpdate(ctx, tx2, id)
	if err != nil {
		tx2.Rollback(ctx) // release FOR UPDATE lock before rollback to avoid self-deadlock
		s.rollbackToDraft(ctx, id, "failed to read experiment")
		return nil, internalError("read experiment for allocation", err)
	}

	// Lock the layer to serialize allocation attempts.
	layer, err := s.layers.GetLayerByIDForUpdate(ctx, tx2, expRow.LayerID)
	if err != nil {
		tx2.Rollback(ctx)
		s.rollbackToDraft(ctx, id, "layer not found")
		return nil, internalError("lock layer", err)
	}

	// Get occupied ranges (active + cooling down).
	activeAllocs, err := s.layers.GetActiveAllocations(ctx, tx2, layer.LayerID)
	if err != nil {
		tx2.Rollback(ctx)
		s.rollbackToDraft(ctx, id, "failed to read allocations")
		return nil, internalError("get active allocations", err)
	}

	// Extract traffic_percentage from type_config (default 1.0 = 100%).
	trafficPct := extractTrafficPercentage(expRow.TypeConfig)

	bucketsNeeded := allocation.BucketsFromPercentage(layer.TotalBuckets, trafficPct)

	// Build occupied ranges for the allocator.
	occupied := make([]allocation.BucketRange, len(activeAllocs))
	for i, a := range activeAllocs {
		occupied[i] = allocation.BucketRange{Start: a.StartBucket, End: a.EndBucket}
	}

	// Find a contiguous gap.
	gap, err := allocation.FindContiguousGap(layer.TotalBuckets, occupied, bucketsNeeded)
	if err != nil {
		tx2.Rollback(ctx) // release FOR UPDATE locks before rollback to avoid self-deadlock
		if errors.Is(err, allocation.ErrInsufficientCapacity) {
			s.rollbackToDraft(ctx, id, "insufficient bucket capacity")
			return nil, connect.NewError(connect.CodeResourceExhausted,
				fmt.Errorf("layer %q has insufficient bucket capacity (need %d contiguous buckets)", layer.Name, bucketsNeeded))
		}
		s.rollbackToDraft(ctx, id, "allocation error")
		return nil, internalError("find bucket gap", err)
	}

	// Insert the allocation.
	alloc, err := s.layers.InsertAllocation(ctx, tx2, store.AllocationRow{
		LayerID:      layer.LayerID,
		ExperimentID: id,
		StartBucket:  gap.Start,
		EndBucket:    gap.End,
	})
	if err != nil {
		tx2.Rollback(ctx)
		s.rollbackToDraft(ctx, id, "failed to insert allocation")
		return nil, internalError("insert allocation", err)
	}

	// Transition to RUNNING.
	_, err = s.store.TransitionState(ctx, tx2, id, "STARTING", "RUNNING", "started_at")
	if err != nil {
		tx2.Rollback(ctx)
		s.rollbackToDraft(ctx, id, "transition_failed")
		return nil, internalError("transition to RUNNING", err)
	}

	allocDetails, _ := json.Marshal(map[string]any{
		"phase":             "activated",
		"allocation_id":     alloc.AllocationID,
		"start_bucket":      alloc.StartBucket,
		"end_bucket":        alloc.EndBucket,
		"traffic_pct":       trafficPct,
		"buckets_allocated": bucketsNeeded,
	})
	if err := s.audit.Insert(ctx, tx2, store.AuditEntry{
		ExperimentID:  id,
		Action:        "start",
		ActorEmail:    "system",
		PreviousState: "STARTING",
		NewState:      "RUNNING",
		DetailsJSON:   allocDetails,
	}); err != nil {
		return nil, internalError("audit start-2", err)
	}

	if err := tx2.Commit(ctx); err != nil {
		return nil, internalError("commit tx2", err)
	}

	// Notify subscribers that this experiment is now RUNNING.
	if s.notifier != nil {
		s.notifier.Publish(ctx, id, "upsert")
	}

	// Read back full experiment.
	finalRow, variants, guardrails, err := s.store.GetByID(ctx, id)
	if err != nil {
		return nil, internalError("read back experiment", err)
	}

	slog.Info("experiment started", "id", id,
		"buckets", fmt.Sprintf("[%d-%d]", alloc.StartBucket, alloc.EndBucket))
	return connect.NewResponse(store.RowToExperiment(finalRow, variants, guardrails)), nil
}

// ConcludeExperiment transitions RUNNING -> CONCLUDING -> CONCLUDED
// and releases the experiment's bucket allocation with a cooldown period.
func (s *ExperimentService) ConcludeExperiment(
	ctx context.Context,
	req *connect.Request[mgmtv1.ConcludeExperimentRequest],
) (*connect.Response[commonv1.Experiment], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	// Transition 1: RUNNING -> CONCLUDING
	tx1, err := s.store.BeginTx(ctx)
	if err != nil {
		return nil, internalError("begin tx1", err)
	}
	defer tx1.Rollback(ctx)

	_, err = s.store.TransitionState(ctx, tx1, id, "RUNNING", "CONCLUDING", "")
	if err != nil {
		tx1.Rollback(ctx)
		return nil, preconditionError("experiment must be in RUNNING state to conclude")
	}

	if err := s.audit.Insert(ctx, tx1, store.AuditEntry{
		ExperimentID:  id,
		Action:        "conclude",
		ActorEmail:    "system",
		PreviousState: "RUNNING",
		NewState:      "CONCLUDING",
		DetailsJSON:   json.RawMessage(`{"phase":"final_analysis_begin"}`),
	}); err != nil {
		return nil, internalError("audit conclude-1", err)
	}

	if err := tx1.Commit(ctx); err != nil {
		return nil, internalError("commit tx1", err)
	}

	// TODO(M4a): Trigger final analysis. For now, transition synchronously.
	slog.Info("concluding experiment: final analysis mocked", "id", id)

	// Transition 2: CONCLUDING -> CONCLUDED (with allocation release)
	tx2, err := s.store.BeginTx(ctx)
	if err != nil {
		return nil, internalError("begin tx2", err)
	}
	defer tx2.Rollback(ctx)

	// Read experiment to get layer_id.
	expRow, err := s.store.GetByIDForUpdate(ctx, tx2, id)
	if err != nil {
		return nil, internalError("read experiment for conclude", err)
	}

	// Get layer to read cooldown seconds.
	layer, err := s.layers.GetLayerByIDForUpdate(ctx, tx2, expRow.LayerID)
	if err != nil {
		return nil, internalError("read layer for conclude", err)
	}

	// Release the allocation with cooldown.
	if err := s.layers.ReleaseAllocation(ctx, tx2, id, layer.BucketReuseCooldownSeconds); err != nil {
		return nil, internalError("release allocation", err)
	}

	concluded, err := s.store.TransitionState(ctx, tx2, id, "CONCLUDING", "CONCLUDED", "concluded_at")
	if err != nil {
		return nil, internalError("transition to CONCLUDED", err)
	}

	releaseDetails, _ := json.Marshal(map[string]any{
		"phase":               "analysis_complete",
		"allocation_released": true,
		"cooldown_seconds":    layer.BucketReuseCooldownSeconds,
	})
	if err := s.audit.Insert(ctx, tx2, store.AuditEntry{
		ExperimentID:  id,
		Action:        "conclude",
		ActorEmail:    "system",
		PreviousState: "CONCLUDING",
		NewState:      "CONCLUDED",
		DetailsJSON:   releaseDetails,
	}); err != nil {
		return nil, internalError("audit conclude-2", err)
	}

	if err := tx2.Commit(ctx); err != nil {
		return nil, internalError("commit tx2", err)
	}

	// Notify subscribers that this experiment is no longer RUNNING.
	if s.notifier != nil {
		s.notifier.Publish(ctx, id, "delete")
	}

	finalRow, variants, guardrails, err := s.store.GetByID(ctx, concluded.ExperimentID)
	if err != nil {
		return nil, internalError("read back experiment", err)
	}

	slog.Info("experiment concluded", "id", id)
	return connect.NewResponse(store.RowToExperiment(finalRow, variants, guardrails)), nil
}

// ArchiveExperiment transitions CONCLUDED -> ARCHIVED.
func (s *ExperimentService) ArchiveExperiment(
	ctx context.Context,
	req *connect.Request[mgmtv1.ArchiveExperimentRequest],
) (*connect.Response[commonv1.Experiment], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	tx, err := s.store.BeginTx(ctx)
	if err != nil {
		return nil, internalError("begin tx", err)
	}
	defer tx.Rollback(ctx)

	archived, err := s.store.TransitionState(ctx, tx, id, "CONCLUDED", "ARCHIVED", "archived_at")
	if err != nil {
		tx.Rollback(ctx)
		return nil, preconditionError("experiment must be in CONCLUDED state to archive")
	}

	if err := s.audit.Insert(ctx, tx, store.AuditEntry{
		ExperimentID:  id,
		Action:        "archive",
		ActorEmail:    "system",
		PreviousState: "CONCLUDED",
		NewState:      "ARCHIVED",
	}); err != nil {
		return nil, internalError("audit archive", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, internalError("commit tx", err)
	}

	// Notify subscribers that this experiment is archived.
	if s.notifier != nil {
		s.notifier.Publish(ctx, id, "delete")
	}

	expRow, variants, guardrails, err := s.store.GetByID(ctx, archived.ExperimentID)
	if err != nil {
		return nil, internalError("read back experiment", err)
	}

	slog.Info("experiment archived", "id", id)
	return connect.NewResponse(store.RowToExperiment(expRow, variants, guardrails)), nil
}

// PauseExperiment records a pause event. The experiment stays in RUNNING state
// (RUNNING->RUNNING is valid per ADR-005). Traffic zeroing deferred to M1.23.
func (s *ExperimentService) PauseExperiment(
	ctx context.Context,
	req *connect.Request[mgmtv1.PauseExperimentRequest],
) (*connect.Response[commonv1.Experiment], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	expRow, variants, guardrails, err := s.store.GetByID(ctx, id)
	if err != nil {
		return nil, wrapDBError(err, "experiment", id)
	}
	if expRow.State != "RUNNING" {
		return nil, preconditionError("experiment must be in RUNNING state to pause")
	}

	action := "pause"
	if req.Msg.GetIsGuardrailAutoPause() {
		action = "guardrail_auto_pause"
	}

	details, _ := json.Marshal(map[string]string{"reason": req.Msg.GetReason()})

	if err := s.audit.Insert(ctx, nil, store.AuditEntry{
		ExperimentID:  id,
		Action:        action,
		ActorEmail:    "system",
		PreviousState: "RUNNING",
		NewState:      "RUNNING",
		DetailsJSON:   details,
	}); err != nil {
		return nil, internalError("audit pause", err)
	}

	// Notify subscribers of the pause (traffic config may have changed).
	if s.notifier != nil {
		s.notifier.Publish(ctx, id, "upsert")
	}

	slog.Info("experiment paused", "id", id, "reason", req.Msg.GetReason())
	return connect.NewResponse(store.RowToExperiment(expRow, variants, guardrails)), nil
}

// ResumeExperiment records a resume event. Experiment stays in RUNNING state.
func (s *ExperimentService) ResumeExperiment(
	ctx context.Context,
	req *connect.Request[mgmtv1.ResumeExperimentRequest],
) (*connect.Response[commonv1.Experiment], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	expRow, variants, guardrails, err := s.store.GetByID(ctx, id)
	if err != nil {
		return nil, wrapDBError(err, "experiment", id)
	}
	if expRow.State != "RUNNING" {
		return nil, preconditionError("experiment must be in RUNNING state to resume")
	}

	if err := s.audit.Insert(ctx, nil, store.AuditEntry{
		ExperimentID:  id,
		Action:        "resume",
		ActorEmail:    "system",
		PreviousState: "RUNNING",
		NewState:      "RUNNING",
	}); err != nil {
		return nil, internalError("audit resume", err)
	}

	// Notify subscribers of the resume.
	if s.notifier != nil {
		s.notifier.Publish(ctx, id, "upsert")
	}

	slog.Info("experiment resumed", "id", id)
	return connect.NewResponse(store.RowToExperiment(expRow, variants, guardrails)), nil
}

// validateMetricsForStart checks that the experiment's primary, secondary, and
// guardrail metrics all exist in the metric_definitions table.
func (s *ExperimentService) validateMetricsForStart(ctx context.Context, experimentID string) error {
	expRow, _, guardrails, err := s.store.GetByID(ctx, experimentID)
	if err != nil {
		return internalError("read experiment for metric validation", err)
	}

	var metricIDs []string
	metricIDs = append(metricIDs, expRow.PrimaryMetricID)
	metricIDs = append(metricIDs, expRow.SecondaryMetricIDs...)
	for _, g := range guardrails {
		metricIDs = append(metricIDs, g.MetricID)
	}

	missing, err := s.metrics.ExistAll(ctx, metricIDs)
	if err != nil {
		return internalError("check metric existence", err)
	}
	if missing != "" {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("metric %q does not exist", missing))
	}
	return nil
}

// validateTypeConfigForStart validates type-specific config fields that
// reference external resources. Called during STARTING phase, after
// validateMetricsForStart.
func (s *ExperimentService) validateTypeConfigForStart(ctx context.Context, experimentID string) error {
	expRow, _, _, err := s.store.GetByID(ctx, experimentID)
	if err != nil {
		return internalError("read experiment for type config validation", err)
	}

	if expRow.Type != "MAB" && expRow.Type != "CONTEXTUAL_BANDIT" {
		return nil
	}

	// Extract reward_metric_id from bandit_config in type_config JSONB.
	rewardMetricID := extractBanditRewardMetricID(expRow.TypeConfig)
	if rewardMetricID == "" {
		return nil // Already validated at creation time.
	}

	exists, err := s.metrics.Exists(ctx, rewardMetricID)
	if err != nil {
		return internalError("check bandit reward metric existence", err)
	}
	if !exists {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("bandit_config.reward_metric_id %q does not exist in metric_definitions", rewardMetricID))
	}
	return nil
}

// extractBanditRewardMetricID reads reward_metric_id from the bandit_config
// nested in the experiment's type_config JSONB.
func extractBanditRewardMetricID(typeConfig json.RawMessage) string {
	if len(typeConfig) == 0 {
		return ""
	}
	var tc map[string]json.RawMessage
	if err := json.Unmarshal(typeConfig, &tc); err != nil {
		return ""
	}
	raw, ok := tc["bandit_config"]
	if !ok {
		return ""
	}
	var bc map[string]json.RawMessage
	if err := json.Unmarshal(raw, &bc); err != nil {
		return ""
	}
	raw, ok = bc["reward_metric_id"]
	if !ok {
		return ""
	}
	var id string
	if err := json.Unmarshal(raw, &id); err != nil {
		return ""
	}
	return id
}

// extractTrafficPercentage reads traffic_percentage from the experiment's
// type_config JSONB. Defaults to 1.0 (100% of layer) if absent or invalid.
func extractTrafficPercentage(typeConfig json.RawMessage) float64 {
	if len(typeConfig) == 0 {
		return 1.0
	}
	var tc map[string]json.RawMessage
	if err := json.Unmarshal(typeConfig, &tc); err != nil {
		return 1.0
	}
	raw, ok := tc["traffic_percentage"]
	if !ok {
		return 1.0
	}
	var pct float64
	if err := json.Unmarshal(raw, &pct); err != nil {
		return 1.0
	}
	if pct <= 0 || pct > 1.0 {
		return 1.0
	}
	return pct
}
