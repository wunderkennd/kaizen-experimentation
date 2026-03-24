package handlers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/allocation"
	"github.com/org/experimentation-platform/services/management/internal/mlrate"
	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/validation"
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
		ActorEmail:    actorFromContext(ctx),
		PreviousState: "DRAFT",
		NewState:      "STARTING",
		DetailsJSON:   json.RawMessage(`{"phase":"validation_begin"}`),
	}); err != nil {
		return nil, internalError("audit start-1", err)
	}

	if err := tx1.Commit(ctx); err != nil {
		return nil, internalError("commit tx1", err)
	}

	// ADR-015 Phase 2: emit MLRATE model training request when the experiment
	// uses SEQUENTIAL_METHOD_AVLM with a configured surrogate model. Best-effort:
	// failure to publish does not block the STARTING → RUNNING transition.
	s.maybeEmitModelTrainingRequest(ctx, id)

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
		ActorEmail:    actorFromContext(ctx),
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

	// Check if this is a cumulative holdout retirement.
	var extraDetails map[string]any
	expRow, _, _, readErr := s.store.GetByID(ctx, id)
	if readErr == nil && expRow.IsCumulativeHoldout {
		extraDetails = map[string]any{"holdout_retirement": true}
	}

	exp, err := s.concludeByID(ctx, id, actorFromContext(ctx), extraDetails)
	if err != nil {
		return nil, err
	}

	return connect.NewResponse(exp), nil
}

// concludeByID performs the full RUNNING → CONCLUDING → CONCLUDED transition
// with allocation release. Used by both the ConcludeExperiment RPC and the
// sequential auto-conclude consumer. The actor identifies who triggered the
// conclude (e.g., "system" for manual, "sequential_auto_conclude" for auto).
// extraDetails are merged into the audit trail entries.
func (s *ExperimentService) concludeByID(ctx context.Context, id, actor string, extraDetails map[string]any) (*commonv1.Experiment, error) {
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

	concludePhase1 := map[string]any{"phase": "final_analysis_begin"}
	for k, v := range extraDetails {
		concludePhase1[k] = v
	}
	phase1JSON, _ := json.Marshal(concludePhase1)

	if err := s.audit.Insert(ctx, tx1, store.AuditEntry{
		ExperimentID:  id,
		Action:        "conclude",
		ActorEmail:    actor,
		PreviousState: "RUNNING",
		NewState:      "CONCLUDING",
		DetailsJSON:   phase1JSON,
	}); err != nil {
		return nil, internalError("audit conclude-1", err)
	}

	if err := tx1.Commit(ctx); err != nil {
		return nil, internalError("commit tx1", err)
	}

	// Type-specific conclude: trigger M4a analysis, M4b policy snapshot (bandits),
	// surrogate projection flagging. All calls are best-effort/non-blocking.
	expForConclude, _, _, readErr := s.store.GetByID(ctx, id)
	if readErr == nil {
		concludeDetails := s.handleTypeSpecificConclude(ctx, expForConclude)
		if extraDetails == nil {
			extraDetails = map[string]any{}
		}
		for k, v := range concludeDetails {
			extraDetails[k] = v
		}

		// Submit primary metric e-value to the e-LOND Online FDR controller
		// (ADR-018 Phase 2). Best-effort — never blocks conclusion.
		s.submitFdrDecision(ctx, expForConclude.ExperimentID, expForConclude.PrimaryMetricID)
	}
	slog.Info("concluding experiment: type-specific conclude complete", "id", id)

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

	releaseDetails := map[string]any{
		"phase":               "analysis_complete",
		"allocation_released": true,
		"cooldown_seconds":    layer.BucketReuseCooldownSeconds,
	}
	for k, v := range extraDetails {
		releaseDetails[k] = v
	}
	releaseJSON, _ := json.Marshal(releaseDetails)
	if err := s.audit.Insert(ctx, tx2, store.AuditEntry{
		ExperimentID:  id,
		Action:        "conclude",
		ActorEmail:    actor,
		PreviousState: "CONCLUDING",
		NewState:      "CONCLUDED",
		DetailsJSON:   releaseJSON,
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

	slog.Info("experiment concluded", "id", id, "actor", actor)
	return store.RowToExperiment(finalRow, variants, guardrails), nil
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
		ActorEmail:    actorFromContext(ctx),
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
//
// Uses a transaction with GetByIDForUpdate to prevent TOCTOU race conditions
// where a concurrent ConcludeExperiment could change state between our read
// and audit insert.
func (s *ExperimentService) PauseExperiment(
	ctx context.Context,
	req *connect.Request[mgmtv1.PauseExperimentRequest],
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

	expRow, err := s.store.GetByIDForUpdate(ctx, tx, id)
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

	if err := s.audit.Insert(ctx, tx, store.AuditEntry{
		ExperimentID:  id,
		Action:        action,
		ActorEmail:    actorFromContext(ctx),
		PreviousState: "RUNNING",
		NewState:      "RUNNING",
		DetailsJSON:   details,
	}); err != nil {
		return nil, internalError("audit pause", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, internalError("commit tx", err)
	}

	// Re-read full experiment (with variants/guardrails) for response.
	fullRow, variants, guardrails, err := s.store.GetByID(ctx, id)
	if err != nil {
		return nil, internalError("read back experiment", err)
	}

	// Notify subscribers of the pause (traffic config may have changed).
	if s.notifier != nil {
		s.notifier.Publish(ctx, id, "upsert")
	}

	slog.Info("experiment paused", "id", id, "reason", req.Msg.GetReason())
	return connect.NewResponse(store.RowToExperiment(fullRow, variants, guardrails)), nil
}

// ResumeExperiment records a resume event. Experiment stays in RUNNING state.
//
// Uses a transaction with GetByIDForUpdate to prevent TOCTOU race conditions
// where a concurrent ConcludeExperiment could change state between our read
// and audit insert.
func (s *ExperimentService) ResumeExperiment(
	ctx context.Context,
	req *connect.Request[mgmtv1.ResumeExperimentRequest],
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

	expRow, err := s.store.GetByIDForUpdate(ctx, tx, id)
	if err != nil {
		return nil, wrapDBError(err, "experiment", id)
	}
	if expRow.State != "RUNNING" {
		return nil, preconditionError("experiment must be in RUNNING state to resume")
	}

	if err := s.audit.Insert(ctx, tx, store.AuditEntry{
		ExperimentID:  id,
		Action:        "resume",
		ActorEmail:    actorFromContext(ctx),
		PreviousState: "RUNNING",
		NewState:      "RUNNING",
	}); err != nil {
		return nil, internalError("audit resume", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, internalError("commit tx", err)
	}

	// Re-read full experiment (with variants/guardrails) for response.
	fullRow, variants, guardrails, err := s.store.GetByID(ctx, id)
	if err != nil {
		return nil, internalError("read back experiment", err)
	}

	// Notify subscribers of the resume.
	if s.notifier != nil {
		s.notifier.Publish(ctx, id, "upsert")
	}

	slog.Info("experiment resumed", "id", id)
	return connect.NewResponse(store.RowToExperiment(fullRow, variants, guardrails)), nil
}

// validateMetricsForStart checks that the experiment's primary, secondary, and
// guardrail metrics all exist in the metric_definitions table. Also enforces
// ADR-014: guardrail metrics must use USER or EXPERIMENT aggregation.
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

	// ADR-014: guardrail metrics must use USER or EXPERIMENT aggregation.
	for _, g := range guardrails {
		mrow, err := s.metrics.GetByID(ctx, g.MetricID)
		if err != nil {
			return internalError("fetch guardrail metric for aggregation check", err)
		}
		mDef := store.RowToMetricDefinition(mrow)
		if verr := validation.ValidateGuardrailMetricAggregation(mDef); verr != nil {
			return verr
		}
	}

	return nil
}

// validateTypeConfigForStart validates type-specific config fields that
// reference external resources. Called during STARTING phase, after
// validateMetricsForStart.
func (s *ExperimentService) validateTypeConfigForStart(ctx context.Context, experimentID string) error {
	expRow, variants, _, err := s.store.GetByID(ctx, experimentID)
	if err != nil {
		return internalError("read experiment for type config validation", err)
	}

	if expRow.Type == "PLAYBACK_QOE" {
		return s.validateQoeMetricsForStart(ctx, experimentID)
	}

	if expRow.Type == "META" {
		return s.validateMetaForStart(ctx, experimentID)
	}

	if expRow.Type == "CUMULATIVE_HOLDOUT" {
		trafficPct := extractTrafficPercentage(expRow.TypeConfig)
		if trafficPct < 0.01 || trafficPct > 0.05 {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("CUMULATIVE_HOLDOUT traffic_percentage must be between 1%% and 5%%, got %.1f%%", trafficPct*100))
		}
		return nil
	}

	if expRow.Type == "META" || expRow.Type == "SWITCHBACK" || expRow.Type == "QUASI" {
		exp := store.RowToExperiment(expRow, variants, nil)
		switch expRow.Type {
		case "META":
			if verr := validation.ValidateMetaExperimentForStart(exp); verr != nil {
				return verr
			}
		case "SWITCHBACK":
			if verr := validation.ValidateSwitchbackForStart(exp); verr != nil {
				return verr
			}
		case "QUASI":
			if verr := validation.ValidateQuasiExperimentForStart(exp); verr != nil {
				return verr
			}
		}
		return nil
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

	// ADR-014: bandit reward metric must use USER aggregation.
	mrow, err := s.metrics.GetByID(ctx, rewardMetricID)
	if err != nil {
		return internalError("fetch bandit reward metric for aggregation check", err)
	}
	mDef := store.RowToMetricDefinition(mrow)
	if verr := validation.ValidateBanditRewardMetricAggregation(mDef); verr != nil {
		return verr
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

// validateMetaForStart validates MetaExperimentConfig during the STARTING phase (ADR-013).
// Checks that variant_objectives is non-empty and all variant_ids reference declared variants.
func (s *ExperimentService) validateMetaForStart(ctx context.Context, experimentID string) error {
	expRow, variants, _, err := s.store.GetByID(ctx, experimentID)
	if err != nil {
		return internalError("read experiment for META validation", err)
	}

	objectives := extractMetaVariantObjectives(expRow.TypeConfig)
	if len(objectives) == 0 {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("META experiment %q: meta_experiment_config.variant_objectives must be non-empty", experimentID))
	}

	// Build a set of declared variant IDs.
	variantSet := make(map[string]struct{}, len(variants))
	for _, v := range variants {
		variantSet[v.VariantID] = struct{}{}
	}

	for i, vid := range objectives {
		if _, ok := variantSet[vid]; !ok {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("META experiment %q: variant_objectives[%d].variant_id %q not found in declared variants",
					experimentID, i, vid))
		}
	}
	return nil
}

// extractMetaVariantObjectives reads the ordered list of variant_ids from the
// meta_experiment_config nested in the experiment's type_config JSONB.
// Returns nil if the field is absent or malformed.
func extractMetaVariantObjectives(typeConfig json.RawMessage) []string {
	if len(typeConfig) == 0 {
		return nil
	}
	var tc map[string]json.RawMessage
	if err := json.Unmarshal(typeConfig, &tc); err != nil {
		return nil
	}
	raw, ok := tc["meta_experiment_config"]
	if !ok {
		return nil
	}
	var mc struct {
		VariantObjectives []struct {
			VariantID string `json:"variant_id"`
		} `json:"variant_objectives"`
	}
	if err := json.Unmarshal(raw, &mc); err != nil {
		return nil
	}
	ids := make([]string, 0, len(mc.VariantObjectives))
	for _, obj := range mc.VariantObjectives {
		ids = append(ids, obj.VariantID)
	}
	return ids
}

// validateQoeMetricsForStart checks that at least one metric in the experiment's
// metric set (primary, secondary, or guardrail) has is_qoe_metric = true.
func (s *ExperimentService) validateQoeMetricsForStart(ctx context.Context, experimentID string) error {
	expRow, _, guardrails, err := s.store.GetByID(ctx, experimentID)
	if err != nil {
		return internalError("read experiment for QoE validation", err)
	}

	var metricIDs []string
	metricIDs = append(metricIDs, expRow.PrimaryMetricID)
	metricIDs = append(metricIDs, expRow.SecondaryMetricIDs...)
	for _, g := range guardrails {
		metricIDs = append(metricIDs, g.MetricID)
	}

	hasQoe, err := s.metrics.AnyQoeMetric(ctx, metricIDs)
	if err != nil {
		return internalError("check QoE metric existence", err)
	}
	if !hasQoe {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("PLAYBACK_QOE experiments require at least one metric with is_qoe_metric = true"))
	}
	return nil
}

// maybeEmitModelTrainingRequest publishes a ModelTrainingRequest to Kafka when
// an experiment with SEQUENTIAL_METHOD_AVLM and a configured surrogate model
// transitions to STARTING. The publish is best-effort: failures are logged but
// do not abort the start flow.
//
// ADR-015 Phase 2 (MLRATE): M3 uses this event to train an ML-predicted
// control variate model over the 30-day pre-experiment window.
func (s *ExperimentService) maybeEmitModelTrainingRequest(ctx context.Context, experimentID string) {
	if s.modelTrainingPublisher == nil {
		return
	}

	expRow, _, _, err := s.store.GetByID(ctx, experimentID)
	if err != nil {
		slog.Warn("mlrate: failed to read experiment for training trigger",
			"experiment_id", experimentID, "error", err)
		return
	}

	seqMethod := ""
	if expRow.SequentialMethod != nil {
		seqMethod = *expRow.SequentialMethod
	}
	surrogateID := ""
	if expRow.SurrogateModelID != nil {
		surrogateID = *expRow.SurrogateModelID
	}

	if !mlrate.ShouldTrigger(seqMethod, surrogateID) {
		return
	}

	// Fetch the surrogate model to get the covariate metric ID (the metric
	// the surrogate predicts, used as the AVLM control variate).
	covariateMetricID := ""
	surModel, surErr := s.surrogates.GetByID(ctx, surrogateID)
	if surErr != nil {
		slog.Warn("mlrate: failed to read surrogate model; covariate_metric_id will be empty",
			"experiment_id", experimentID, "surrogate_model_id", surrogateID, "error", surErr)
	} else {
		covariateMetricID = surModel.TargetMetricID
	}

	published := mlrate.Emit(
		ctx, s.modelTrainingPublisher,
		experimentID, seqMethod, surrogateID,
		expRow.PrimaryMetricID, covariateMetricID,
		time.Now(),
	)

	slog.Info("mlrate: model training request",
		"experiment_id", experimentID,
		"metric_id", expRow.PrimaryMetricID,
		"covariate_metric_id", covariateMetricID,
		"surrogate_model_id", surrogateID,
		"kafka_published", published)
}
