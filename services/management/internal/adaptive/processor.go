// Package adaptive implements the adaptive sample size (ADR-020) interim
// trigger and zone-classification response logic for M5.
//
// # Architecture
//
// Statistical computation lives entirely in experimentation-stats (Rust).
// M5 delegates to M4a via the ConditionalPowerClient interface, which wraps
// the ComputeConditionalPower RPC (to be implemented by Agent-4). A no-op
// stub implementation is provided for tests and deployments that do not yet
// have M4a wired up.
//
// Zone responses:
//   - Favorable  → no action; record in audit table.
//   - Promising  → update experiment's recommended_n_max in type_config,
//     insert adaptive_sample_size_audit row with extended=true.
//   - Futile     → send early-termination recommendation to experiment owner
//     via audit trail; does NOT auto-conclude.
package adaptive

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/org/experimentation-platform/services/management/internal/store"
)

// ---------------------------------------------------------------------------
// ConditionalPowerClient — M4a delegation interface
// ---------------------------------------------------------------------------

// ConditionalPowerRequest is sent to M4a to compute conditional power using
// the blinded variance estimate and current effect size.
type ConditionalPowerRequest struct {
	ExperimentID    string  `json:"experiment_id"`
	ObservedEffect  float64 `json:"observed_effect"`
	BlinderVariance float64 `json:"blinded_variance"` // σ²_B
	NMaxPerArm      float64 `json:"n_max_per_arm"`
	Alpha           float64 `json:"alpha"`
}

// ConditionalPowerResponse holds the zone classification returned by M4a.
type ConditionalPowerResponse struct {
	ConditionalPower float64 `json:"conditional_power"`
	Zone             Zone    `json:"zone"` // "favorable" | "promising" | "futile"
	RecommendedNMax  float64 `json:"recommended_n_max,omitempty"`
	BlinderVariance  float64 `json:"blinded_variance"`
}

// Zone is the string tag used in the database and audit trail.
type Zone string

const (
	ZoneFavorable Zone = "favorable"
	ZonePromising Zone = "promising"
	ZoneFutile    Zone = "futile"
)

// ConditionalPowerClient abstracts the M4a RPC call so that M5 tests can
// inject a mock without spinning up a real gRPC server.
type ConditionalPowerClient interface {
	// ComputeZone requests conditional power and zone classification from M4a.
	// Returns an error when M4a is unavailable; callers should treat that as a
	// soft failure (skip the interim, retry next cycle).
	ComputeZone(ctx context.Context, req ConditionalPowerRequest) (ConditionalPowerResponse, error)
}

// ---------------------------------------------------------------------------
// Processor
// ---------------------------------------------------------------------------

// InterimTrigger describes a single adaptive-N interim check.
type InterimTrigger struct {
	ExperimentID    string
	NInterimPerArm  float64
	NMaxPerArm      float64
	Alpha           float64
	InterimFraction float64
}

// ProcessResult summarises the action taken for one interim trigger.
type ProcessResult struct {
	ExperimentID     string
	Zone             Zone
	ConditionalPower float64
	RecommendedNMax  float64
	Extended         bool
	Skipped          bool
	SkipReason       string
}

// Processor executes the zone-classification and side-effects for one
// adaptive-N interim trigger.
type Processor struct {
	pool   *pgxpool.Pool
	store  *store.ExperimentStore
	audit  *store.AuditStore
	client ConditionalPowerClient
}

// NewProcessor creates a Processor.
func NewProcessor(
	pool *pgxpool.Pool,
	es *store.ExperimentStore,
	as *store.AuditStore,
	client ConditionalPowerClient,
) *Processor {
	return &Processor{pool: pool, store: es, audit: as, client: client}
}

// Process runs the full interim analysis for a single trigger.
//
// Steps:
//  1. Read current metric summary from the experiment (blinded variance and
//     effect estimate must be pre-computed by M3/M4a and stored in type_config
//     or fetched from metric_results; this implementation reads them from the
//     trigger payload for now).
//  2. Delegate to M4a (ConditionalPowerClient) for zone classification.
//  3. Act on zone: record audit row, optionally extend experiment.
func (p *Processor) Process(ctx context.Context, trigger InterimTrigger, observedEffect, blindedVariance float64) (ProcessResult, error) {
	log := slog.With(
		"experiment_id", trigger.ExperimentID,
		"n_interim", trigger.NInterimPerArm,
		"n_max", trigger.NMaxPerArm,
	)

	// Guard: experiment must still be RUNNING.
	exp, _, _, err := p.store.GetByID(ctx, trigger.ExperimentID)
	if err != nil {
		return skipResult(trigger.ExperimentID, "experiment not found"), nil
	}
	if exp.State != "RUNNING" {
		log.Info("adaptive-N: experiment not RUNNING, skipping", "state", exp.State)
		return skipResult(trigger.ExperimentID, "not RUNNING"), nil
	}

	// Delegate to M4a for conditional power and zone classification.
	resp, err := p.client.ComputeZone(ctx, ConditionalPowerRequest{
		ExperimentID:    trigger.ExperimentID,
		ObservedEffect:  observedEffect,
		BlinderVariance: blindedVariance,
		NMaxPerArm:      trigger.NMaxPerArm,
		Alpha:           trigger.Alpha,
	})
	if err != nil {
		log.Error("adaptive-N: M4a delegation failed, skipping", "error", err)
		return skipResult(trigger.ExperimentID, fmt.Sprintf("M4a unavailable: %v", err)), nil
	}

	log.Info("adaptive-N: zone classified",
		"zone", resp.Zone,
		"conditional_power", resp.ConditionalPower,
		"recommended_n_max", resp.RecommendedNMax)

	result := ProcessResult{
		ExperimentID:     trigger.ExperimentID,
		Zone:             resp.Zone,
		ConditionalPower: resp.ConditionalPower,
		RecommendedNMax:  resp.RecommendedNMax,
	}

	// Insert the audit record.
	if dbErr := p.insertAuditRow(ctx, trigger, observedEffect, blindedVariance, resp); dbErr != nil {
		log.Error("adaptive-N: failed to insert audit row", "error", dbErr)
		// Non-fatal: continue with zone actions.
	}

	switch resp.Zone {
	case ZoneFavorable:
		if auditErr := p.recordAuditTrail(ctx, trigger.ExperimentID, "adaptive_n_favorable", nil); auditErr != nil {
			log.Warn("adaptive-N: audit trail insert failed", "error", auditErr)
		}

	case ZonePromising:
		extended, extErr := p.extendExperiment(ctx, &exp, resp.RecommendedNMax)
		if extErr != nil {
			log.Error("adaptive-N: experiment extension failed", "error", extErr)
		}
		result.Extended = extended
		details := map[string]any{
			"zone":              "promising",
			"conditional_power": resp.ConditionalPower,
			"recommended_n_max": resp.RecommendedNMax,
			"extended":          extended,
		}
		if auditErr := p.recordAuditTrail(ctx, trigger.ExperimentID, "adaptive_n_promising", details); auditErr != nil {
			log.Warn("adaptive-N: audit trail insert failed", "error", auditErr)
		}

	case ZoneFutile:
		details := map[string]any{
			"zone":              "futile",
			"conditional_power": resp.ConditionalPower,
			"recommendation":    "early_termination",
			"message":           "Conditional power below 30%. Consider stopping this experiment.",
		}
		if auditErr := p.recordAuditTrail(ctx, trigger.ExperimentID, "adaptive_n_futile", details); auditErr != nil {
			log.Warn("adaptive-N: audit trail insert failed", "error", auditErr)
		}
	}

	return result, nil
}

// extendExperiment records the recommended extension in the experiment's
// type_config under the "adaptive_n_extension" key.
//
// Returns true if the extension was recorded successfully.
func (p *Processor) extendExperiment(ctx context.Context, exp *store.ExperimentRow, recommendedNMax float64) (bool, error) {
	if recommendedNMax <= 0 {
		return false, nil
	}

	// Parse existing type_config.
	var tc map[string]json.RawMessage
	if len(exp.TypeConfig) > 0 {
		if err := json.Unmarshal(exp.TypeConfig, &tc); err != nil {
			return false, fmt.Errorf("unmarshal type_config: %w", err)
		}
	}
	if tc == nil {
		tc = make(map[string]json.RawMessage)
	}

	// Record extension under "adaptive_n_extension" key.
	ext := map[string]any{
		"recommended_n_max": recommendedNMax,
		"extended_at":       time.Now().UTC().Format(time.RFC3339),
	}
	extJSON, err := json.Marshal(ext)
	if err != nil {
		return false, fmt.Errorf("marshal extension: %w", err)
	}
	tc["adaptive_n_extension"] = extJSON

	newConfig, err := json.Marshal(tc)
	if err != nil {
		return false, fmt.Errorf("marshal type_config: %w", err)
	}

	_, err = p.pool.Exec(ctx,
		`UPDATE experiments SET type_config = $1, updated_at = NOW() WHERE experiment_id = $2`,
		newConfig, exp.ExperimentID,
	)
	if err != nil {
		return false, fmt.Errorf("update type_config: %w", err)
	}

	// Mark the audit row as extended.
	_, _ = p.pool.Exec(ctx,
		`UPDATE adaptive_sample_size_audit SET extended = TRUE
		 WHERE experiment_id = $1
		 ORDER BY triggered_at DESC LIMIT 1`,
		exp.ExperimentID,
	)

	return true, nil
}

// insertAuditRow writes a row to the adaptive_sample_size_audit table.
func (p *Processor) insertAuditRow(
	ctx context.Context,
	trigger InterimTrigger,
	observedEffect, blindedVariance float64,
	resp ConditionalPowerResponse,
) error {
	var recNMax *float64
	if resp.RecommendedNMax > 0 {
		recNMax = &resp.RecommendedNMax
	}

	_, err := p.pool.Exec(ctx, `
		INSERT INTO adaptive_sample_size_audit
			(experiment_id, interim_fraction, n_interim_per_arm, n_max_per_arm,
			 observed_effect, blinded_variance, conditional_power, zone,
			 recommended_n_max, actor)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'adaptive_n_scheduler')`,
		trigger.ExperimentID,
		trigger.InterimFraction,
		trigger.NInterimPerArm,
		trigger.NMaxPerArm,
		observedEffect,
		blindedVariance,
		resp.ConditionalPower,
		string(resp.Zone),
		recNMax,
	)
	return err
}

// recordAuditTrail writes an entry to the existing audit_trail table.
func (p *Processor) recordAuditTrail(ctx context.Context, experimentID, action string, details map[string]any) error {
	detailsJSON, _ := json.Marshal(details)
	return p.audit.Insert(ctx, nil, store.AuditEntry{
		ExperimentID:  experimentID,
		Action:        action,
		ActorEmail:    "adaptive_n_scheduler",
		PreviousState: "RUNNING",
		NewState:      "RUNNING",
		DetailsJSON:   detailsJSON,
	})
}

func skipResult(experimentID, reason string) ProcessResult {
	return ProcessResult{
		ExperimentID: experimentID,
		Skipped:      true,
		SkipReason:   reason,
	}
}
