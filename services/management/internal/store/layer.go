package store

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// LayerRow mirrors the layers table.
type LayerRow struct {
	LayerID                   string
	Name                      string
	Description               string
	TotalBuckets              int32
	BucketReuseCooldownSeconds int32
	CreatedAt                 time.Time
}

// AllocationRow mirrors the layer_allocations table.
type AllocationRow struct {
	AllocationID string
	LayerID      string
	ExperimentID string
	StartBucket  int32
	EndBucket    int32
	ActivatedAt  *time.Time
	ReleasedAt   *time.Time
	ReusableAfter *time.Time
}

// LayerStore provides database operations for layers and allocations.
type LayerStore struct {
	pool *pgxpool.Pool
}

// NewLayerStore creates a new LayerStore.
func NewLayerStore(pool *pgxpool.Pool) *LayerStore {
	return &LayerStore{pool: pool}
}

// InsertLayer creates a new layer.
func (s *LayerStore) InsertLayer(ctx context.Context, tx pgx.Tx, row LayerRow) (LayerRow, error) {
	q := db(tx, s.pool)
	var out LayerRow
	err := q.QueryRow(ctx, `
		INSERT INTO layers (name, description, total_buckets, bucket_reuse_cooldown_seconds)
		VALUES ($1, $2, $3, $4)
		RETURNING layer_id, name, description, total_buckets, bucket_reuse_cooldown_seconds, created_at`,
		row.Name, row.Description, row.TotalBuckets, row.BucketReuseCooldownSeconds,
	).Scan(
		&out.LayerID, &out.Name, &out.Description,
		&out.TotalBuckets, &out.BucketReuseCooldownSeconds, &out.CreatedAt,
	)
	return out, err
}

// GetLayerByID retrieves a layer by ID (read-only).
func (s *LayerStore) GetLayerByID(ctx context.Context, id string) (LayerRow, error) {
	var out LayerRow
	err := s.pool.QueryRow(ctx, `
		SELECT layer_id, name, description, total_buckets, bucket_reuse_cooldown_seconds, created_at
		FROM layers WHERE layer_id = $1`, id,
	).Scan(
		&out.LayerID, &out.Name, &out.Description,
		&out.TotalBuckets, &out.BucketReuseCooldownSeconds, &out.CreatedAt,
	)
	return out, err
}

// GetLayerByIDForUpdate retrieves a layer with SELECT FOR UPDATE to serialize
// concurrent allocation attempts within the same layer. Requires a transaction.
func (s *LayerStore) GetLayerByIDForUpdate(ctx context.Context, tx pgx.Tx, id string) (LayerRow, error) {
	var out LayerRow
	err := tx.QueryRow(ctx, `
		SELECT layer_id, name, description, total_buckets, bucket_reuse_cooldown_seconds, created_at
		FROM layers WHERE layer_id = $1 FOR UPDATE`, id,
	).Scan(
		&out.LayerID, &out.Name, &out.Description,
		&out.TotalBuckets, &out.BucketReuseCooldownSeconds, &out.CreatedAt,
	)
	return out, err
}

// GetActiveAllocations returns allocations that are currently occupied: either
// active (released_at IS NULL) or in cooldown (reusable_after > NOW()).
func (s *LayerStore) GetActiveAllocations(ctx context.Context, tx pgx.Tx, layerID string) ([]AllocationRow, error) {
	q := db(tx, s.pool)
	rows, err := q.Query(ctx, `
		SELECT allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
			activated_at, released_at, reusable_after
		FROM layer_allocations
		WHERE layer_id = $1 AND (released_at IS NULL OR reusable_after > NOW())
		ORDER BY start_bucket`, layerID,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	return scanAllocations(rows)
}

// GetAllocationsByLayer returns allocations for a layer. If includeReleased is false,
// only returns active (unreleased) allocations.
func (s *LayerStore) GetAllocationsByLayer(ctx context.Context, layerID string, includeReleased bool) ([]AllocationRow, error) {
	query := `
		SELECT allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
			activated_at, released_at, reusable_after
		FROM layer_allocations
		WHERE layer_id = $1`
	if !includeReleased {
		query += ` AND released_at IS NULL`
	}
	query += ` ORDER BY start_bucket`

	rows, err := s.pool.Query(ctx, query, layerID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	return scanAllocations(rows)
}

// InsertAllocation inserts a new layer allocation with activated_at = NOW().
func (s *LayerStore) InsertAllocation(ctx context.Context, tx pgx.Tx, row AllocationRow) (AllocationRow, error) {
	q := db(tx, s.pool)
	var out AllocationRow
	err := q.QueryRow(ctx, `
		INSERT INTO layer_allocations (layer_id, experiment_id, start_bucket, end_bucket, activated_at)
		VALUES ($1, $2, $3, $4, NOW())
		RETURNING allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
			activated_at, released_at, reusable_after`,
		row.LayerID, row.ExperimentID, row.StartBucket, row.EndBucket,
	).Scan(
		&out.AllocationID, &out.LayerID, &out.ExperimentID,
		&out.StartBucket, &out.EndBucket,
		&out.ActivatedAt, &out.ReleasedAt, &out.ReusableAfter,
	)
	return out, err
}

// ReleaseAllocation marks an experiment's allocation as released with the
// given cooldown. Computes reusable_after = NOW() + cooldown in SQL.
func (s *LayerStore) ReleaseAllocation(ctx context.Context, tx pgx.Tx, experimentID string, cooldownSeconds int32) error {
	q := db(tx, s.pool)
	tag, err := q.Exec(ctx, `
		UPDATE layer_allocations
		SET released_at = NOW(),
			reusable_after = NOW() + ($2 || ' seconds')::interval
		WHERE experiment_id = $1 AND released_at IS NULL`,
		experimentID, fmt.Sprintf("%d", cooldownSeconds),
	)
	if err != nil {
		return err
	}
	if tag.RowsAffected() == 0 {
		// No active allocation found — not an error, experiment may not have had one.
		return nil
	}
	return nil
}

// GetAllocationByExperiment returns the active allocation for an experiment, if any.
func (s *LayerStore) GetAllocationByExperiment(ctx context.Context, tx pgx.Tx, experimentID string) (*AllocationRow, error) {
	q := db(tx, s.pool)
	var out AllocationRow
	err := q.QueryRow(ctx, `
		SELECT allocation_id, layer_id, experiment_id, start_bucket, end_bucket,
			activated_at, released_at, reusable_after
		FROM layer_allocations
		WHERE experiment_id = $1 AND released_at IS NULL`, experimentID,
	).Scan(
		&out.AllocationID, &out.LayerID, &out.ExperimentID,
		&out.StartBucket, &out.EndBucket,
		&out.ActivatedAt, &out.ReleasedAt, &out.ReusableAfter,
	)
	if err != nil {
		if err == pgx.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}
	return &out, nil
}

func scanAllocations(rows pgx.Rows) ([]AllocationRow, error) {
	var result []AllocationRow
	for rows.Next() {
		var a AllocationRow
		if err := rows.Scan(
			&a.AllocationID, &a.LayerID, &a.ExperimentID,
			&a.StartBucket, &a.EndBucket,
			&a.ActivatedAt, &a.ReleasedAt, &a.ReusableAfter,
		); err != nil {
			return nil, err
		}
		result = append(result, a)
	}
	return result, rows.Err()
}
