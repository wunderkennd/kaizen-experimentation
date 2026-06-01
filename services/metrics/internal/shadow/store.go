package shadow

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ErrCASFailure is returned by Transition when zero rows are affected —
// meaning the row is either missing or not in the expected `from` state.
// Callers can use errors.Is(err, shadow.ErrCASFailure) to distinguish a
// logical precondition failure from a transient store error.
var ErrCASFailure = errors.New("shadow: status CAS failure (row missing or not in expected state)")

// IsCASFailure reports whether err wraps ErrCASFailure.
// Convenience helper so callers don't need to import "errors" just for Is().
func IsCASFailure(err error) bool {
	return errors.Is(err, ErrCASFailure)
}

// Store is the persistence interface for shadow runs.
// Implementations: PgStore (production), MockStore (tests).
type Store interface {
	// Schedule inserts a new shadow run in PENDING status and returns its UUID.
	Schedule(ctx context.Context, originalMetricID string, candidate json.RawMessage) (uuid.UUID, error)
	// Get returns the Run for the given shadowID.  Returns nil, nil when the
	// row does not exist (the caller should map this to CodeNotFound).
	Get(ctx context.Context, shadowID uuid.UUID) (*Run, error)
	// ListPending returns all runs in PENDING status.  Used by B2 to pick up
	// work in the nightly pass.
	ListPending(ctx context.Context) ([]Run, error)
	// ListNeedingComputation returns PENDING runs for which no result row exists
	// for (experimentID, computationDate) yet.  This is the dedup gate: once
	// computeOneShadow writes a stub ResultRow for (shadow_id, experimentID,
	// computationDate), the shadow is excluded from subsequent calls within the
	// same nightly pass for that experiment.  computationDate must be "YYYY-MM-DD".
	// B3 will UPDATE the stub row with real diff values later.
	ListNeedingComputation(ctx context.Context, experimentID, computationDate string) ([]Run, error)
	// Transition atomically updates the status of a shadow run using a
	// compare-and-swap: the row is updated only when its current status equals
	// `from`.  Returns an error when zero rows are affected (CAS failure).
	// `reason` is persisted only for transitions to StatusRejected or
	// StatusFailed; pass "" for all other transitions.
	Transition(ctx context.Context, shadowID uuid.UUID, from, to Status, reason string) error
	// Results returns all ResultRows for the given shadowID ordered by
	// (computation_date, experiment_id, variant_id).
	Results(ctx context.Context, shadowID uuid.UUID) ([]ResultRow, error)
	// InsertResult writes a single per-tuple result row.  Used by B3 after
	// each nightly differ pass.
	InsertResult(ctx context.Context, row ResultRow) error
}

// PgStore is the PostgreSQL-backed implementation of Store.
// Modelled on services/metrics/internal/status/pg_writer.go.
type PgStore struct {
	pool *pgxpool.Pool
}

// NewPgStore returns a PgStore backed by the given connection pool.
func NewPgStore(pool *pgxpool.Pool) *PgStore {
	return &PgStore{pool: pool}
}

// Schedule inserts a new metric_shadow_runs row in PENDING status.
func (s *PgStore) Schedule(ctx context.Context, originalMetricID string, candidate json.RawMessage) (uuid.UUID, error) {
	var id uuid.UUID
	err := s.pool.QueryRow(ctx, `
		INSERT INTO metric_shadow_runs
			(original_metric_id, candidate_metric, scheduled_at, status)
		VALUES ($1, $2, NOW(), 'PENDING')
		RETURNING shadow_id
	`, originalMetricID, []byte(candidate)).Scan(&id)
	if err != nil {
		return uuid.Nil, fmt.Errorf("shadow: schedule %s: %w", originalMetricID, err)
	}
	return id, nil
}

// Get returns the Run for shadowID, or nil if no such row exists.
func (s *PgStore) Get(ctx context.Context, shadowID uuid.UUID) (*Run, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT shadow_id, original_metric_id, candidate_metric,
		       scheduled_at, status, COALESCE(rejection_reason, '')
		FROM metric_shadow_runs
		WHERE shadow_id = $1
	`, shadowID)
	var r Run
	var rawCandidate []byte
	var statusStr string
	err := row.Scan(&r.ShadowID, &r.OriginalMetricID, &rawCandidate,
		&r.ScheduledAt, &statusStr, &r.RejectionReason)
	if err != nil {
		// pgx.ErrNoRows means the shadow run does not exist; callers map this to
		// CodeNotFound.  Matches the convention in
		// services/management/internal/fdr/controller.go:129,229 and
		// services/management/internal/handlers/errors.go:31.
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, nil
		}
		return nil, fmt.Errorf("shadow: get %s: %w", shadowID, err)
	}
	r.CandidateMetric = json.RawMessage(rawCandidate)
	r.Status = Status(statusStr)
	return &r, nil
}

// ListPending returns all runs currently in PENDING status.
func (s *PgStore) ListPending(ctx context.Context) ([]Run, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT shadow_id, original_metric_id, candidate_metric,
		       scheduled_at, status, COALESCE(rejection_reason, '')
		FROM metric_shadow_runs
		WHERE status = 'PENDING'
		ORDER BY scheduled_at
	`)
	if err != nil {
		return nil, fmt.Errorf("shadow: list pending: %w", err)
	}
	defer rows.Close()

	var runs []Run
	for rows.Next() {
		var r Run
		var rawCandidate []byte
		var statusStr string
		if err := rows.Scan(&r.ShadowID, &r.OriginalMetricID, &rawCandidate,
			&r.ScheduledAt, &statusStr, &r.RejectionReason); err != nil {
			return nil, fmt.Errorf("shadow: list pending scan: %w", err)
		}
		r.CandidateMetric = json.RawMessage(rawCandidate)
		r.Status = Status(statusStr)
		runs = append(runs, r)
	}
	return runs, rows.Err()
}

// ListNeedingComputation returns PENDING runs for which no result row yet exists
// for (experimentID, computationDate), making it safe to call multiple times
// within a nightly pass (once per experiment) without re-computing a shadow
// for an experiment that already received a stub result row in this pass.
// After a successful compute, computeOneShadow writes a stub ResultRow; this
// NOT EXISTS gate then skips the shadow on subsequent calls for the same
// (experimentID, computationDate) pair — which is the correct B2→B3 contract:
// B3 will UPDATE the stub to add real diff values.
func (s *PgStore) ListNeedingComputation(ctx context.Context, experimentID, computationDate string) ([]Run, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT shadow_id, original_metric_id, candidate_metric,
		       scheduled_at, status, COALESCE(rejection_reason, '')
		FROM metric_shadow_runs
		WHERE status = 'PENDING'
		  AND NOT EXISTS (
		        SELECT 1
		        FROM metric_shadow_run_results
		        WHERE shadow_id   = metric_shadow_runs.shadow_id
		          AND experiment_id    = $1
		          AND computation_date = $2::DATE
		      )
		ORDER BY scheduled_at
	`, experimentID, computationDate)
	if err != nil {
		return nil, fmt.Errorf("shadow: list needing computation: %w", err)
	}
	defer rows.Close()

	var runs []Run
	for rows.Next() {
		var r Run
		var rawCandidate []byte
		var statusStr string
		if err := rows.Scan(&r.ShadowID, &r.OriginalMetricID, &rawCandidate,
			&r.ScheduledAt, &statusStr, &r.RejectionReason); err != nil {
			return nil, fmt.Errorf("shadow: list needing computation scan: %w", err)
		}
		r.CandidateMetric = json.RawMessage(rawCandidate)
		r.Status = Status(statusStr)
		runs = append(runs, r)
	}
	return runs, rows.Err()
}

// Transition atomically updates the status of a run from `from` to `to`.
// Returns an error if zero rows were affected (CAS failure: the row is either
// missing or not in the expected `from` state).
func (s *PgStore) Transition(ctx context.Context, shadowID uuid.UUID, from, to Status, reason string) error {
	// Only persist the reason for terminal failure states; clear it on success.
	var reasonArg *string
	if to == StatusRejected || to == StatusFailed {
		r := reason
		reasonArg = &r
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE metric_shadow_runs
		SET    status           = $1,
		       rejection_reason = $2
		WHERE  shadow_id = $3
		  AND  status    = $4
	`, string(to), reasonArg, shadowID, string(from))
	if err != nil {
		return fmt.Errorf("shadow: transition %s %s→%s: %w", shadowID, from, to, err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("transition shadow %s %s->%s: %w", shadowID, from, to, ErrCASFailure)
	}
	return nil
}

// Results returns all result rows for the given shadow run.
func (s *PgStore) Results(ctx context.Context, shadowID uuid.UUID) ([]ResultRow, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT result_id, shadow_id, experiment_id, variant_id,
		       computation_date::TEXT,
		       original_value, candidate_value, diff_abs, diff_rel,
		       within_tolerance
		FROM metric_shadow_run_results
		WHERE shadow_id = $1
		ORDER BY computation_date, experiment_id, variant_id
	`, shadowID)
	if err != nil {
		return nil, fmt.Errorf("shadow: results %s: %w", shadowID, err)
	}
	defer rows.Close()

	var results []ResultRow
	for rows.Next() {
		var r ResultRow
		if err := rows.Scan(
			&r.ResultID, &r.ShadowID, &r.ExperimentID, &r.VariantID,
			&r.ComputationDate,
			&r.OriginalValue, &r.CandidateValue, &r.DiffAbs, &r.DiffRel,
			&r.WithinTolerance,
		); err != nil {
			return nil, fmt.Errorf("shadow: results %s scan: %w", shadowID, err)
		}
		results = append(results, r)
	}
	return results, rows.Err()
}

// InsertResult writes one result row.
func (s *PgStore) InsertResult(ctx context.Context, row ResultRow) error {
	_, err := s.pool.Exec(ctx, `
		INSERT INTO metric_shadow_run_results
			(shadow_id, experiment_id, variant_id, computation_date,
			 original_value, candidate_value, diff_abs, diff_rel, within_tolerance)
		VALUES ($1, $2, $3, $4::DATE, $5, $6, $7, $8, $9)
	`,
		row.ShadowID, row.ExperimentID, row.VariantID, row.ComputationDate,
		row.OriginalValue, row.CandidateValue, row.DiffAbs, row.DiffRel,
		row.WithinTolerance,
	)
	if err != nil {
		return fmt.Errorf("shadow: insert result %s/%s/%s/%s: %w",
			row.ShadowID, row.ExperimentID, row.VariantID, row.ComputationDate, err)
	}
	return nil
}


