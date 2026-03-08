package handlers

import (
	"context"
	"log/slog"

	"connectrpc.com/connect"
	"github.com/google/uuid"
	"google.golang.org/protobuf/types/known/emptypb"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/store"
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

	// Audit trail: use model_id as experiment_id field for surrogate operations.
	_ = s.audit.Insert(ctx, nil, store.AuditEntry{
		ExperimentID: created.ModelID,
		Action:       "create_surrogate_model",
		ActorEmail:   "system",
	})

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

	// Verify model exists.
	_, err := s.surrogates.GetByID(ctx, id)
	if err != nil {
		return nil, wrapDBError(err, "surrogate_model", id)
	}

	// Audit trail entry for the recalibration trigger.
	_ = s.audit.Insert(ctx, nil, store.AuditEntry{
		ExperimentID: id,
		Action:       "trigger_surrogate_recalibration",
		ActorEmail:   "system",
	})

	// TODO: Publish Kafka event or call Agent-3 to trigger actual recalibration.
	slog.Info("surrogate recalibration triggered", "model_id", id)

	return connect.NewResponse(&emptypb.Empty{}), nil
}
