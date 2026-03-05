package store

import (
	"context"
	"encoding/json"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// AuditEntry represents a row in the audit_trail table.
type AuditEntry struct {
	ExperimentID  string
	Action        string
	ActorEmail    string
	PreviousState string
	NewState      string
	DetailsJSON   json.RawMessage
}

// AuditStore provides database operations for the audit trail.
type AuditStore struct {
	pool *pgxpool.Pool
}

// NewAuditStore creates a new AuditStore.
func NewAuditStore(pool *pgxpool.Pool) *AuditStore {
	return &AuditStore{pool: pool}
}

// Insert writes an audit trail entry, optionally within a transaction.
func (s *AuditStore) Insert(ctx context.Context, tx pgx.Tx, entry AuditEntry) error {
	q := db(tx, s.pool)

	details := entry.DetailsJSON
	if details == nil {
		details = json.RawMessage(`{}`)
	}

	_, err := q.Exec(ctx, `
		INSERT INTO audit_trail (experiment_id, action, actor_email, previous_state, new_state, details_json)
		VALUES ($1, $2, $3, $4, $5, $6)`,
		entry.ExperimentID, entry.Action, entry.ActorEmail,
		nilIfEmpty(entry.PreviousState), nilIfEmpty(entry.NewState), details,
	)
	return err
}

func nilIfEmpty(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}
