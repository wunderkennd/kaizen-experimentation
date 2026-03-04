package handlers

import (
	"context"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"

	"github.com/org/experimentation-platform/services/management/internal/store"
)

// CreateLayer creates a new traffic layer.
func (s *ExperimentService) CreateLayer(
	ctx context.Context,
	req *connect.Request[mgmtv1.CreateLayerRequest],
) (*connect.Response[commonv1.Layer], error) {
	l := req.Msg.GetLayer()
	if l.GetName() == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	row := store.LayerProtoToRow(l)
	created, err := s.layers.InsertLayer(ctx, nil, row)
	if err != nil {
		return nil, wrapDBError(err, "layer", row.Name)
	}

	return connect.NewResponse(store.LayerRowToProto(created)), nil
}

// GetLayer retrieves a layer by ID.
func (s *ExperimentService) GetLayer(
	ctx context.Context,
	req *connect.Request[mgmtv1.GetLayerRequest],
) (*connect.Response[commonv1.Layer], error) {
	id := req.Msg.GetLayerId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	row, err := s.layers.GetLayerByID(ctx, id)
	if err != nil {
		return nil, wrapDBError(err, "layer", id)
	}

	return connect.NewResponse(store.LayerRowToProto(row)), nil
}

// GetLayerAllocations lists allocations for a layer.
func (s *ExperimentService) GetLayerAllocations(
	ctx context.Context,
	req *connect.Request[mgmtv1.GetLayerAllocationsRequest],
) (*connect.Response[mgmtv1.GetLayerAllocationsResponse], error) {
	id := req.Msg.GetLayerId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, nil)
	}

	rows, err := s.layers.GetAllocationsByLayer(ctx, id, req.Msg.GetIncludeReleased())
	if err != nil {
		return nil, internalError("list allocations", err)
	}

	resp := &mgmtv1.GetLayerAllocationsResponse{}
	for _, r := range rows {
		resp.Allocations = append(resp.Allocations, store.AllocationRowToProto(r))
	}

	return connect.NewResponse(resp), nil
}
