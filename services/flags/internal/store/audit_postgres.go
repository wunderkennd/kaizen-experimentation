package store

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresAuditStore implements AuditStore using pgx connection pool.
type PostgresAuditStore struct {
	pool *pgxpool.Pool
}

// NewPostgresAuditStore creates a new PostgresAuditStore.
func NewPostgresAuditStore(pool *pgxpool.Pool) *PostgresAuditStore {
	return &PostgresAuditStore{pool: pool}
}

func (s *PostgresAuditStore) RecordAudit(ctx context.Context, entry *AuditEntry) error {
	_, err := s.pool.Exec(ctx,
		`INSERT INTO flag_audit_trail (flag_id, action, actor_email, previous_value, new_value)
		 VALUES ($1, $2, $3, $4, $5)`,
		entry.FlagID, entry.Action, entry.ActorEmail, entry.PreviousValue, entry.NewValue,
	)
	if err != nil {
		return fmt.Errorf("record audit: %w", err)
	}
	return nil
}

func (s *PostgresAuditStore) GetFlagAuditLog(ctx context.Context, flagID string, limit int) ([]*AuditEntry, error) {
	if limit <= 0 || limit > 1000 {
		limit = 100
	}

	rows, err := s.pool.Query(ctx,
		`SELECT audit_id, flag_id, action, actor_email, previous_value, new_value, created_at
		 FROM flag_audit_trail
		 WHERE flag_id = $1
		 ORDER BY created_at DESC
		 LIMIT $2`, flagID, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("get audit log: %w", err)
	}
	defer rows.Close()

	var entries []*AuditEntry
	for rows.Next() {
		var e AuditEntry
		if err := rows.Scan(&e.AuditID, &e.FlagID, &e.Action, &e.ActorEmail, &e.PreviousValue, &e.NewValue, &e.CreatedAt); err != nil {
			return nil, fmt.Errorf("scan audit entry: %w", err)
		}
		entries = append(entries, &e)
	}
	return entries, nil
}

func (s *PostgresAuditStore) GetStaleFlags(ctx context.Context, staleThreshold time.Duration) ([]*StaleFlagEntry, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT flag_id, name, description, type, enabled, rollout_percentage, updated_at,
		        NOW() - updated_at AS stale_duration
		 FROM feature_flags
		 WHERE enabled = TRUE
		   AND rollout_percentage >= 1.0
		   AND updated_at < NOW() - $1::interval
		 ORDER BY updated_at ASC`, staleThreshold.String(),
	)
	if err != nil {
		return nil, fmt.Errorf("get stale flags: %w", err)
	}
	defer rows.Close()

	var entries []*StaleFlagEntry
	for rows.Next() {
		var e StaleFlagEntry
		if err := rows.Scan(&e.FlagID, &e.Name, &e.Description, &e.Type, &e.Enabled, &e.RolloutPercentage, &e.UpdatedAt, &e.StaleDuration); err != nil {
			return nil, fmt.Errorf("scan stale flag: %w", err)
		}
		entries = append(entries, &e)
	}
	return entries, nil
}
