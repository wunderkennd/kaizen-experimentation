package status

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PgWriter persists status entries to PostgreSQL via pgxpool, matching the
// driver choice used by services/metrics/internal/querylog/writer.go.
type PgWriter struct {
	pool *pgxpool.Pool
}

// NewPgWriter returns a PostgreSQL-backed status Writer.
func NewPgWriter(pool *pgxpool.Pool) *PgWriter {
	return &PgWriter{pool: pool}
}

// Write upserts a single status entry. Re-runs of the same
// (experiment, metric, date) overwrite the prior outcome so the table always
// reflects the latest scheduling pass.
func (w *PgWriter) Write(ctx context.Context, entry Entry) error {
	_, err := w.pool.Exec(ctx, `
        INSERT INTO metric_computation_status
            (experiment_id, metric_id, computation_date, status, reason, recorded_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        ON CONFLICT (experiment_id, metric_id, computation_date) DO UPDATE
        SET status = EXCLUDED.status,
            reason = EXCLUDED.reason,
            recorded_at = NOW()
    `, entry.ExperimentID, entry.MetricID, entry.ComputationDate, string(entry.Status), entry.Reason)
	if err != nil {
		return fmt.Errorf("status: write %s/%s/%s: %w",
			entry.ExperimentID, entry.MetricID, entry.ComputationDate, err)
	}
	return nil
}
