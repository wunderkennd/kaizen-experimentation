package surrogate

import (
	"context"
	"fmt"
	"sync"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// ProjectionRecord is a row in the surrogate_projections table.
type ProjectionRecord struct {
	ExperimentID        string
	VariantID           string
	ModelID             string
	ProjectedEffect     float64
	ProjectionCILower   float64
	ProjectionCIUpper   float64
	CalibrationRSquared float64
	ComputedAt          time.Time
}

// ProjectionWriter persists surrogate projections.
type ProjectionWriter interface {
	Write(ctx context.Context, record ProjectionRecord) error
}

// PgProjectionWriter writes to PostgreSQL surrogate_projections table.
type PgProjectionWriter struct {
	pool *pgxpool.Pool
}

func NewPgProjectionWriter(pool *pgxpool.Pool) *PgProjectionWriter {
	return &PgProjectionWriter{pool: pool}
}

func (w *PgProjectionWriter) Write(ctx context.Context, record ProjectionRecord) error {
	_, err := w.pool.Exec(ctx, `
		INSERT INTO surrogate_projections
			(experiment_id, variant_id, model_id, projected_effect,
			 projection_ci_lower, projection_ci_upper, calibration_r_squared, computed_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
		record.ExperimentID, record.VariantID, record.ModelID,
		record.ProjectedEffect, record.ProjectionCILower, record.ProjectionCIUpper,
		record.CalibrationRSquared, record.ComputedAt,
	)
	if err != nil {
		return fmt.Errorf("surrogate: write projection: %w", err)
	}
	return nil
}

// MemProjectionWriter is an in-memory writer for tests.
type MemProjectionWriter struct {
	mu      sync.Mutex
	Records []ProjectionRecord
}

func NewMemProjectionWriter() *MemProjectionWriter {
	return &MemProjectionWriter{}
}

func (w *MemProjectionWriter) Write(_ context.Context, record ProjectionRecord) error {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.Records = append(w.Records, record)
	return nil
}

// Reset clears all stored records (for benchmarks).
func (w *MemProjectionWriter) Reset() {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.Records = w.Records[:0]
}

func (w *MemProjectionWriter) AllRecords() []ProjectionRecord {
	w.mu.Lock()
	defer w.mu.Unlock()
	out := make([]ProjectionRecord, len(w.Records))
	copy(out, w.Records)
	return out
}
