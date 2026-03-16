// Package querylog provides logging of executed SQL queries for transparency.
// Every Spark SQL query is logged to PostgreSQL's query_log table.
package querylog

import (
	"context"
	"fmt"
	"sort"
	"strconv"
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

// LogFilter specifies filter criteria for retrieving query log entries.
type LogFilter struct {
	ExperimentID string
	MetricID     string
	JobType      string
	After        time.Time
	Before       time.Time
	PageSize     int
	PageToken    string // opaque cursor: stringified index for MemWriter, timestamp for PgWriter
}

// Writer is the interface for logging and retrieving SQL queries.
type Writer interface {
	Log(ctx context.Context, entry Entry) error
	GetLogs(ctx context.Context, experimentID string, metricID string) ([]Entry, error)
	GetLogsFiltered(ctx context.Context, filter LogFilter) ([]Entry, string, error)
	PurgeOldLogs(ctx context.Context, olderThan time.Time) (int64, error)
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

func (w *PgWriter) GetLogsFiltered(ctx context.Context, filter LogFilter) ([]Entry, string, error) {
	query := `SELECT experiment_id, metric_id, sql_text, row_count, duration_ms, job_type, computed_at
			  FROM query_log WHERE experiment_id = $1`
	args := []any{filter.ExperimentID}
	argIdx := 2

	if filter.MetricID != "" {
		query += fmt.Sprintf(` AND metric_id = $%d`, argIdx)
		args = append(args, filter.MetricID)
		argIdx++
	}
	if filter.JobType != "" {
		query += fmt.Sprintf(` AND job_type = $%d`, argIdx)
		args = append(args, filter.JobType)
		argIdx++
	}
	if !filter.After.IsZero() {
		query += fmt.Sprintf(` AND computed_at > $%d`, argIdx)
		args = append(args, filter.After)
		argIdx++
	}
	if !filter.Before.IsZero() {
		query += fmt.Sprintf(` AND computed_at < $%d`, argIdx)
		args = append(args, filter.Before)
		argIdx++
	}
	if filter.PageToken != "" {
		// Page token is an RFC3339Nano timestamp of the last entry.
		ts, err := time.Parse(time.RFC3339Nano, filter.PageToken)
		if err == nil {
			query += fmt.Sprintf(` AND computed_at < $%d`, argIdx)
			args = append(args, ts)
			argIdx++
		}
	}

	query += ` ORDER BY computed_at DESC`

	pageSize := filter.PageSize
	if pageSize <= 0 {
		pageSize = 100
	}
	if pageSize > 1000 {
		pageSize = 1000
	}
	// Fetch one extra to determine if there are more results.
	query += fmt.Sprintf(` LIMIT $%d`, argIdx)
	args = append(args, pageSize+1)

	rows, err := w.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, "", fmt.Errorf("querylog: filtered query: %w", err)
	}
	defer rows.Close()

	var entries []Entry
	for rows.Next() {
		var e Entry
		if err := rows.Scan(&e.ExperimentID, &e.MetricID, &e.SQLText,
			&e.RowCount, &e.DurationMs, &e.JobType, &e.ComputedAt); err != nil {
			return nil, "", fmt.Errorf("querylog: scan: %w", err)
		}
		entries = append(entries, e)
	}
	if err := rows.Err(); err != nil {
		return nil, "", err
	}

	var nextToken string
	if len(entries) > pageSize {
		entries = entries[:pageSize]
		nextToken = entries[pageSize-1].ComputedAt.Format(time.RFC3339Nano)
	}
	return entries, nextToken, nil
}

func (w *PgWriter) PurgeOldLogs(ctx context.Context, olderThan time.Time) (int64, error) {
	tag, err := w.pool.Exec(ctx,
		`DELETE FROM query_log WHERE computed_at < $1`, olderThan)
	if err != nil {
		return 0, fmt.Errorf("querylog: purge: %w", err)
	}
	return tag.RowsAffected(), nil
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

func (w *MemWriter) GetLogsFiltered(_ context.Context, filter LogFilter) ([]Entry, string, error) {
	w.mu.Lock()
	defer w.mu.Unlock()

	var filtered []Entry
	for _, e := range w.entries {
		if e.ExperimentID != filter.ExperimentID {
			continue
		}
		if filter.MetricID != "" && e.MetricID != filter.MetricID {
			continue
		}
		if filter.JobType != "" && e.JobType != filter.JobType {
			continue
		}
		if !filter.After.IsZero() && !e.ComputedAt.After(filter.After) {
			continue
		}
		if !filter.Before.IsZero() && !e.ComputedAt.Before(filter.Before) {
			continue
		}
		filtered = append(filtered, e)
	}

	// Sort by ComputedAt descending.
	sort.Slice(filtered, func(i, j int) bool {
		return filtered[i].ComputedAt.After(filtered[j].ComputedAt)
	})

	// Apply page token (index-based for MemWriter).
	startIdx := 0
	if filter.PageToken != "" {
		if idx, err := strconv.Atoi(filter.PageToken); err == nil && idx > 0 {
			startIdx = idx
		}
	}
	if startIdx >= len(filtered) {
		return nil, "", nil
	}
	filtered = filtered[startIdx:]

	pageSize := filter.PageSize
	if pageSize <= 0 {
		pageSize = 100
	}
	if pageSize > 1000 {
		pageSize = 1000
	}

	var nextToken string
	if len(filtered) > pageSize {
		filtered = filtered[:pageSize]
		nextToken = strconv.Itoa(startIdx + pageSize)
	}
	return filtered, nextToken, nil
}

func (w *MemWriter) PurgeOldLogs(_ context.Context, olderThan time.Time) (int64, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	var kept []Entry
	var purged int64
	for _, e := range w.entries {
		if e.ComputedAt.Before(olderThan) {
			purged++
		} else {
			kept = append(kept, e)
		}
	}
	w.entries = kept
	return purged, nil
}

// Reset clears all logged entries (for benchmarks).
func (w *MemWriter) Reset() {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.entries = w.entries[:0]
}

// AllEntries returns all logged entries (for test assertions).
func (w *MemWriter) AllEntries() []Entry {
	w.mu.Lock()
	defer w.mu.Unlock()
	out := make([]Entry, len(w.entries))
	copy(out, w.entries)
	return out
}
