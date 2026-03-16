package handlers

import (
	"time"

	"github.com/org/experimentation-platform/services/flags/internal/store"
	"github.com/org/experimentation-platform/services/flags/internal/telemetry"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
)

// FlagService implements the FeatureFlagServiceHandler interface.
type FlagService struct {
	flagsv1connect.UnimplementedFeatureFlagServiceHandler
	store            store.Store
	auditStore       store.AuditStore
	managementClient managementv1connect.ExperimentManagementServiceClient
	defaultLayerID   string
	metrics          *telemetry.Metrics
}

// NewFlagService creates a new FlagService.
func NewFlagService(s store.Store) *FlagService {
	return &FlagService{store: s}
}

// NewFlagServiceWithAudit creates a new FlagService with audit trail support.
func NewFlagServiceWithAudit(s store.Store, a store.AuditStore) *FlagService {
	return &FlagService{store: s, auditStore: a}
}

// NewFlagServiceFull creates a FlagService with all dependencies.
// If defaultLayerID is empty, it defaults to "default".
func NewFlagServiceFull(s store.Store, a store.AuditStore, mc managementv1connect.ExperimentManagementServiceClient, defaultLayerID string) *FlagService {
	if defaultLayerID == "" {
		defaultLayerID = "default"
	}
	return &FlagService{store: s, auditStore: a, managementClient: mc, defaultLayerID: defaultLayerID}
}

// WithMetrics returns the FlagService with metrics instrumentation attached.
func (s *FlagService) WithMetrics(m *telemetry.Metrics) *FlagService {
	s.metrics = m
	return s
}

// NewReconciler creates a Reconciler using the FlagService's dependencies.
func (s *FlagService) NewReconciler(interval time.Duration, defaultAction ResolutionAction) *Reconciler {
	return NewReconcilerWithMetrics(s.store, s.auditStore, s.managementClient, interval, defaultAction, s.metrics)
}
