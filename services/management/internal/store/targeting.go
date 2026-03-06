package store

import (
	"context"
	"encoding/json"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// TargetingRuleRow mirrors the targeting_rules table.
type TargetingRuleRow struct {
	RuleID         string
	Name           string
	RuleDefinition json.RawMessage
	CreatedAt      time.Time
}

// TargetingStore provides database operations for targeting rules.
type TargetingStore struct {
	pool *pgxpool.Pool
}

// NewTargetingStore creates a new TargetingStore.
func NewTargetingStore(pool *pgxpool.Pool) *TargetingStore {
	return &TargetingStore{pool: pool}
}

const targetingCols = `rule_id, name, rule_definition, created_at`

// Insert creates a new targeting rule.
func (s *TargetingStore) Insert(ctx context.Context, row TargetingRuleRow) (TargetingRuleRow, error) {
	var out TargetingRuleRow
	err := s.pool.QueryRow(ctx, `
		INSERT INTO targeting_rules (rule_id, name, rule_definition)
		VALUES ($1, $2, $3)
		RETURNING `+targetingCols,
		row.RuleID, row.Name, row.RuleDefinition,
	).Scan(&out.RuleID, &out.Name, &out.RuleDefinition, &out.CreatedAt)
	return out, err
}

// GetByID retrieves a targeting rule by its ID.
func (s *TargetingStore) GetByID(ctx context.Context, ruleID string) (TargetingRuleRow, error) {
	var out TargetingRuleRow
	err := s.pool.QueryRow(ctx,
		`SELECT `+targetingCols+` FROM targeting_rules WHERE rule_id = $1`, ruleID,
	).Scan(&out.RuleID, &out.Name, &out.RuleDefinition, &out.CreatedAt)
	return out, err
}

// List queries targeting rules ordered by creation time (newest first).
func (s *TargetingStore) List(ctx context.Context, pageSize int32) ([]TargetingRuleRow, error) {
	if pageSize <= 0 || pageSize > 100 {
		pageSize = 20
	}

	rows, err := s.pool.Query(ctx,
		`SELECT `+targetingCols+` FROM targeting_rules ORDER BY created_at DESC LIMIT $1`, pageSize)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var results []TargetingRuleRow
	for rows.Next() {
		var out TargetingRuleRow
		if err := rows.Scan(&out.RuleID, &out.Name, &out.RuleDefinition, &out.CreatedAt); err != nil {
			return nil, err
		}
		results = append(results, out)
	}
	return results, rows.Err()
}
