package handlers

import (
	"context"
	"log/slog"
	"sync/atomic"

	"connectrpc.com/connect"

	assignmentv1 "github.com/org/experimentation/gen/go/experimentation/assignment/v1"
	"github.com/org/experimentation/gen/go/experimentation/assignment/v1/assignmentv1connect"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"
)

// Compile-time check that ConfigStreamService implements the handler interface.
var _ assignmentv1connect.AssignmentServiceHandler = (*ConfigStreamService)(nil)

// ConfigStreamService implements the StreamConfigUpdates RPC from the
// AssignmentService proto. M5 serves this endpoint so M1 can subscribe
// to real-time config changes.
type ConfigStreamService struct {
	assignmentv1connect.UnimplementedAssignmentServiceHandler
	store    *store.ExperimentStore
	notifier *streaming.Notifier
	version  atomic.Int64
}

// NewConfigStreamService creates a new ConfigStreamService.
func NewConfigStreamService(es *store.ExperimentStore, n *streaming.Notifier) *ConfigStreamService {
	return &ConfigStreamService{
		store:    es,
		notifier: n,
	}
}

// StreamConfigUpdates sends a full snapshot of RUNNING experiments on connect,
// then streams delta updates as experiments change state.
func (s *ConfigStreamService) StreamConfigUpdates(
	ctx context.Context,
	req *connect.Request[assignmentv1.StreamConfigUpdatesRequest],
	stream *connect.ServerStream[assignmentv1.ConfigUpdate],
) error {
	slog.Info("stream: client connected", "last_known_version", req.Msg.GetLastKnownVersion())

	// Phase 1: Send snapshot of all RUNNING experiments.
	experiments, allVariants, allGuardrails, err := s.store.ListRunning(ctx)
	if err != nil {
		return internalError("list running experiments", err)
	}

	for i, exp := range experiments {
		proto := store.RowToExperiment(exp, allVariants[i], allGuardrails[i])
		update := &assignmentv1.ConfigUpdate{
			Experiment: proto,
			IsDeletion: false,
			Version:    s.version.Add(1),
		}
		if err := stream.Send(update); err != nil {
			return err
		}
	}

	slog.Info("stream: snapshot sent", "experiment_count", len(experiments))

	// Phase 2: Subscribe to notifications and stream deltas.
	ch, unsubscribe := s.notifier.Subscribe()
	defer unsubscribe()

	for {
		select {
		case notif, ok := <-ch:
			if !ok {
				return nil
			}
			update, err := s.buildUpdate(ctx, notif)
			if err != nil {
				slog.Error("stream: build update failed", "error", err, "experiment_id", notif.ExperimentID)
				continue
			}
			if err := stream.Send(update); err != nil {
				return err
			}
		case <-ctx.Done():
			slog.Info("stream: client disconnected")
			return nil
		}
	}
}

func (s *ConfigStreamService) buildUpdate(ctx context.Context, notif streaming.Notification) (*assignmentv1.ConfigUpdate, error) {
	if notif.Operation == "delete" {
		return &assignmentv1.ConfigUpdate{
			Experiment: nil,
			IsDeletion: true,
			Version:    s.version.Add(1),
		}, nil
	}

	expRow, variants, guardrails, err := s.store.GetByID(ctx, notif.ExperimentID)
	if err != nil {
		// Experiment not found or in terminal state — treat as deletion.
		return &assignmentv1.ConfigUpdate{
			IsDeletion: true,
			Version:    s.version.Add(1),
		}, nil
	}

	// If experiment is no longer RUNNING (concluded/archived), treat as deletion.
	if expRow.State != "RUNNING" {
		return &assignmentv1.ConfigUpdate{
			IsDeletion: true,
			Version:    s.version.Add(1),
		}, nil
	}

	proto := store.RowToExperiment(expRow, variants, guardrails)
	return &assignmentv1.ConfigUpdate{
		Experiment: proto,
		IsDeletion: false,
		Version:    s.version.Add(1),
	}, nil
}
