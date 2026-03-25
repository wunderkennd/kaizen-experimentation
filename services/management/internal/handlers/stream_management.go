package handlers

import (
	"context"
	"log/slog"

	"connectrpc.com/connect"

	managementv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"
)

// StreamConfigUpdates implements ExperimentManagementServiceHandler.
// It sends a snapshot of all RUNNING/PAUSED experiments on connect, then
// streams delta updates as experiments change state.
//
// This satisfies the managementv1connect.ExperimentManagementServiceHandler
// interface added in ADR-025 Phase 2 so M1 can subscribe directly to M5
// instead of routing through the assignment service.
func (s *ExperimentService) StreamConfigUpdates(
	ctx context.Context,
	req *connect.Request[managementv1.StreamConfigUpdatesRequest],
	stream *connect.ServerStream[managementv1.ConfigUpdateEvent],
) error {
	slog.Info("mgmt-stream: client connected", "last_known_version", req.Msg.GetLastKnownVersion())

	// Subscribe BEFORE sending the snapshot so changes that arrive during
	// the snapshot flush are buffered and delivered afterwards.
	ch, unsubscribe := s.notifier.Subscribe()
	defer unsubscribe()

	// Phase 1: snapshot of all RUNNING experiments.
	experiments, allVariants, allGuardrails, err := s.store.ListRunning(ctx)
	if err != nil {
		return internalError("list running experiments", err)
	}

	for i, exp := range experiments {
		proto := store.RowToExperiment(exp, allVariants[i], allGuardrails[i])
		event := &managementv1.ConfigUpdateEvent{
			Experiment: proto,
			IsDeletion: false,
			Version:    s.streamVersion.Add(1),
		}
		if err := stream.Send(event); err != nil {
			return err
		}
	}

	slog.Info("mgmt-stream: snapshot sent", "experiment_count", len(experiments))

	// Phase 2: stream deltas.
	for {
		select {
		case notif, ok := <-ch:
			if !ok {
				return nil
			}
			event, err := s.buildMgmtUpdate(ctx, notif)
			if err != nil {
				slog.Error("mgmt-stream: build update failed", "error", err, "experiment_id", notif.ExperimentID)
				continue
			}
			if err := stream.Send(event); err != nil {
				return err
			}
		case <-ctx.Done():
			slog.Info("mgmt-stream: client disconnected")
			return nil
		}
	}
}

func (s *ExperimentService) buildMgmtUpdate(ctx context.Context, notif streaming.Notification) (*managementv1.ConfigUpdateEvent, error) {
	if notif.Operation == "delete" {
		return &managementv1.ConfigUpdateEvent{
			IsDeletion: true,
			Version:    s.streamVersion.Add(1),
		}, nil
	}

	expRow, variants, guardrails, err := s.store.GetByID(ctx, notif.ExperimentID)
	if err != nil {
		return &managementv1.ConfigUpdateEvent{
			IsDeletion: true,
			Version:    s.streamVersion.Add(1),
		}, nil
	}

	if expRow.State != "RUNNING" {
		return &managementv1.ConfigUpdateEvent{
			IsDeletion: true,
			Version:    s.streamVersion.Add(1),
		}, nil
	}

	proto := store.RowToExperiment(expRow, variants, guardrails)
	return &managementv1.ConfigUpdateEvent{
		Experiment: proto,
		IsDeletion: false,
		Version:    s.streamVersion.Add(1),
	}, nil
}
