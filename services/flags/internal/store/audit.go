package store

import (
	"context"
	"encoding/json"
	"time"
)

// AuditEntry records a single mutation to a feature flag.
type AuditEntry struct {
	AuditID       string          `json:"audit_id"`
	FlagID        string          `json:"flag_id"`
	Action        string          `json:"action"`
	ActorEmail    string          `json:"actor_email"`
	PreviousValue json.RawMessage `json:"previous_value,omitempty"`
	NewValue      json.RawMessage `json:"new_value,omitempty"`
	CreatedAt     time.Time       `json:"created_at"`
}

// StaleFlagEntry represents a flag that is a candidate for cleanup.
type StaleFlagEntry struct {
	FlagID            string        `json:"flag_id"`
	Name              string        `json:"name"`
	Description       string        `json:"description"`
	Type              string        `json:"type"`
	Enabled           bool          `json:"enabled"`
	RolloutPercentage float64       `json:"rollout_percentage"`
	UpdatedAt         time.Time     `json:"updated_at"`
	StaleDuration     time.Duration `json:"stale_duration"`
}

// AuditStore defines the persistence interface for flag audit trails.
type AuditStore interface {
	RecordAudit(ctx context.Context, entry *AuditEntry) error
	GetFlagAuditLog(ctx context.Context, flagID string, limit int) ([]*AuditEntry, error)
	GetStaleFlags(ctx context.Context, staleThreshold time.Duration) ([]*StaleFlagEntry, error)
}
