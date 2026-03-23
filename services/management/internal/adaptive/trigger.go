package adaptive

import (
	"context"
	"encoding/json"
	"log/slog"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/org/experimentation-platform/services/management/internal/store"
)

// AdaptiveNConfig is embedded in experiment.type_config["adaptive_n_config"].
//
// JSON key: "adaptive_n_config".
type AdaptiveNConfig struct {
	// Fraction of planned duration at which to fire the interim check.
	// Must be in (0, 1). Typical values: 0.50 (half-way) or 0.67.
	InterimFraction float64 `json:"interim_fraction"`

	// Original planned per-arm sample size.
	NMaxPerArm float64 `json:"n_max_per_arm"`

	// Overall significance level for the experiment (typically == experiments.overall_alpha).
	Alpha float64 `json:"alpha"`

	// Maximum allowed extension factor (n_extended ≤ ExtensionCeiling × NMaxPerArm).
	// Default: 2.0.
	ExtensionCeiling float64 `json:"extension_ceiling,omitempty"`

	// Planned duration in seconds. Used to compute the trigger time as
	// started_at + interim_fraction × planned_duration_seconds.
	PlannedDurationSeconds float64 `json:"planned_duration_seconds"`

	// Internal: set to true once the interim has fired.
	InterimFired bool `json:"interim_fired,omitempty"`
}

// Trigger polls the experiments table on a fixed interval and fires adaptive-N
// interim analyses for RUNNING experiments that have reached their
// interim_fraction × planned_duration wall-clock time.
type Trigger struct {
	pool      *pgxpool.Pool
	store     *store.ExperimentStore
	processor *Processor
	interval  time.Duration
	done      chan struct{}
	cancel    context.CancelFunc
}

// NewTrigger creates a Trigger. `interval` controls how often the scheduler
// polls for experiments that need an interim check (default: 1 minute).
func NewTrigger(pool *pgxpool.Pool, es *store.ExperimentStore, processor *Processor, interval time.Duration) *Trigger {
	if interval <= 0 {
		interval = time.Minute
	}
	return &Trigger{
		pool:      pool,
		store:     es,
		processor: processor,
		interval:  interval,
		done:      make(chan struct{}),
	}
}

// Start begins the polling loop in a background goroutine.
func (t *Trigger) Start(ctx context.Context) {
	ctx, t.cancel = context.WithCancel(ctx)
	go t.loop(ctx)
}

// Stop shuts down the polling loop and waits for it to exit.
func (t *Trigger) Stop() {
	if t.cancel != nil {
		t.cancel()
	}
	<-t.done
}

func (t *Trigger) loop(ctx context.Context) {
	defer close(t.done)
	slog.Info("adaptive-N trigger: started", "interval", t.interval)

	ticker := time.NewTicker(t.interval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			slog.Info("adaptive-N trigger: shutting down")
			return
		case <-ticker.C:
			t.scanAndFire(ctx)
		}
	}
}

// scanAndFire queries for all RUNNING experiments with adaptive_n_config
// set in type_config, then fires any whose interim time has arrived.
func (t *Trigger) scanAndFire(ctx context.Context) {
	// Query experiments that are RUNNING and have adaptive_n_config.
	rows, err := t.pool.Query(ctx, `
		SELECT experiment_id, type_config, started_at, overall_alpha
		FROM experiments
		WHERE state = 'RUNNING'
		  AND type_config ? 'adaptive_n_config'
		ORDER BY started_at ASC
	`)
	if err != nil {
		slog.Error("adaptive-N trigger: query failed", "error", err)
		return
	}
	defer rows.Close()

	type candidateRow struct {
		ExperimentID string
		TypeConfig   json.RawMessage
		StartedAt    *time.Time
		OverallAlpha *float64
	}

	var candidates []candidateRow
	for rows.Next() {
		var r candidateRow
		if err := rows.Scan(&r.ExperimentID, &r.TypeConfig, &r.StartedAt, &r.OverallAlpha); err != nil {
			slog.Error("adaptive-N trigger: scan error", "error", err)
			continue
		}
		candidates = append(candidates, r)
	}
	if err := rows.Err(); err != nil {
		slog.Error("adaptive-N trigger: rows error", "error", err)
		return
	}

	now := time.Now().UTC()

	for _, c := range candidates {
		cfg, err := parseAdaptiveNConfig(c.TypeConfig)
		if err != nil || cfg == nil {
			continue
		}

		// Skip if already fired.
		if cfg.InterimFired {
			continue
		}

		// Compute trigger time: started_at + interim_fraction × planned_duration.
		if c.StartedAt == nil || cfg.PlannedDurationSeconds <= 0 {
			continue
		}
		triggerAt := c.StartedAt.Add(
			time.Duration(cfg.InterimFraction*cfg.PlannedDurationSeconds) * time.Second,
		)

		if now.Before(triggerAt) {
			continue // Not yet.
		}

		// Apply default alpha from experiment if not set.
		alpha := cfg.Alpha
		if alpha <= 0 && c.OverallAlpha != nil {
			alpha = *c.OverallAlpha
		}
		if alpha <= 0 {
			alpha = 0.05
		}

		// Apply default extension ceiling.
		ceiling := cfg.ExtensionCeiling
		if ceiling <= 0 {
			ceiling = 2.0
		}

		nMaxPerArm := cfg.NMaxPerArm

		trigger := InterimTrigger{
			ExperimentID:    c.ExperimentID,
			NInterimPerArm:  estimateCurrentN(c.TypeConfig),
			NMaxPerArm:      nMaxPerArm,
			Alpha:           alpha,
			InterimFraction: cfg.InterimFraction,
		}

		// Fetch observed effect and blinded variance from the last metric result.
		// In Sprint 5.2 M4a delegates the actual stats work; here we fall back to
		// values stored in type_config by M3 metric computation.
		observedEffect, blindedVariance := fetchMetricStats(c.TypeConfig)

		slog.Info("adaptive-N trigger: firing interim",
			"experiment_id", c.ExperimentID,
			"interim_fraction", cfg.InterimFraction,
			"trigger_at", triggerAt.Format(time.RFC3339),
		)

		result, err := t.processor.Process(ctx, trigger, observedEffect, blindedVariance)
		if err != nil {
			slog.Error("adaptive-N trigger: processor error",
				"experiment_id", c.ExperimentID, "error", err)
			continue
		}

		if result.Skipped {
			slog.Info("adaptive-N trigger: skipped",
				"experiment_id", c.ExperimentID, "reason", result.SkipReason)
			continue
		}

		// Mark interim as fired so we don't repeat it.
		if markErr := markInterimFired(ctx, t.pool, c.ExperimentID, c.TypeConfig); markErr != nil {
			slog.Error("adaptive-N trigger: failed to mark interim fired",
				"experiment_id", c.ExperimentID, "error", markErr)
		}
	}
}

// parseAdaptiveNConfig extracts the adaptive_n_config object from type_config.
func parseAdaptiveNConfig(typeConfig json.RawMessage) (*AdaptiveNConfig, error) {
	if len(typeConfig) == 0 {
		return nil, nil
	}
	var tc map[string]json.RawMessage
	if err := json.Unmarshal(typeConfig, &tc); err != nil {
		return nil, err
	}
	raw, ok := tc["adaptive_n_config"]
	if !ok {
		return nil, nil
	}
	var cfg AdaptiveNConfig
	if err := json.Unmarshal(raw, &cfg); err != nil {
		return nil, err
	}
	return &cfg, nil
}

// estimateCurrentN reads n_interim_per_arm from type_config["adaptive_n_current_n"]
// if set; otherwise returns 0. M3/M4a should populate this before the interim fires.
func estimateCurrentN(typeConfig json.RawMessage) float64 {
	var tc map[string]json.RawMessage
	if err := json.Unmarshal(typeConfig, &tc); err != nil {
		return 0
	}
	raw, ok := tc["adaptive_n_current_n"]
	if !ok {
		return 0
	}
	var n float64
	_ = json.Unmarshal(raw, &n)
	return n
}

// fetchMetricStats reads the pre-computed observed_effect and blinded_variance
// from type_config["adaptive_n_metric_stats"]. These are populated by M3/M4a
// during their metric computation run; if absent, returns zeros which will
// cause the processor to report to M4a for a real computation.
func fetchMetricStats(typeConfig json.RawMessage) (observedEffect, blindedVariance float64) {
	var tc map[string]json.RawMessage
	if err := json.Unmarshal(typeConfig, &tc); err != nil {
		return 0, 0
	}
	raw, ok := tc["adaptive_n_metric_stats"]
	if !ok {
		return 0, 0
	}
	var stats struct {
		ObservedEffect  float64 `json:"observed_effect"`
		BlinderVariance float64 `json:"blinded_variance"`
	}
	_ = json.Unmarshal(raw, &stats)
	return stats.ObservedEffect, stats.BlinderVariance
}

// markInterimFired sets adaptive_n_config.interim_fired = true in type_config.
func markInterimFired(ctx context.Context, pool *pgxpool.Pool, experimentID string, typeConfig json.RawMessage) error {
	var tc map[string]json.RawMessage
	if err := json.Unmarshal(typeConfig, &tc); err != nil {
		return err
	}

	rawCfg, ok := tc["adaptive_n_config"]
	if !ok {
		return nil
	}
	var cfg AdaptiveNConfig
	if err := json.Unmarshal(rawCfg, &cfg); err != nil {
		return err
	}
	cfg.InterimFired = true

	newCfgJSON, err := json.Marshal(cfg)
	if err != nil {
		return err
	}
	tc["adaptive_n_config"] = newCfgJSON

	newTypeConfig, err := json.Marshal(tc)
	if err != nil {
		return err
	}

	_, err = pool.Exec(ctx,
		`UPDATE experiments SET type_config = $1, updated_at = NOW() WHERE experiment_id = $2`,
		newTypeConfig, experimentID,
	)
	return err
}
