package audit

import (
	"context"
	"encoding/json"
	"time"
)

// Entry records a single mutation in the audit trail.
type Entry struct {
	ExperimentID  string          `json:"experiment_id"`
	Action        string          `json:"action"`
	ActorEmail    string          `json:"actor_email"`
	PreviousState string          `json:"previous_state,omitempty"`
	NewState      string          `json:"new_state,omitempty"`
	Details       json.RawMessage `json:"details,omitempty"`
	CreatedAt     time.Time       `json:"created_at"`
}

// Trail writes audit entries to PostgreSQL.
type Trail struct {
	// TODO: Add pgx pool
}

func NewTrail() *Trail {
	return &Trail{}
}

// Record writes an audit entry. Must succeed — failure here indicates
// a critical system issue and should cause the enclosing transaction to roll back.
func (t *Trail) Record(ctx context.Context, entry Entry) error {
	// TODO: INSERT INTO audit_trail
	return nil
}
