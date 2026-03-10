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

// ReadForExperiment returns all projection records for the given experiment.
func (w *MemProjectionWriter) ReadForExperiment(_ context.Context, experimentID string) ([]ProjectionRecord, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	var out []ProjectionRecord
	for _, r := range w.Records {
		if r.ExperimentID == experimentID {
			out = append(out, r)
		}
	}
	return out, nil
}

// ProjectionReader reads past surrogate projections for recalibration.
type ProjectionReader interface {
	ReadForExperiment(ctx context.Context, experimentID string) ([]ProjectionRecord, error)
}

// PgProjectionReader reads projections from PostgreSQL.
type PgProjectionReader struct {
	pool *pgxpool.Pool
}

func NewPgProjectionReader(pool *pgxpool.Pool) *PgProjectionReader {
	return &PgProjectionReader{pool: pool}
}

func (r *PgProjectionReader) ReadForExperiment(ctx context.Context, experimentID string) ([]ProjectionRecord, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT experiment_id, variant_id, model_id, projected_effect,
		       projection_ci_lower, projection_ci_upper, calibration_r_squared, computed_at
		FROM surrogate_projections
		WHERE experiment_id = $1
		ORDER BY computed_at DESC`, experimentID)
	if err != nil {
		return nil, fmt.Errorf("surrogate: read projections: %w", err)
	}
	defer rows.Close()

	var records []ProjectionRecord
	for rows.Next() {
		var rec ProjectionRecord
		if err := rows.Scan(&rec.ExperimentID, &rec.VariantID, &rec.ModelID,
			&rec.ProjectedEffect, &rec.ProjectionCILower, &rec.ProjectionCIUpper,
			&rec.CalibrationRSquared, &rec.ComputedAt); err != nil {
			return nil, fmt.Errorf("surrogate: scan projection: %w", err)
		}
		records = append(records, rec)
	}
	return records, rows.Err()
}

// CalibrationUpdater updates a surrogate model's calibration R².
type CalibrationUpdater interface {
	UpdateCalibration(ctx context.Context, modelID string, rSquared float64) error
}

// PgCalibrationUpdater updates calibration in PostgreSQL.
type PgCalibrationUpdater struct {
	pool *pgxpool.Pool
}

func NewPgCalibrationUpdater(pool *pgxpool.Pool) *PgCalibrationUpdater {
	return &PgCalibrationUpdater{pool: pool}
}

func (u *PgCalibrationUpdater) UpdateCalibration(ctx context.Context, modelID string, rSquared float64) error {
	_, err := u.pool.Exec(ctx, `
		UPDATE surrogate_models
		SET calibration_r_squared = $2, last_calibrated_at = NOW()
		WHERE model_id = $1`, modelID, rSquared)
	if err != nil {
		return fmt.Errorf("surrogate: update calibration: %w", err)
	}
	return nil
}

// MemCalibrationUpdater is an in-memory CalibrationUpdater for tests.
type MemCalibrationUpdater struct {
	mu      sync.Mutex
	Updates map[string]float64
}

func NewMemCalibrationUpdater() *MemCalibrationUpdater {
	return &MemCalibrationUpdater{Updates: make(map[string]float64)}
}

func (u *MemCalibrationUpdater) UpdateCalibration(_ context.Context, modelID string, rSquared float64) error {
	u.mu.Lock()
	defer u.mu.Unlock()
	u.Updates[modelID] = rSquared
	return nil
}

// Reset clears all stored updates (for benchmarks).
func (u *MemCalibrationUpdater) Reset() {
	u.mu.Lock()
	defer u.mu.Unlock()
	for k := range u.Updates {
		delete(u.Updates, k)
	}
}
