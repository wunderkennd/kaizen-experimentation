package store

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// SurrogateModelRow mirrors the surrogate_models table.
type SurrogateModelRow struct {
	ModelID               string
	TargetMetricID        string
	InputMetricIDs        []string
	ObservationWindowDays int32
	PredictionHorizonDays int32
	ModelType             string
	CalibrationRSquared   *float64
	MlflowModelURI       string
	LastCalibratedAt      *time.Time
	CreatedAt             time.Time
}

const surrogateCols = `model_id, target_metric_id, input_metric_ids,
	observation_window_days, prediction_horizon_days, model_type,
	calibration_r_squared, mlflow_model_uri, last_calibrated_at, created_at`

// SurrogateStore provides database operations for surrogate models.
type SurrogateStore struct {
	pool *pgxpool.Pool
}

// NewSurrogateStore creates a new SurrogateStore.
func NewSurrogateStore(pool *pgxpool.Pool) *SurrogateStore {
	return &SurrogateStore{pool: pool}
}

// Insert creates a new surrogate model.
func (s *SurrogateStore) Insert(ctx context.Context, row SurrogateModelRow) (SurrogateModelRow, error) {
	var out SurrogateModelRow
	err := s.pool.QueryRow(ctx, `
		INSERT INTO surrogate_models (
			model_id, target_metric_id, input_metric_ids,
			observation_window_days, prediction_horizon_days, model_type,
			calibration_r_squared, mlflow_model_uri
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
		RETURNING `+surrogateCols,
		row.ModelID, row.TargetMetricID, row.InputMetricIDs,
		row.ObservationWindowDays, row.PredictionHorizonDays, row.ModelType,
		row.CalibrationRSquared, row.MlflowModelURI,
	).Scan(
		&out.ModelID, &out.TargetMetricID, &out.InputMetricIDs,
		&out.ObservationWindowDays, &out.PredictionHorizonDays, &out.ModelType,
		&out.CalibrationRSquared, &out.MlflowModelURI,
		&out.LastCalibratedAt, &out.CreatedAt,
	)
	return out, err
}

// GetByID retrieves a surrogate model by its ID.
func (s *SurrogateStore) GetByID(ctx context.Context, modelID string) (SurrogateModelRow, error) {
	var out SurrogateModelRow
	err := s.pool.QueryRow(ctx,
		`SELECT `+surrogateCols+` FROM surrogate_models WHERE model_id = $1`, modelID,
	).Scan(
		&out.ModelID, &out.TargetMetricID, &out.InputMetricIDs,
		&out.ObservationWindowDays, &out.PredictionHorizonDays, &out.ModelType,
		&out.CalibrationRSquared, &out.MlflowModelURI,
		&out.LastCalibratedAt, &out.CreatedAt,
	)
	if err != nil {
		return SurrogateModelRow{}, err
	}
	return out, nil
}

// List queries surrogate models with keyset pagination.
func (s *SurrogateStore) List(ctx context.Context, pageSize int32, pageToken string) ([]SurrogateModelRow, string, error) {
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
				where += fmt.Sprintf(" AND (created_at, model_id) < (%s, %s)", p1, p2)
				args = append(args, tokenTime, tokenID)
			}
		}
	}

	query := fmt.Sprintf(
		`SELECT %s FROM surrogate_models %s ORDER BY created_at DESC, model_id DESC LIMIT %s`,
		surrogateCols, where, nextArg(),
	)
	args = append(args, pageSize+1)

	rows, err := s.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, "", err
	}
	defer rows.Close()

	var results []SurrogateModelRow
	for rows.Next() {
		var out SurrogateModelRow
		if err := rows.Scan(
			&out.ModelID, &out.TargetMetricID, &out.InputMetricIDs,
			&out.ObservationWindowDays, &out.PredictionHorizonDays, &out.ModelType,
			&out.CalibrationRSquared, &out.MlflowModelURI,
			&out.LastCalibratedAt, &out.CreatedAt,
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
		nextToken = last.CreatedAt.Format(time.RFC3339Nano) + "|" + last.ModelID
		results = results[:pageSize]
	}

	return results, nextToken, nil
}

// UpdateCalibration updates the calibration fields for a surrogate model.
func (s *SurrogateStore) UpdateCalibration(ctx context.Context, modelID string, rSquared float64, mlflowURI string) error {
	tag, err := s.pool.Exec(ctx, `
		UPDATE surrogate_models
		SET calibration_r_squared = $2, mlflow_model_uri = $3, last_calibrated_at = NOW()
		WHERE model_id = $1`,
		modelID, rSquared, mlflowURI,
	)
	if err != nil {
		return err
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("surrogate model %s not found", modelID)
	}
	return nil
}
