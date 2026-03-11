package store

import (
	"context"
	"encoding/base64"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresStore implements Store using pgx connection pool.
type PostgresStore struct {
	pool *pgxpool.Pool
}

// NewPostgresStore creates a new PostgresStore.
func NewPostgresStore(pool *pgxpool.Pool) *PostgresStore {
	return &PostgresStore{pool: pool}
}

func (s *PostgresStore) CreateFlag(ctx context.Context, f *Flag) (*Flag, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx)

	var targetingRuleID *string
	if f.TargetingRuleID != "" {
		targetingRuleID = &f.TargetingRuleID
	}

	row := tx.QueryRow(ctx,
		`INSERT INTO feature_flags (name, description, type, default_value, enabled, rollout_percentage, targeting_rule_id)
		 VALUES ($1, $2, $3, $4, $5, $6, $7)
		 RETURNING flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at, resolved_at`,
		f.Name, f.Description, f.Type, f.DefaultValue, f.Enabled, f.RolloutPercentage, targetingRuleID,
	)

	created, err := scanFlag(row)
	if err != nil {
		return nil, fmt.Errorf("insert flag: %w", err)
	}

	// Insert variants.
	for i, v := range f.Variants {
		_, err := tx.Exec(ctx,
			`INSERT INTO flag_variants (flag_id, value, traffic_fraction, ordinal)
			 VALUES ($1, $2, $3, $4)`,
			created.FlagID, v.Value, v.TrafficFraction, i,
		)
		if err != nil {
			return nil, fmt.Errorf("insert variant: %w", err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit: %w", err)
	}

	// Re-fetch with variants.
	return s.GetFlag(ctx, created.FlagID)
}

func (s *PostgresStore) GetFlag(ctx context.Context, flagID string) (*Flag, error) {
	row := s.pool.QueryRow(ctx,
		`SELECT flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
		 FROM feature_flags WHERE flag_id = $1`, flagID,
	)

	f, err := scanFlag(row)
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, fmt.Errorf("flag not found: %s", flagID)
		}
		return nil, fmt.Errorf("get flag: %w", err)
	}

	variants, err := s.getVariants(ctx, flagID)
	if err != nil {
		return nil, err
	}
	f.Variants = variants

	return f, nil
}

func (s *PostgresStore) UpdateFlag(ctx context.Context, f *Flag) (*Flag, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx)

	var targetingRuleID *string
	if f.TargetingRuleID != "" {
		targetingRuleID = &f.TargetingRuleID
	}

	tag, err := tx.Exec(ctx,
		`UPDATE feature_flags
		 SET name = $2, description = $3, type = $4, default_value = $5, enabled = $6,
		     rollout_percentage = $7, targeting_rule_id = $8, updated_at = NOW()
		 WHERE flag_id = $1`,
		f.FlagID, f.Name, f.Description, f.Type, f.DefaultValue, f.Enabled, f.RolloutPercentage, targetingRuleID,
	)
	if err != nil {
		return nil, fmt.Errorf("update flag: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return nil, fmt.Errorf("flag not found: %s", f.FlagID)
	}

	// Replace variants: delete old, insert new.
	_, err = tx.Exec(ctx, `DELETE FROM flag_variants WHERE flag_id = $1`, f.FlagID)
	if err != nil {
		return nil, fmt.Errorf("delete variants: %w", err)
	}

	for i, v := range f.Variants {
		_, err := tx.Exec(ctx,
			`INSERT INTO flag_variants (flag_id, value, traffic_fraction, ordinal)
			 VALUES ($1, $2, $3, $4)`,
			f.FlagID, v.Value, v.TrafficFraction, i,
		)
		if err != nil {
			return nil, fmt.Errorf("insert variant: %w", err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit: %w", err)
	}

	return s.GetFlag(ctx, f.FlagID)
}

func (s *PostgresStore) DeleteFlag(ctx context.Context, flagID string) error {
	tag, err := s.pool.Exec(ctx,
		`DELETE FROM feature_flags WHERE flag_id = $1`, flagID,
	)
	if err != nil {
		return fmt.Errorf("delete flag: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("flag not found: %s", flagID)
	}
	return nil
}

func (s *PostgresStore) ListFlags(ctx context.Context, pageSize int, pageToken string) ([]*Flag, string, error) {
	if pageSize <= 0 || pageSize > 100 {
		pageSize = 50
	}

	var cursor string
	if pageToken != "" {
		decoded, err := base64.StdEncoding.DecodeString(pageToken)
		if err != nil {
			return nil, "", fmt.Errorf("invalid page token")
		}
		cursor = string(decoded)
	}

	var rows pgx.Rows
	var err error
	if cursor == "" {
		rows, err = s.pool.Query(ctx,
			`SELECT flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
			 FROM feature_flags ORDER BY flag_id LIMIT $1`, pageSize+1,
		)
	} else {
		rows, err = s.pool.Query(ctx,
			`SELECT flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
			 FROM feature_flags WHERE flag_id > $1 ORDER BY flag_id LIMIT $2`, cursor, pageSize+1,
		)
	}
	if err != nil {
		return nil, "", fmt.Errorf("list flags: %w", err)
	}
	defer rows.Close()

	var flags []*Flag
	for rows.Next() {
		f, err := scanFlagFromRows(rows)
		if err != nil {
			return nil, "", fmt.Errorf("scan flag: %w", err)
		}
		flags = append(flags, f)
	}

	var nextToken string
	if len(flags) > pageSize {
		nextToken = base64.StdEncoding.EncodeToString([]byte(flags[pageSize-1].FlagID))
		flags = flags[:pageSize]
	}

	// Load variants for all flags.
	for _, f := range flags {
		variants, err := s.getVariants(ctx, f.FlagID)
		if err != nil {
			return nil, "", err
		}
		f.Variants = variants
	}

	return flags, nextToken, nil
}

func (s *PostgresStore) GetAllEnabledFlags(ctx context.Context) ([]*Flag, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
		 FROM feature_flags WHERE enabled = TRUE ORDER BY flag_id`,
	)
	if err != nil {
		return nil, fmt.Errorf("get enabled flags: %w", err)
	}
	defer rows.Close()

	var flags []*Flag
	for rows.Next() {
		f, err := scanFlagFromRows(rows)
		if err != nil {
			return nil, fmt.Errorf("scan flag: %w", err)
		}
		flags = append(flags, f)
	}

	for _, f := range flags {
		variants, err := s.getVariants(ctx, f.FlagID)
		if err != nil {
			return nil, err
		}
		f.Variants = variants
	}

	return flags, nil
}

func (s *PostgresStore) getVariants(ctx context.Context, flagID string) ([]FlagVariant, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT variant_id, flag_id, value, traffic_fraction, ordinal
		 FROM flag_variants WHERE flag_id = $1 ORDER BY ordinal`, flagID,
	)
	if err != nil {
		return nil, fmt.Errorf("get variants: %w", err)
	}
	defer rows.Close()

	var variants []FlagVariant
	for rows.Next() {
		var v FlagVariant
		if err := rows.Scan(&v.VariantID, &v.FlagID, &v.Value, &v.TrafficFraction, &v.Ordinal); err != nil {
			return nil, fmt.Errorf("scan variant: %w", err)
		}
		variants = append(variants, v)
	}
	return variants, nil
}

type scannable interface {
	Scan(dest ...any) error
}

func scanFlag(row scannable) (*Flag, error) {
	var f Flag
	var targetingRuleID *string
	var promotedExperimentID *string
	var promotedAt *time.Time
	var resolvedAt *time.Time
	err := row.Scan(
		&f.FlagID, &f.Name, &f.Description, &f.Type, &f.DefaultValue,
		&f.Enabled, &f.RolloutPercentage, &f.Salt, &targetingRuleID,
		&f.CreatedAt, &f.UpdatedAt, &promotedExperimentID, &promotedAt, &resolvedAt,
	)
	if err != nil {
		return nil, err
	}
	if targetingRuleID != nil {
		f.TargetingRuleID = *targetingRuleID
	}
	if promotedExperimentID != nil {
		f.PromotedExperimentID = *promotedExperimentID
	}
	if promotedAt != nil {
		f.PromotedAt = *promotedAt
	}
	if resolvedAt != nil {
		f.ResolvedAt = *resolvedAt
	}
	return &f, nil
}

func scanFlagFromRows(rows pgx.Rows) (*Flag, error) {
	var f Flag
	var targetingRuleID *string
	var promotedExperimentID *string
	var promotedAt *time.Time
	var resolvedAt *time.Time
	err := rows.Scan(
		&f.FlagID, &f.Name, &f.Description, &f.Type, &f.DefaultValue,
		&f.Enabled, &f.RolloutPercentage, &f.Salt, &targetingRuleID,
		&f.CreatedAt, &f.UpdatedAt, &promotedExperimentID, &promotedAt, &resolvedAt,
	)
	if err != nil {
		return nil, err
	}
	if targetingRuleID != nil {
		f.TargetingRuleID = *targetingRuleID
	}
	if promotedExperimentID != nil {
		f.PromotedExperimentID = *promotedExperimentID
	}
	if promotedAt != nil {
		f.PromotedAt = *promotedAt
	}
	if resolvedAt != nil {
		f.ResolvedAt = *resolvedAt
	}
	return &f, nil
}

// --- Flag-experiment linkage ---

func (s *PostgresStore) LinkFlagToExperiment(ctx context.Context, flagID, experimentID string) error {
	tag, err := s.pool.Exec(ctx,
		`UPDATE feature_flags
		 SET promoted_experiment_id = $2, promoted_at = NOW(), updated_at = NOW()
		 WHERE flag_id = $1`,
		flagID, experimentID,
	)
	if err != nil {
		return fmt.Errorf("link flag to experiment: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("flag not found: %s", flagID)
	}
	return nil
}

func (s *PostgresStore) GetFlagByExperiment(ctx context.Context, experimentID string) (*Flag, error) {
	row := s.pool.QueryRow(ctx,
		`SELECT flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
		 FROM feature_flags WHERE promoted_experiment_id = $1`, experimentID,
	)

	f, err := scanFlag(row)
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, fmt.Errorf("no flag found for experiment: %s", experimentID)
		}
		return nil, fmt.Errorf("get flag by experiment: %w", err)
	}

	variants, err := s.getVariants(ctx, f.FlagID)
	if err != nil {
		return nil, err
	}
	f.Variants = variants
	return f, nil
}

// --- Dependency tracking ---

func (s *PostgresStore) GetFlagsByTargetingRule(ctx context.Context, targetingRuleID string) ([]*Flag, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
		 FROM feature_flags WHERE targeting_rule_id = $1 ORDER BY name`, targetingRuleID,
	)
	if err != nil {
		return nil, fmt.Errorf("get flags by targeting rule: %w", err)
	}
	defer rows.Close()

	var flags []*Flag
	for rows.Next() {
		f, err := scanFlagFromRows(rows)
		if err != nil {
			return nil, fmt.Errorf("scan flag: %w", err)
		}
		flags = append(flags, f)
	}

	for _, f := range flags {
		variants, err := s.getVariants(ctx, f.FlagID)
		if err != nil {
			return nil, err
		}
		f.Variants = variants
	}
	return flags, nil
}

func (s *PostgresStore) GetPromotedFlags(ctx context.Context) ([]*Flag, error) {
	rows, err := s.pool.Query(ctx,
		`SELECT flag_id, name, description, type, default_value, enabled, rollout_percentage, salt, targeting_rule_id, created_at, updated_at, promoted_experiment_id, promoted_at, resolved_at
		 FROM feature_flags WHERE promoted_experiment_id IS NOT NULL ORDER BY promoted_at DESC`,
	)
	if err != nil {
		return nil, fmt.Errorf("get promoted flags: %w", err)
	}
	defer rows.Close()

	var flags []*Flag
	for rows.Next() {
		f, err := scanFlagFromRows(rows)
		if err != nil {
			return nil, fmt.Errorf("scan flag: %w", err)
		}
		flags = append(flags, f)
	}

	for _, f := range flags {
		variants, err := s.getVariants(ctx, f.FlagID)
		if err != nil {
			return nil, err
		}
		f.Variants = variants
	}
	return flags, nil
}
