package handlers

import (
	"context"
	"encoding/json"
	"log/slog"
	"time"

	"connectrpc.com/connect"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"github.com/org/experimentation-platform/services/flags/internal/store"
	"github.com/org/experimentation-platform/services/flags/internal/telemetry"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
)

// Reconciler periodically checks promoted flags and auto-resolves them
// when their associated experiments reach a terminal state (CONCLUDED or ARCHIVED).
type Reconciler struct {
	store            store.Store
	auditStore       store.AuditStore
	managementClient managementv1connect.ExperimentManagementServiceClient
	interval         time.Duration
	defaultAction    ResolutionAction
	metrics          *telemetry.Metrics
}

// NewReconciler creates a Reconciler. If interval is 0, defaults to 60s.
// If defaultAction is empty, defaults to rollout_full.
func NewReconciler(
	s store.Store,
	a store.AuditStore,
	mc managementv1connect.ExperimentManagementServiceClient,
	interval time.Duration,
	defaultAction ResolutionAction,
) *Reconciler {
	return NewReconcilerWithMetrics(s, a, mc, interval, defaultAction, nil)
}

// NewReconcilerWithMetrics creates a Reconciler with optional metrics instrumentation.
func NewReconcilerWithMetrics(
	s store.Store,
	a store.AuditStore,
	mc managementv1connect.ExperimentManagementServiceClient,
	interval time.Duration,
	defaultAction ResolutionAction,
	m *telemetry.Metrics,
) *Reconciler {
	if interval == 0 {
		interval = 60 * time.Second
	}
	if defaultAction == "" {
		defaultAction = ResolutionRolloutFull
	}
	return &Reconciler{
		store:            s,
		auditStore:       a,
		managementClient: mc,
		interval:         interval,
		defaultAction:    defaultAction,
		metrics:          m,
	}
}

// Start runs the reconciliation loop. It blocks until ctx is cancelled.
func (r *Reconciler) Start(ctx context.Context) {
	if r.managementClient == nil {
		slog.Info("reconciler: no management client configured, skipping")
		return
	}

	slog.Info("reconciler: starting", "interval", r.interval, "default_action", string(r.defaultAction))
	ticker := time.NewTicker(r.interval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			r.reconcile(ctx)
		}
	}
}

// RunOnce performs a single reconciliation pass (useful for testing).
func (r *Reconciler) RunOnce(ctx context.Context) {
	r.reconcile(ctx)
}

// reconcile performs a single reconciliation pass.
func (r *Reconciler) reconcile(ctx context.Context) {
	start := time.Now()

	flags, err := r.store.GetPromotedFlags(ctx)
	if err != nil {
		slog.Error("reconciler: get promoted flags", "error", err)
		if r.metrics != nil {
			r.metrics.ReconcilerDuration.Record(ctx, time.Since(start).Seconds())
		}
		return
	}

	for _, f := range flags {
		if !f.ResolvedAt.IsZero() {
			if r.metrics != nil {
				r.metrics.ReconcilerRunsTotal.Add(ctx, 1, metric.WithAttributes(
					attribute.String("action", "skip"),
				))
			}
			continue
		}

		r.reconcileFlag(ctx, f)
	}

	if r.metrics != nil {
		r.metrics.ReconcilerDuration.Record(ctx, time.Since(start).Seconds())
	}
}

func (r *Reconciler) reconcileFlag(ctx context.Context, f *store.Flag) {
	resp, err := r.managementClient.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: f.PromotedExperimentID,
	}))
	if err != nil {
		slog.Error("reconciler: get experiment", "error", err, "flag_id", f.FlagID, "experiment_id", f.PromotedExperimentID)
		if r.metrics != nil {
			r.metrics.ReconcilerRunsTotal.Add(ctx, 1, metric.WithAttributes(
				attribute.String("action", "error"),
			))
		}
		return
	}

	state := resp.Msg.GetState()
	if state != commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED &&
		state != commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED {
		if r.metrics != nil {
			r.metrics.ReconcilerRunsTotal.Add(ctx, 1, metric.WithAttributes(
				attribute.String("action", "keep"),
			))
		}
		return
	}

	previous := *f
	switch r.defaultAction {
	case ResolutionRolloutFull:
		f.RolloutPercentage = 1.0
		f.Enabled = true
	case ResolutionRollback:
		f.RolloutPercentage = 0.0
		f.Enabled = false
	case ResolutionKeep:
		// No change.
	}

	f.ResolvedAt = time.Now()

	if r.defaultAction != ResolutionKeep {
		if _, err := r.store.UpdateFlag(ctx, f); err != nil {
			slog.Error("reconciler: update flag", "error", err, "flag_id", f.FlagID)
			if r.metrics != nil {
				r.metrics.ReconcilerRunsTotal.Add(ctx, 1, metric.WithAttributes(
					attribute.String("action", "error"),
				))
			}
			return
		}
	}

	if r.metrics != nil {
		r.metrics.ReconcilerRunsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("action", string(r.defaultAction)),
		))
	}

	r.recordAudit(ctx, f.FlagID, "auto_resolve_experiment", &previous, f)

	slog.Info("reconciler: auto-resolved flag",
		"flag_id", f.FlagID,
		"experiment_id", f.PromotedExperimentID,
		"action", string(r.defaultAction),
		"experiment_state", state.String(),
	)
}

func (r *Reconciler) recordAudit(ctx context.Context, flagID, action string, previous, current *store.Flag) {
	if r.auditStore == nil {
		return
	}

	entry := &store.AuditEntry{
		FlagID:     flagID,
		Action:     action,
		ActorEmail: "system/reconciler",
	}

	if previous != nil {
		if data, err := json.Marshal(flagSnapshot(previous)); err == nil {
			entry.PreviousValue = data
		}
	}
	if current != nil {
		if data, err := json.Marshal(flagSnapshot(current)); err == nil {
			entry.NewValue = data
		}
	}

	if err := r.auditStore.RecordAudit(ctx, entry); err != nil {
		slog.Error("reconciler: record audit", "error", err, "flag_id", flagID)
	}
}
