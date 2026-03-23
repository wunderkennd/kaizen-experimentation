// Package fdr implements the e-LOND Online FDR Controller (ADR-018 Phase 2).
//
// # Algorithm
//
// e-LOND (Xu and Ramdas, AISTATS 2024) controls the false discovery rate across
// a stream of experiments under arbitrary dependence. The controller is a
// platform-level singleton: every time an experiment concludes, its primary
// metric's e-value is submitted via [Controller.Test], which returns a
// reject / don't-reject decision while maintaining FDR ≤ alpha.
//
// # Wealth Management
//
// Alpha wealth W_t tracks the remaining budget for future rejections:
//
//	W_0 = alpha  (initial wealth)
//	W_t = W_{t-1} − alpha_t + alpha · 1[reject_t]
//
// At each step t the allocation is:
//
//	gamma_t    = (1 − gamma_decay) · gamma_decay^(t−1)
//	alpha_t    = W_{t-1} · gamma_t
//
// Rejection rule: reject H_t when E_t ≥ 1/alpha_t.
//
// gamma_t is a geometric sequence that sums to 1 over all t. Alpha wealth
// decays toward 0 without rejections and recovers by +alpha per rejection.
// Setting gamma_decay close to 1 (e.g. 0.90) gives a slow decay suitable
// for platforms running many experiments.
//
// # Persistence
//
// State is persisted in the online_fdr_controller_state table (migration 009).
// Each [Controller.Test] call wraps the read-modify-write in a transaction
// with SELECT FOR UPDATE, serializing concurrent conclude operations. Each
// decision is also appended to fdr_decisions for auditability.
package fdr

import (
	"context"
	"fmt"
	"log/slog"
	"math"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Decision is the output of [Controller.Test].
type Decision struct {
	// Rejected is true when the e-value crossed the rejection threshold:
	// E_t >= 1/alpha_allocated.
	Rejected bool

	// AlphaAllocated is the level alpha_t used for this test.
	// Zero if alpha wealth was depleted before this call.
	AlphaAllocated float64

	// WealthBefore is the alpha wealth before this test.
	WealthBefore float64

	// WealthAfter is the alpha wealth after this test (and any replenishment).
	WealthAfter float64

	// NumTested is the total number of hypotheses tested after this call.
	NumTested int64

	// NumRejected is the total number of rejections after this call.
	NumRejected int64
}

// State mirrors the persistent row in online_fdr_controller_state.
type State struct {
	Alpha       float64
	GammaDecay  float64
	NumTested   int64
	NumRejected int64
	AlphaWealth float64
}

// Controller is the platform-level e-LOND Online FDR controller.
// It is backed by the online_fdr_controller_state singleton in PostgreSQL.
//
// Use [NewController] to create a Controller. The migration 009 must have
// been applied before any call to [Controller.Test].
type Controller struct {
	pool *pgxpool.Pool
}

// NewController creates a Controller backed by the given connection pool.
func NewController(pool *pgxpool.Pool) *Controller {
	return &Controller{pool: pool}
}

// Test submits an e-value for a concluded experiment and returns the FDR
// decision. The call is fully transactional:
//  1. SELECT FOR UPDATE on the singleton state row.
//  2. Compute geometric-decay allocation alpha_t.
//  3. Reject when e_value >= 1/alpha_t.
//  4. Checkpoint updated state back to the singleton row.
//  5. Append a row to fdr_decisions.
//  6. Commit.
//
// If alpha_wealth is effectively zero (< 1e-15), the test is recorded with
// Rejected=false and AlphaAllocated=0 to avoid division-by-zero.
func (c *Controller) Test(ctx context.Context, experimentID string, eValue float64) (Decision, error) {
	tx, err := c.pool.BeginTx(ctx, pgx.TxOptions{})
	if err != nil {
		return Decision{}, fmt.Errorf("fdr: begin tx: %w", err)
	}
	defer tx.Rollback(ctx)

	// Load and lock the singleton row.
	var s State
	err = tx.QueryRow(ctx, `
		SELECT alpha, gamma_decay, num_tested, num_rejected, alpha_wealth
		FROM online_fdr_controller_state
		WHERE id = 1
		FOR UPDATE
	`).Scan(&s.Alpha, &s.GammaDecay, &s.NumTested, &s.NumRejected, &s.AlphaWealth)
	if err != nil {
		return Decision{}, fmt.Errorf("fdr: load state: %w", err)
	}

	wealthBefore := s.AlphaWealth

	var alphaAllocated float64
	var rejected bool

	if s.AlphaWealth >= 1e-15 {
		// Geometric-decay allocation:
		//   t          = num_tested + 1  (1-indexed step)
		//   gamma_t    = (1 − gamma_decay) · gamma_decay^(t−1)
		//   alpha_t    = alpha_wealth · gamma_t
		t := s.NumTested + 1
		gammaT := (1.0 - s.GammaDecay) * math.Pow(s.GammaDecay, float64(t-1))
		alphaAllocated = s.AlphaWealth * gammaT

		// Rejection rule: reject when E_t >= 1 / alpha_t.
		rejected = alphaAllocated > 1e-300 && eValue >= 1.0/alphaAllocated
	} else {
		slog.Warn("fdr: alpha wealth depleted, skipping test",
			"experiment_id", experimentID,
			"num_tested", s.NumTested,
		)
	}

	// Update state.
	s.NumTested++
	s.AlphaWealth -= alphaAllocated
	if rejected {
		s.NumRejected++
		s.AlphaWealth += s.Alpha // replenish on rejection
	}
	// Clamp to prevent floating-point drift below zero.
	if s.AlphaWealth < 0 {
		s.AlphaWealth = 0
	}

	// Checkpoint singleton state.
	_, err = tx.Exec(ctx, `
		UPDATE online_fdr_controller_state
		SET num_tested   = $1,
		    num_rejected = $2,
		    alpha_wealth = $3,
		    updated_at   = NOW()
		WHERE id = 1
	`, s.NumTested, s.NumRejected, s.AlphaWealth)
	if err != nil {
		return Decision{}, fmt.Errorf("fdr: checkpoint state: %w", err)
	}

	// Record the per-experiment decision.
	_, err = tx.Exec(ctx, `
		INSERT INTO fdr_decisions
			(experiment_id, e_value, alpha_allocated, rejected,
			 wealth_before, wealth_after, num_tested_at, num_rejected_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
	`, experimentID, eValue, alphaAllocated, rejected,
		wealthBefore, s.AlphaWealth, s.NumTested, s.NumRejected)
	if err != nil {
		return Decision{}, fmt.Errorf("fdr: record decision: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return Decision{}, fmt.Errorf("fdr: commit: %w", err)
	}

	slog.Info("fdr: decision",
		"experiment_id", experimentID,
		"e_value", eValue,
		"alpha_allocated", alphaAllocated,
		"rejected", rejected,
		"wealth_before", wealthBefore,
		"wealth_after", s.AlphaWealth,
		"num_tested", s.NumTested,
		"num_rejected", s.NumRejected,
	)

	return Decision{
		Rejected:       rejected,
		AlphaAllocated: alphaAllocated,
		WealthBefore:   wealthBefore,
		WealthAfter:    s.AlphaWealth,
		NumTested:      s.NumTested,
		NumRejected:    s.NumRejected,
	}, nil
}

// GetState returns a snapshot of the current controller state (no lock).
func (c *Controller) GetState(ctx context.Context) (State, error) {
	var s State
	err := c.pool.QueryRow(ctx, `
		SELECT alpha, gamma_decay, num_tested, num_rejected, alpha_wealth
		FROM online_fdr_controller_state
		WHERE id = 1
	`).Scan(&s.Alpha, &s.GammaDecay, &s.NumTested, &s.NumRejected, &s.AlphaWealth)
	if err != nil {
		return State{}, fmt.Errorf("fdr: get state: %w", err)
	}
	return s, nil
}
