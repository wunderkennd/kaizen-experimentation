// Package sequential implements the auto-conclude consumer for sequential
// experiments. When M4a detects that a sequential boundary has been crossed
// (mSPRT or GST), it publishes a BoundaryAlert to the sequential_boundary_alerts
// Kafka topic. This package consumes those alerts and auto-concludes experiments
// that are configured for sequential testing.
package sequential

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"
)

// BoundaryAlert matches the JSON schema produced by M4a when a sequential
// boundary is crossed.
type BoundaryAlert struct {
	ExperimentID   string    `json:"experiment_id"`
	MetricID       string    `json:"metric_id"`
	CurrentLook    int32     `json:"current_look"`
	AlphaSpent     float64   `json:"alpha_spent"`
	AlphaRemaining float64   `json:"alpha_remaining"`
	AdjustedPValue float64   `json:"adjusted_p_value"`
	DetectedAt     time.Time `json:"detected_at"`
}

// ProcessResult describes what action was taken for a boundary alert.
type ProcessResult int

const (
	ResultSkipped    ProcessResult = iota // not found, not RUNNING, or not sequential
	ResultConcluded                       // auto-concluded the experiment
)

// Concluder is the interface for concluding an experiment. This decouples the
// processor from the full handler, making it testable.
type Concluder interface {
	ConcludeByID(ctx context.Context, id, actor string, extraDetails map[string]any) error
}

// Processor handles sequential boundary alerts by checking if the experiment
// is RUNNING with a sequential method configured, then auto-concluding it.
type Processor struct {
	store     *store.ExperimentStore
	audit     *store.AuditStore
	notifier  *streaming.Notifier
	concluder Concluder
}

// NewProcessor creates a new boundary alert processor.
func NewProcessor(es *store.ExperimentStore, as *store.AuditStore, n *streaming.Notifier, c Concluder) *Processor {
	return &Processor{store: es, audit: as, notifier: n, concluder: c}
}

// ProcessAlert handles a single boundary alert. Returns what action was taken.
func (p *Processor) ProcessAlert(ctx context.Context, alert BoundaryAlert) (ProcessResult, error) {
	log := slog.With(
		"experiment_id", alert.ExperimentID,
		"metric_id", alert.MetricID,
		"current_look", alert.CurrentLook,
		"alpha_spent", alert.AlphaSpent,
		"adjusted_p_value", alert.AdjustedPValue,
	)

	// Read experiment to check state and sequential config.
	exp, _, _, err := p.store.GetByID(ctx, alert.ExperimentID)
	if err != nil {
		if err == pgx.ErrNoRows {
			log.Warn("sequential: experiment not found, skipping alert")
			return ResultSkipped, nil
		}
		return ResultSkipped, fmt.Errorf("get experiment: %w", err)
	}

	if exp.State != "RUNNING" {
		log.Info("sequential: experiment not RUNNING, skipping alert", "state", exp.State)
		return ResultSkipped, nil
	}

	// Only auto-conclude experiments with a sequential method configured.
	if exp.SequentialMethod == nil || *exp.SequentialMethod == "" {
		log.Warn("sequential: experiment has no sequential_method, skipping alert")
		return ResultSkipped, nil
	}

	details := map[string]any{
		"trigger":          "sequential_boundary_crossed",
		"metric_id":        alert.MetricID,
		"current_look":     alert.CurrentLook,
		"alpha_spent":      alert.AlphaSpent,
		"alpha_remaining":  alert.AlphaRemaining,
		"adjusted_p_value": alert.AdjustedPValue,
		"detected_at":      alert.DetectedAt.Format(time.RFC3339),
	}

	// Record the boundary-crossing event in audit trail.
	auditDetails, _ := json.Marshal(details)
	if err := p.audit.Insert(ctx, nil, store.AuditEntry{
		ExperimentID:  alert.ExperimentID,
		Action:        "sequential_boundary_crossed",
		ActorEmail:    "system",
		PreviousState: "RUNNING",
		NewState:      "RUNNING",
		DetailsJSON:   auditDetails,
	}); err != nil {
		return ResultSkipped, fmt.Errorf("audit sequential_boundary_crossed: %w", err)
	}

	// Auto-conclude the experiment.
	if err := p.concluder.ConcludeByID(ctx, alert.ExperimentID, "sequential_auto_conclude", details); err != nil {
		log.Error("sequential: auto-conclude failed", "error", err)
		return ResultSkipped, fmt.Errorf("auto-conclude: %w", err)
	}

	log.Info("sequential: auto-concluded experiment",
		"sequential_method", *exp.SequentialMethod,
		"look", alert.CurrentLook)
	return ResultConcluded, nil
}
