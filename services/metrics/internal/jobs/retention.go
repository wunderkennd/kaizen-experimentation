package jobs

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strconv"
	"strings"
	"time"

	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// MinRetentionTTLDays is the shortest TTL the retention job accepts. It guards
// against a mistyped override (e.g. "9" for "90") silently deleting recent data.
const MinRetentionTTLDays = 7

// RetentionPolicy is the data TTL for one Delta table (ADR-033 §4).
//
// Delta's logRetentionDuration / deletedFileRetentionDuration only govern the
// transaction log and tombstoned files; this policy is what actually expires
// rows. Rows whose partition is older than TTLDays are deleted, then VACUUM
// removes the unreferenced files once the table's deletedFileRetentionDuration
// has elapsed.
type RetentionPolicy struct {
	Table           string // fully qualified, e.g. "delta.exposures"
	PartitionColumn string
	// PartitionIsString is true for STRING "YYYY-MM-DD" partition columns
	// (date_partition) and false for DATE columns (computation_date).
	PartitionIsString bool
	TTLDays           int
}

// DefaultRetentionPolicies returns the ADR-033 defaults for every Delta table
// in delta/delta_lake_tables.sql that stores unit IDs. Aggregate-only tables
// (daily_treatment_effects, content_consumption) carry no unit IDs and have no TTL.
func DefaultRetentionPolicies() []RetentionPolicy {
	return []RetentionPolicy{
		{Table: "delta.exposures", PartitionColumn: "date_partition", PartitionIsString: true, TTLDays: 90},
		{Table: "delta.metric_events", PartitionColumn: "date_partition", PartitionIsString: true, TTLDays: 90},
		{Table: "delta.qoe_events", PartitionColumn: "date_partition", PartitionIsString: true, TTLDays: 90},
		{Table: "delta.reward_events", PartitionColumn: "date_partition", PartitionIsString: true, TTLDays: 180},
		{Table: "delta.metric_summaries", PartitionColumn: "computation_date", TTLDays: 365},
		{Table: "delta.interleaving_scores", PartitionColumn: "computation_date", TTLDays: 365},
	}
}

// ApplyRetentionOverrides parses "table=days,table=days" (table names with or
// without the "delta." prefix) and returns policies with those TTLs replaced.
// Unknown tables and TTLs below MinRetentionTTLDays are rejected.
func ApplyRetentionOverrides(policies []RetentionPolicy, spec string) ([]RetentionPolicy, error) {
	out := make([]RetentionPolicy, len(policies))
	copy(out, policies)
	spec = strings.TrimSpace(spec)
	if spec == "" {
		return out, nil
	}
	for _, pair := range strings.Split(spec, ",") {
		name, days, ok := strings.Cut(strings.TrimSpace(pair), "=")
		if !ok {
			return nil, fmt.Errorf("retention override %q: want table=days", pair)
		}
		ttl, err := strconv.Atoi(strings.TrimSpace(days))
		if err != nil {
			return nil, fmt.Errorf("retention override %q: %w", pair, err)
		}
		table := strings.TrimSpace(name)
		if !strings.HasPrefix(table, "delta.") {
			table = "delta." + table
		}
		found := false
		for i := range out {
			if out[i].Table == table {
				out[i].TTLDays = ttl
				found = true
			}
		}
		if !found {
			return nil, fmt.Errorf("retention override %q: unknown table %s", pair, table)
		}
	}
	for _, p := range out {
		if err := p.validate(); err != nil {
			return nil, err
		}
	}
	return out, nil
}

func (p RetentionPolicy) validate() error {
	if p.TTLDays < MinRetentionTTLDays {
		return fmt.Errorf("retention policy %s: ttl %d days is below the %d-day minimum", p.Table, p.TTLDays, MinRetentionTTLDays)
	}
	return nil
}

// Cutoff returns the first partition date that is kept: partitions strictly
// older than it are deleted.
func (p RetentionPolicy) Cutoff(now time.Time) string {
	return now.UTC().AddDate(0, 0, -p.TTLDays).Format("2006-01-02")
}

// DeleteSQL renders the partition-pruned DELETE for rows older than the TTL.
// Table and column names come from the fixed policy list, never from input.
func (p RetentionPolicy) DeleteSQL(now time.Time) string {
	literal := fmt.Sprintf("DATE '%s'", p.Cutoff(now))
	if p.PartitionIsString {
		literal = fmt.Sprintf("'%s'", p.Cutoff(now))
	}
	return fmt.Sprintf("DELETE FROM %s WHERE %s < %s", p.Table, p.PartitionColumn, literal)
}

// VacuumSQL renders a VACUUM that honours the table's own
// delta.deletedFileRetentionDuration (no RETAIN override).
func (p RetentionPolicy) VacuumSQL() string {
	return "VACUUM " + p.Table
}

// RetentionTableResult is the outcome for one table.
type RetentionTableResult struct {
	Table       string
	Cutoff      string
	RowsDeleted int64
	Err         error
}

// RetentionResult summarises one retention run.
type RetentionResult struct {
	Tables      []RetentionTableResult
	CompletedAt time.Time
}

// RetentionJob expires unit-level Delta rows past their data TTL (ADR-033 §4).
// It is the backstop that bounds data lifetime independently of erasure.
//
// Statements are logged via slog, not query_log: query_log rows are keyed by
// experiment_id (FK to experiments), and retention is table-wide.
type RetentionJob struct {
	executor spark.SQLExecutor
	policies []RetentionPolicy
}

// NewRetentionJob creates a retention job. Every policy must pass validation.
func NewRetentionJob(executor spark.SQLExecutor, policies []RetentionPolicy) (*RetentionJob, error) {
	for _, p := range policies {
		if err := p.validate(); err != nil {
			return nil, err
		}
	}
	return &RetentionJob{executor: executor, policies: policies}, nil
}

// Run deletes expired partitions and vacuums each table. A failure on one
// table does not stop the others; all failures are returned joined.
func (j *RetentionJob) Run(ctx context.Context, now time.Time) (*RetentionResult, error) {
	start := time.Now()
	result := &RetentionResult{}
	var errs []error
	for _, p := range j.policies {
		tr := j.runTable(ctx, p, now)
		if tr.Err != nil {
			errs = append(errs, tr.Err)
		}
		result.Tables = append(result.Tables, tr)
	}
	result.CompletedAt = time.Now()

	status := "success"
	if len(errs) > 0 {
		status = "error"
	}
	m3metrics.JobDuration.WithLabelValues("retention", "").Observe(time.Since(start).Seconds())
	m3metrics.JobTotal.WithLabelValues("retention", status).Inc()
	return result, errors.Join(errs...)
}

func (j *RetentionJob) runTable(ctx context.Context, p RetentionPolicy, now time.Time) RetentionTableResult {
	tr := RetentionTableResult{Table: p.Table, Cutoff: p.Cutoff(now)}

	deleteSQL := p.DeleteSQL(now)
	res, err := j.executor.ExecuteSQL(ctx, deleteSQL)
	if err != nil {
		tr.Err = fmt.Errorf("retention %s: delete: %w", p.Table, err)
		return tr
	}
	tr.RowsDeleted = res.RowCount
	m3metrics.SparkQueryDuration.WithLabelValues("retention_delete").Observe(res.Duration.Seconds())
	slog.Info("retention sql", "table", p.Table, "sql", deleteSQL, "rows", res.RowCount)

	vacuumSQL := p.VacuumSQL()
	vres, err := j.executor.ExecuteSQL(ctx, vacuumSQL)
	if err != nil {
		tr.Err = fmt.Errorf("retention %s: vacuum: %w", p.Table, err)
		return tr
	}
	m3metrics.SparkQueryDuration.WithLabelValues("retention_vacuum").Observe(vres.Duration.Seconds())
	slog.Info("retention sql", "table", p.Table, "sql", vacuumSQL)

	slog.Info("retention applied", "table", p.Table, "cutoff", tr.Cutoff, "rows_deleted", tr.RowsDeleted)
	return tr
}

// Start runs the job once immediately and then every interval until ctx is
// cancelled. Errors are logged; the next tick retries.
func (j *RetentionJob) Start(ctx context.Context, interval time.Duration) {
	go func() {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()
		for {
			if _, err := j.Run(ctx, time.Now()); err != nil {
				slog.Error("retention run failed", "error", err)
			}
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
			}
		}
	}()
}
