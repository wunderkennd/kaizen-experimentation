package handlers

import (
	"context"
	"log/slog"

	"connectrpc.com/connect"
	"github.com/google/uuid"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/validation"
)

// CreateMetricDefinition validates the request, inserts the metric definition,
// and returns the created metric.
func (s *ExperimentService) CreateMetricDefinition(
	ctx context.Context,
	req *connect.Request[mgmtv1.CreateMetricDefinitionRequest],
) (*connect.Response[commonv1.MetricDefinition], error) {
	m := req.Msg.GetMetric()

	if err := validation.ValidateCreateMetricDefinition(m); err != nil {
		return nil, err
	}

	row := store.MetricDefinitionToRow(m)

	// Generate metric_id if not provided.
	if row.MetricID == "" {
		row.MetricID = uuid.NewString()
	}

	created, err := s.metrics.Insert(ctx, row)
	if err != nil {
		return nil, wrapDBError(err, "metric_definition", row.MetricID)
	}

	slog.Info("metric definition created", "metric_id", created.MetricID, "name", created.Name, "type", created.Type)
	return connect.NewResponse(store.RowToMetricDefinition(created)), nil
}

// GetMetricDefinition retrieves a single metric definition by ID.
func (s *ExperimentService) GetMetricDefinition(
	ctx context.Context,
	req *connect.Request[mgmtv1.GetMetricDefinitionRequest],
) (*connect.Response[commonv1.MetricDefinition], error) {
	id := req.Msg.GetMetricId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	row, err := s.metrics.GetByID(ctx, id)
	if err != nil {
		return nil, wrapDBError(err, "metric_definition", id)
	}

	return connect.NewResponse(store.RowToMetricDefinition(row)), nil
}

// ListMetricDefinitions returns a paginated list of metric definitions.
func (s *ExperimentService) ListMetricDefinitions(
	ctx context.Context,
	req *connect.Request[mgmtv1.ListMetricDefinitionsRequest],
) (*connect.Response[mgmtv1.ListMetricDefinitionsResponse], error) {
	typeFilter := store.MetricTypeToString(req.Msg.GetTypeFilter())
	rows, nextToken, err := s.metrics.ListMetrics(ctx, req.Msg.GetPageSize(), req.Msg.GetPageToken(), typeFilter)
	if err != nil {
		return nil, internalError("list metric definitions", err)
	}

	resp := &mgmtv1.ListMetricDefinitionsResponse{
		NextPageToken: nextToken,
	}
	for _, row := range rows {
		resp.Metrics = append(resp.Metrics, store.RowToMetricDefinition(row))
	}

	return connect.NewResponse(resp), nil
}
