package store

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// MetricDefinitionRow mirrors the metric_definitions table.
type MetricDefinitionRow struct {
	MetricID               string
	Name                   string
	Description            string
	Type                   string
	SourceEventType        string
	NumeratorEventType     string
	DenominatorEventType   string
	Percentile             *float64
	CustomSQL              string
	LowerIsBetter          bool
	IsQoeMetric            bool
	CupedCovariateMetricID string
	MinimumDetectableEffect *float64
	CreatedAt              time.Time
}

const metricCols = `metric_id, name, description, type, source_event_type,
	numerator_event_type, denominator_event_type, percentile, custom_sql,
	lower_is_better, is_qoe_metric, cuped_covariate_metric_id,
	minimum_detectable_effect, created_at`

// MetricStore provides database operations for metric definitions.
type MetricStore struct {
	pool *pgxpool.Pool
}

// NewMetricStore creates a new MetricStore.
func NewMetricStore(pool *pgxpool.Pool) *MetricStore {
	return &MetricStore{pool: pool}
}

// Insert creates a new metric definition.
func (s *MetricStore) Insert(ctx context.Context, row MetricDefinitionRow) (MetricDefinitionRow, error) {
	var out MetricDefinitionRow
	err := s.pool.QueryRow(ctx, `
		INSERT INTO metric_definitions (
			metric_id, name, description, type, source_event_type,
			numerator_event_type, denominator_event_type, percentile, custom_sql,
			lower_is_better, is_qoe_metric, cuped_covariate_metric_id,
			minimum_detectable_effect
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
		RETURNING `+metricCols,
		row.MetricID, row.Name, row.Description, row.Type, row.SourceEventType,
		row.NumeratorEventType, row.DenominatorEventType, row.Percentile, row.CustomSQL,
		row.LowerIsBetter, row.IsQoeMetric, row.CupedCovariateMetricID,
		row.MinimumDetectableEffect,
	).Scan(
		&out.MetricID, &out.Name, &out.Description, &out.Type, &out.SourceEventType,
		&out.NumeratorEventType, &out.DenominatorEventType, &out.Percentile, &out.CustomSQL,
		&out.LowerIsBetter, &out.IsQoeMetric, &out.CupedCovariateMetricID,
		&out.MinimumDetectableEffect, &out.CreatedAt,
	)
	return out, err
}

// GetByID retrieves a metric definition by its ID.
func (s *MetricStore) GetByID(ctx context.Context, metricID string) (MetricDefinitionRow, error) {
	var out MetricDefinitionRow
	err := s.pool.QueryRow(ctx,
		`SELECT `+metricCols+` FROM metric_definitions WHERE metric_id = $1`, metricID,
	).Scan(
		&out.MetricID, &out.Name, &out.Description, &out.Type, &out.SourceEventType,
		&out.NumeratorEventType, &out.DenominatorEventType, &out.Percentile, &out.CustomSQL,
		&out.LowerIsBetter, &out.IsQoeMetric, &out.CupedCovariateMetricID,
		&out.MinimumDetectableEffect, &out.CreatedAt,
	)
	if err != nil {
		return MetricDefinitionRow{}, err
	}
	return out, nil
}

// ListMetrics queries metric definitions with keyset pagination.
func (s *MetricStore) ListMetrics(ctx context.Context, pageSize int32, pageToken string) ([]MetricDefinitionRow, string, error) {
	if pageSize <= 0 || pageSize > 100 {
		pageSize = 20
	}

	args := []any{}
	where := "WHERE 1=1"
	argN := 0

	nextArg := func() string {
		argN++
		return fmt.Sprintf("$%d", argN)
	}

	if pageToken != "" {
		parts := splitPageToken(pageToken)
		if len(parts) == 2 {
			tokenTime, parseErr := time.Parse(time.RFC3339Nano, parts[0])
			if parseErr == nil {
				tokenID := parts[1]
				p1 := nextArg()
				p2 := nextArg()
				where += fmt.Sprintf(" AND (created_at, metric_id) < (%s, %s)", p1, p2)
				args = append(args, tokenTime, tokenID)
			}
		}
	}

	query := fmt.Sprintf(
		`SELECT %s FROM metric_definitions %s ORDER BY created_at DESC, metric_id DESC LIMIT %s`,
		metricCols, where, nextArg(),
	)
	args = append(args, pageSize+1)

	rows, err := s.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, "", err
	}
	defer rows.Close()

	var results []MetricDefinitionRow
	for rows.Next() {
		var out MetricDefinitionRow
		if err := rows.Scan(
			&out.MetricID, &out.Name, &out.Description, &out.Type, &out.SourceEventType,
			&out.NumeratorEventType, &out.DenominatorEventType, &out.Percentile, &out.CustomSQL,
			&out.LowerIsBetter, &out.IsQoeMetric, &out.CupedCovariateMetricID,
			&out.MinimumDetectableEffect, &out.CreatedAt,
		); err != nil {
			return nil, "", err
		}
		results = append(results, out)
	}
	if err := rows.Err(); err != nil {
		return nil, "", err
	}

	var nextToken string
	if int32(len(results)) > pageSize {
		last := results[pageSize-1]
		nextToken = last.CreatedAt.Format(time.RFC3339Nano) + "|" + last.MetricID
		results = results[:pageSize]
	}

	return results, nextToken, nil
}

// Exists checks whether a metric definition with the given ID exists.
func (s *MetricStore) Exists(ctx context.Context, metricID string) (bool, error) {
	var exists bool
	err := s.pool.QueryRow(ctx,
		`SELECT EXISTS(SELECT 1 FROM metric_definitions WHERE metric_id = $1)`, metricID,
	).Scan(&exists)
	return exists, err
}

// ExistAll checks that all given metric IDs exist. Returns the first missing ID, or "" if all exist.
func (s *MetricStore) ExistAll(ctx context.Context, metricIDs []string) (string, error) {
	for _, id := range metricIDs {
		exists, err := s.Exists(ctx, id)
		if err != nil {
			return id, err
		}
		if !exists {
			return id, nil
		}
	}
	return "", nil
}

// GetByIDTx retrieves a metric definition within a transaction (for future use).
func (s *MetricStore) GetByIDTx(ctx context.Context, tx pgx.Tx, metricID string) (MetricDefinitionRow, error) {
	var out MetricDefinitionRow
	err := tx.QueryRow(ctx,
		`SELECT `+metricCols+` FROM metric_definitions WHERE metric_id = $1`, metricID,
	).Scan(
		&out.MetricID, &out.Name, &out.Description, &out.Type, &out.SourceEventType,
		&out.NumeratorEventType, &out.DenominatorEventType, &out.Percentile, &out.CustomSQL,
		&out.LowerIsBetter, &out.IsQoeMetric, &out.CupedCovariateMetricID,
		&out.MinimumDetectableEffect, &out.CreatedAt,
	)
	if err != nil {
		return MetricDefinitionRow{}, err
	}
	return out, nil
}
