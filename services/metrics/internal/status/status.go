// Package status records the per-metric outcome of a single M3 scheduling pass
// to PostgreSQL so M4a can distinguish "missing because not scheduled" from
// "skipped because upstream failed" or "failed".
//
// See ADR-026 Phase 1 follow-up (#475) and migration 012.
package status

import (
	"context"
	"time"
)

// Status enumerates the four terminal outcomes a metric can land in during a
// scheduling pass. The string values match the CHECK constraint in
// `sql/migrations/012_metric_computation_status.sql` exactly and are part of
// the M4a contract — do not rename without coordinating with Agent-4.
type Status string

const (
	Completed              Status = "completed"
	Failed                 Status = "failed"
	SkippedUpstreamFailure Status = "skipped_upstream_failure"
	SkippedCycle           Status = "skipped_cycle"
)

// Entry is one row of metric_computation_status.
type Entry struct {
	ExperimentID    string
	MetricID        string
	ComputationDate string // YYYY-MM-DD; matches Spark SQL templates' date format.
	Status          Status
	Reason          string // Free-form explanation (e.g., "operand watch_time failed: <err>").
	RecordedAt      time.Time
}

// Writer is the interface M3's scheduler uses to flush per-metric status
// after each pass. Implementations: PgWriter (production), MockWriter (tests).
type Writer interface {
	Write(ctx context.Context, entry Entry) error
}
