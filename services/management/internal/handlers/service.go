package handlers

import (
	"context"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"
	"google.golang.org/protobuf/types/known/emptypb"
)

// Compile-time check that ExperimentService implements the handler interface.
var _ managementv1connect.ExperimentManagementServiceHandler = (*ExperimentService)(nil)

// ExperimentService implements the ExperimentManagementService ConnectRPC handler.
type ExperimentService struct {
	store    *store.ExperimentStore
	audit    *store.AuditStore
	layers   *store.LayerStore
	metrics  *store.MetricStore
	notifier *streaming.Notifier
}

// NewExperimentService creates a new handler with the given stores and notifier.
func NewExperimentService(es *store.ExperimentStore, as *store.AuditStore, ls *store.LayerStore, ms *store.MetricStore, n *streaming.Notifier) *ExperimentService {
	return &ExperimentService{store: es, audit: as, layers: ls, metrics: ms, notifier: n}
}

// --- Unimplemented stubs for targeting/surrogate RPCs ---

func (s *ExperimentService) CreateTargetingRule(_ context.Context, _ *connect.Request[mgmtv1.CreateTargetingRuleRequest]) (*connect.Response[commonv1.TargetingRule], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, nil)
}

func (s *ExperimentService) CreateSurrogateModel(_ context.Context, _ *connect.Request[mgmtv1.CreateSurrogateModelRequest]) (*connect.Response[commonv1.SurrogateModelConfig], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, nil)
}

func (s *ExperimentService) ListSurrogateModels(_ context.Context, _ *connect.Request[mgmtv1.ListSurrogateModelsRequest]) (*connect.Response[mgmtv1.ListSurrogateModelsResponse], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, nil)
}

func (s *ExperimentService) GetSurrogateCalibration(_ context.Context, _ *connect.Request[mgmtv1.GetSurrogateCalibrationRequest]) (*connect.Response[commonv1.SurrogateModelConfig], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, nil)
}

func (s *ExperimentService) TriggerSurrogateRecalibration(_ context.Context, _ *connect.Request[mgmtv1.TriggerSurrogateRecalibrationRequest]) (*connect.Response[emptypb.Empty], error) {
	return nil, connect.NewError(connect.CodeUnimplemented, nil)
}
