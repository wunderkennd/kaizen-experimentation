// Package querylog provides logging of executed SQL queries for transparency.
// Every Spark SQL query is logged to PostgreSQL's query_log table.
package querylog

import (
	"context"
	"fmt"
	"sync"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Entry represents a single query log row.
type Entry struct {
	ExperimentID string
	MetricID     string
	SQLText      string
	RowCount     int64
	DurationMs   int64
	JobType      string // "daily_metric", "hourly_guardrail", etc.
	ComputedAt   time.Time
}

// Writer is the interface for logging and retrieving SQL queries.
type Writer interface {
	Log(ctx context.Context, entry Entry) error
	GetLogs(ctx context.Context, experimentID string, metricID string) ([]Entry, error)
}

// PgWriter writes query logs to PostgreSQL.
type PgWriter struct {
	pool *pgxpool.Pool
}

// NewPgWriter creates a PostgreSQL-backed query log writer.
func NewPgWriter(pool *pgxpool.Pool) *PgWriter {
	return &PgWriter{pool: pool}
}

func (w *PgWriter) Log(ctx context.Context, entry Entry) error {
	_, err := w.pool.Exec(ctx,
		`INSERT INTO query_log (experiment_id, metric_id, sql_text, row_count, duration_ms, job_type)
		 VALUES ($1, $2, $3, $4, $5, $6)`,
		entry.ExperimentID, entry.MetricID, entry.SQLText,
		entry.RowCount, entry.DurationMs, entry.JobType,
	)
	if err != nil {
		return fmt.Errorf("querylog: insert: %w", err)
	}
	return nil
}

func (w *PgWriter) GetLogs(ctx context.Context, experimentID string, metricID string) ([]Entry, error) {
	query := `SELECT experiment_id, metric_id, sql_text, row_count, duration_ms, job_type, computed_at
			  FROM query_log WHERE experiment_id = $1`
	args := []any{experimentID}

	if metricID != "" {
		query += ` AND metric_id = $2`
		args = append(args, metricID)
	}
	query += ` ORDER BY computed_at DESC`

	rows, err := w.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, fmt.Errorf("querylog: query: %w", err)
	}
	defer rows.Close()

	var entries []Entry
	for rows.Next() {
		var e Entry
		if err := rows.Scan(&e.ExperimentID, &e.MetricID, &e.SQLText,
			&e.RowCount, &e.DurationMs, &e.JobType, &e.ComputedAt); err != nil {
			return nil, fmt.Errorf("querylog: scan: %w", err)
		}
		entries = append(entries, e)
	}
	return entries, rows.Err()
}

// MemWriter is an in-memory Writer for testing.
type MemWriter struct {
	mu      sync.Mutex
	entries []Entry
}

// NewMemWriter creates an in-memory query log writer.
func NewMemWriter() *MemWriter {
	return &MemWriter{}
}

func (w *MemWriter) Log(_ context.Context, entry Entry) error {
	w.mu.Lock()
	defer w.mu.Unlock()
	entry.ComputedAt = time.Now()
	w.entries = append(w.entries, entry)
	return nil
}

func (w *MemWriter) GetLogs(_ context.Context, experimentID string, metricID string) ([]Entry, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	var result []Entry
	for _, e := range w.entries {
		if e.ExperimentID != experimentID {
			continue
		}
		if metricID != "" && e.MetricID != metricID {
			continue
		}
		result = append(result, e)
	}
	return result, nil
}

// AllEntries returns all logged entries (for test assertions).
func (w *MemWriter) AllEntries() []Entry {
	w.mu.Lock()
	defer w.mu.Unlock()
	out := make([]Entry, len(w.entries))
	copy(out, w.entries)
	return out
}
