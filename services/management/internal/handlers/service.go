package handlers

import (
	"context"

	"github.com/org/experimentation/gen/go/experimentation/analysis/v1/analysisv1connect"
	"github.com/org/experimentation/gen/go/experimentation/bandit/v1/banditv1connect"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"
	"github.com/org/experimentation-platform/services/management/internal/surrogate"
)

// Compile-time check that ExperimentService implements the handler interface.
var _ managementv1connect.ExperimentManagementServiceHandler = (*ExperimentService)(nil)

// ExperimentService implements the ExperimentManagementService ConnectRPC handler.
type ExperimentService struct {
	store      *store.ExperimentStore
	audit      *store.AuditStore
	layers     *store.LayerStore
	metrics    *store.MetricStore
	targeting  *store.TargetingStore
	surrogates *store.SurrogateStore
	notifier   *streaming.Notifier

	// Optional external service clients (nil = graceful degradation).
	analysisClient     analysisv1connect.AnalysisServiceClient
	banditClient       banditv1connect.BanditPolicyServiceClient
	surrogatePublisher surrogate.Publisher
}

// ServiceOption configures optional dependencies on ExperimentService.
type ServiceOption func(*ExperimentService)

// WithAnalysisClient sets the M4a analysis service client.
func WithAnalysisClient(c analysisv1connect.AnalysisServiceClient) ServiceOption {
	return func(s *ExperimentService) { s.analysisClient = c }
}

// WithBanditClient sets the M4b bandit policy service client.
func WithBanditClient(c banditv1connect.BanditPolicyServiceClient) ServiceOption {
	return func(s *ExperimentService) { s.banditClient = c }
}

// WithSurrogatePublisher sets the Kafka publisher for surrogate recalibration requests.
func WithSurrogatePublisher(p surrogate.Publisher) ServiceOption {
	return func(s *ExperimentService) { s.surrogatePublisher = p }
}

// NewExperimentService creates a new handler with the given stores and notifier.
func NewExperimentService(es *store.ExperimentStore, as *store.AuditStore, ls *store.LayerStore, ms *store.MetricStore, ts *store.TargetingStore, ss *store.SurrogateStore, n *streaming.Notifier, opts ...ServiceOption) *ExperimentService {
	svc := &ExperimentService{store: es, audit: as, layers: ls, metrics: ms, targeting: ts, surrogates: ss, notifier: n}
	for _, o := range opts {
		o(svc)
	}
	return svc
}

// ConcludeByID implements the sequential.Concluder interface, exposing the
// internal concludeByID method for the auto-conclude consumer.
func (s *ExperimentService) ConcludeByID(ctx context.Context, id, actor string, extraDetails map[string]any) error {
	_, err := s.concludeByID(ctx, id, actor, extraDetails)
	return err
}

