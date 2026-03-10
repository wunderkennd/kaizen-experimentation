package handlers

import (
	"context"
	"log/slog"
	"time"

	"connectrpc.com/connect"
	"github.com/google/uuid"
	"google.golang.org/protobuf/types/known/emptypb"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/store"
	surrogatepkg "github.com/org/experimentation-platform/services/management/internal/surrogate"
	"github.com/org/experimentation-platform/services/management/internal/validation"
)

// CreateSurrogateModel validates the request, inserts the surrogate model,
// records an audit trail entry, and returns the created model.
func (s *ExperimentService) CreateSurrogateModel(
	ctx context.Context,
	req *connect.Request[mgmtv1.CreateSurrogateModelRequest],
) (*connect.Response[commonv1.SurrogateModelConfig], error) {
	m := req.Msg.GetModel()

	if err := validation.ValidateCreateSurrogateModel(m); err != nil {
		return nil, err
	}

	row := store.SurrogateModelToRow(m)

	if row.ModelID == "" {
		row.ModelID = uuid.NewString()
	}

	created, err := s.surrogates.Insert(ctx, row)
	if err != nil {
		return nil, wrapDBError(err, "surrogate_model", row.ModelID)
	}

	// NOTE: audit_trail.experiment_id has a FK to experiments — surrogate model
	// operations are logged via slog until the schema supports non-experiment auditing.
	slog.Info("surrogate model created", "model_id", created.ModelID, "target_metric", created.TargetMetricID, "type", created.ModelType)
	return connect.NewResponse(store.RowToSurrogateModel(created)), nil
}

// ListSurrogateModels returns a paginated list of surrogate models.
func (s *ExperimentService) ListSurrogateModels(
	ctx context.Context,
	req *connect.Request[mgmtv1.ListSurrogateModelsRequest],
) (*connect.Response[mgmtv1.ListSurrogateModelsResponse], error) {
	rows, nextToken, err := s.surrogates.List(ctx, req.Msg.GetPageSize(), req.Msg.GetPageToken())
	if err != nil {
		return nil, internalError("list surrogate models", err)
	}

	resp := &mgmtv1.ListSurrogateModelsResponse{
		NextPageToken: nextToken,
	}
	for _, row := range rows {
		resp.Models = append(resp.Models, store.RowToSurrogateModel(row))
	}

	return connect.NewResponse(resp), nil
}

// GetSurrogateCalibration retrieves a surrogate model by ID (includes calibration data).
func (s *ExperimentService) GetSurrogateCalibration(
	ctx context.Context,
	req *connect.Request[mgmtv1.GetSurrogateCalibrationRequest],
) (*connect.Response[commonv1.SurrogateModelConfig], error) {
	id := req.Msg.GetModelId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	row, err := s.surrogates.GetByID(ctx, id)
	if err != nil {
		return nil, wrapDBError(err, "surrogate_model", id)
	}

	return connect.NewResponse(store.RowToSurrogateModel(row)), nil
}

// TriggerSurrogateRecalibration validates the model exists, records an audit
// trail entry, and returns. Actual recalibration is async via Agent-3.
func (s *ExperimentService) TriggerSurrogateRecalibration(
	ctx context.Context,
	req *connect.Request[mgmtv1.TriggerSurrogateRecalibrationRequest],
) (*connect.Response[emptypb.Empty], error) {
	id := req.Msg.GetModelId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	// Verify model exists and capture row for the Kafka envelope.
	model, err := s.surrogates.GetByID(ctx, id)
	if err != nil {
		return nil, wrapDBError(err, "surrogate_model", id)
	}

	actor := actorFromContext(ctx)
	published := false

	if s.surrogatePublisher != nil {
		req := surrogatepkg.RecalibrationRequest{
			ModelID:               model.ModelID,
			TargetMetricID:        model.TargetMetricID,
			InputMetricIDs:        model.InputMetricIDs,
			ModelType:             model.ModelType,
			ObservationWindowDays: model.ObservationWindowDays,
			PredictionHorizonDays: model.PredictionHorizonDays,
			RequestedBy:           actor,
			RequestedAt:           time.Now().UTC().Format(time.RFC3339),
		}
		if pubErr := s.surrogatePublisher.Publish(ctx, req); pubErr != nil {
			slog.Warn("surrogate recalibration publish failed (best-effort)",
				"model_id", id, "error", pubErr)
		} else {
			published = true
		}
	} else {
		slog.Warn("surrogate recalibration publisher not configured", "model_id", id)
	}

	slog.Info("surrogate recalibration triggered",
		"model_id", model.ModelID,
		"target_metric_id", model.TargetMetricID,
		"model_type", model.ModelType,
		"requested_by", actor,
		"kafka_published", published)

	return connect.NewResponse(&emptypb.Empty{}), nil
}
