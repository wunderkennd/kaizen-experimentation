package handlers

import (
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"
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
}

// NewExperimentService creates a new handler with the given stores and notifier.
func NewExperimentService(es *store.ExperimentStore, as *store.AuditStore, ls *store.LayerStore, ms *store.MetricStore, ts *store.TargetingStore, ss *store.SurrogateStore, n *streaming.Notifier) *ExperimentService {
	return &ExperimentService{store: es, audit: as, layers: ls, metrics: ms, targeting: ts, surrogates: ss, notifier: n}
}

