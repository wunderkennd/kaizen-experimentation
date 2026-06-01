// Package shadow provides the shadow-run lifecycle types and storage interface
// for ADR-026 Phase 3 (#437).
//
// A shadow run tracks the parallel execution of an original CUSTOM metric
// alongside a candidate structured/MetricQL definition.  M3's nightly pass
// picks up PENDING runs, executes both sides (B2), records per-tuple diffs
// (B3), and the promotion evaluator (EvaluatePromotion) enforces the
// 7 *consecutive calendar days* within tolerance gate before M5 accepts the
// migration.  Gaps in the calendar window (non-contiguous observation days)
// cause REJECTED because the gate must observe equivalence across a full
// weekly cycle to catch day-of-week / seasonality effects.
package shadow

import (
	"database/sql"
	"encoding/json"
	"time"

	"github.com/google/uuid"
)

// Status is the lifecycle state of a metric_shadow_runs row.
// Values match the CHECK constraint in migration 015 exactly — do not rename
// without updating the DB constraint and the proto comment in metrics_service.proto.
type Status string

const (
	// StatusPending is the initial state after ScheduleShadowComputation.
	// The nightly pass picks up PENDING rows and transitions to RUNNING.
	StatusPending Status = "PENDING"
	// StatusRunning means M3's nightly pass is currently executing this run.
	StatusRunning Status = "RUNNING"
	// StatusApproved means EvaluatePromotion found >= 7 consecutive days
	// within tolerance.  PromoteShadowResult transitions to this state.
	StatusApproved Status = "APPROVED"
	// StatusRejected means at least one day outside tolerance was observed.
	// PromoteShadowResult transitions to this state when EvaluatePromotion
	// returns StatusRejected.
	StatusRejected Status = "REJECTED"
	// StatusFailed means M3's nightly pass encountered an execution error.
	// The nightly scheduler transitions to this state on fatal errors.
	StatusFailed Status = "FAILED"
)

// Run represents one row in metric_shadow_runs.
type Run struct {
	ShadowID         uuid.UUID
	OriginalMetricID string
	// CandidateMetric is the JSON-serialised MetricDefinition proto.
	// B2 unmarshals this to call the MetricQL/structured compute path.
	CandidateMetric json.RawMessage
	ScheduledAt     time.Time
	Status          Status
	// RejectionReason is populated for StatusRejected and StatusFailed.
	// Empty for all other statuses.
	RejectionReason string
}

// ResultRow represents one row in metric_shadow_run_results.
// One row per (shadow_id, experiment_id, variant_id, computation_date) tuple.
// Written by the differ (B3); read by EvaluatePromotion and GetShadowResults.
type ResultRow struct {
	ResultID        uuid.UUID
	ShadowID        uuid.UUID
	ExperimentID    string
	VariantID       string
	ComputationDate string // YYYY-MM-DD; matches Spark SQL template date format.
	// OriginalValue and CandidateValue are NULL when the corresponding
	// computation failed for this tuple.
	OriginalValue  sql.NullFloat64
	CandidateValue sql.NullFloat64
	// DiffAbs and DiffRel are NULL when either OriginalValue or CandidateValue
	// is NULL.  DiffRel is additionally NULL when OriginalValue is zero.
	DiffAbs         sql.NullFloat64
	DiffRel         sql.NullFloat64
	WithinTolerance bool
}
