// Package guardrail implements the guardrail alert consumer and auto-pause logic.
// When M3 detects a guardrail metric breach, it publishes a GuardrailAlert to the
// guardrail_alerts Kafka topic. This package consumes those alerts and auto-pauses
// experiments (per ADR-008) unless the experiment has guardrail_action = ALERT_ONLY.
package guardrail

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

// Alert matches the JSON schema produced by Agent-3's guardrail breach detection.
// Field names use snake_case to match both the proto JSON mapping and Agent-3's
// Go struct tags.
type Alert struct {
	ExperimentID           string    `json:"experiment_id"`
	MetricID               string    `json:"metric_id"`
	VariantID              string    `json:"variant_id"`
	CurrentValue           float64   `json:"current_value"`
	Threshold              float64   `json:"threshold"`
	ConsecutiveBreachCount int       `json:"consecutive_breach_count"`
	DetectedAt             time.Time `json:"detected_at"`
}

// ProcessResult describes what action was taken for an alert.
type ProcessResult int

const (
	ResultSkipped   ProcessResult = iota // experiment not found or not RUNNING
	ResultAlertOnly                      // logged alert, no pause (ALERT_ONLY)
	ResultPaused                         // auto-paused experiment (AUTO_PAUSE)
)

// Processor handles guardrail alerts by checking the experiment's guardrail_action
// and either auto-pausing (default) or just logging (ALERT_ONLY).
type Processor struct {
	store    *store.ExperimentStore
	audit    *store.AuditStore
	notifier *streaming.Notifier
}

// NewProcessor creates a new alert processor.
func NewProcessor(es *store.ExperimentStore, as *store.AuditStore, n *streaming.Notifier) *Processor {
	return &Processor{store: es, audit: as, notifier: n}
}

// ProcessAlert handles a single guardrail alert. Returns what action was taken.
func (p *Processor) ProcessAlert(ctx context.Context, alert Alert) (ProcessResult, error) {
	log := slog.With(
		"experiment_id", alert.ExperimentID,
		"metric_id", alert.MetricID,
		"variant_id", alert.VariantID,
		"current_value", alert.CurrentValue,
		"threshold", alert.Threshold,
	)

	// Read experiment to check state and guardrail_action.
	exp, _, _, err := p.store.GetByID(ctx, alert.ExperimentID)
	if err != nil {
		if err == pgx.ErrNoRows {
			log.Warn("guardrail: experiment not found, skipping alert")
			return ResultSkipped, nil
		}
		return ResultSkipped, fmt.Errorf("get experiment: %w", err)
	}

	if exp.State != "RUNNING" {
		log.Info("guardrail: experiment not RUNNING, skipping alert", "state", exp.State)
		return ResultSkipped, nil
	}

	// Safety net: holdouts should never be auto-paused even if guardrail_action
	// was manually set to AUTO_PAUSE in the database. Force ALERT_ONLY behavior.
	if exp.IsCumulativeHoldout {
		log.Info("guardrail: cumulative holdout, forcing ALERT_ONLY")
		exp.GuardrailAction = "ALERT_ONLY"
	}

	details, _ := json.Marshal(map[string]any{
		"metric_id":                alert.MetricID,
		"variant_id":              alert.VariantID,
		"current_value":           alert.CurrentValue,
		"threshold":               alert.Threshold,
		"consecutive_breach_count": alert.ConsecutiveBreachCount,
		"detected_at":             alert.DetectedAt.Format(time.RFC3339),
	})

	if exp.GuardrailAction == "ALERT_ONLY" {
		// Log the alert but don't pause.
		if err := p.audit.Insert(ctx, nil, store.AuditEntry{
			ExperimentID:  alert.ExperimentID,
			Action:        "guardrail_alert",
			ActorEmail:    "system",
			PreviousState: "RUNNING",
			NewState:      "RUNNING",
			DetailsJSON:   details,
		}); err != nil {
			return ResultSkipped, fmt.Errorf("audit guardrail_alert: %w", err)
		}
		log.Info("guardrail: ALERT_ONLY — logged alert without pausing")
		return ResultAlertOnly, nil
	}

	// Default: AUTO_PAUSE — pause the experiment.
	if err := p.audit.Insert(ctx, nil, store.AuditEntry{
		ExperimentID:  alert.ExperimentID,
		Action:        "guardrail_auto_pause",
		ActorEmail:    "system",
		PreviousState: "RUNNING",
		NewState:      "RUNNING",
		DetailsJSON:   details,
	}); err != nil {
		return ResultSkipped, fmt.Errorf("audit guardrail_auto_pause: %w", err)
	}

	// Notify config stream subscribers so M1 zeros traffic for this experiment.
	if p.notifier != nil {
		p.notifier.Publish(ctx, alert.ExperimentID, "upsert")
	}

	log.Warn("guardrail: AUTO_PAUSE — experiment paused due to guardrail breach",
		"breach_count", alert.ConsecutiveBreachCount)
	return ResultPaused, nil
}
