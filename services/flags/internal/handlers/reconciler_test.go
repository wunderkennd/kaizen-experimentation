package handlers

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// reconcilerMgmtHandler supports CreateExperiment, GetExperiment, and returning errors.
type reconcilerMgmtHandler struct {
	managementv1connect.UnimplementedExperimentManagementServiceHandler
	experiments map[string]*commonv1.Experiment
	getErr      error
}

func newReconcilerMgmtHandler() *reconcilerMgmtHandler {
	return &reconcilerMgmtHandler{
		experiments: make(map[string]*commonv1.Experiment),
	}
}

func (m *reconcilerMgmtHandler) CreateExperiment(_ context.Context, req *connect.Request[mgmtv1.CreateExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	exp := req.Msg.GetExperiment()
	result := &commonv1.Experiment{
		ExperimentId: fmt.Sprintf("exp-%d", len(m.experiments)+1),
		Name:         exp.GetName(),
		State:        commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT,
		Variants:     exp.GetVariants(),
	}
	m.experiments[result.ExperimentId] = result
	return connect.NewResponse(result), nil
}

func (m *reconcilerMgmtHandler) GetExperiment(_ context.Context, req *connect.Request[mgmtv1.GetExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	if m.getErr != nil {
		return nil, m.getErr
	}
	exp, ok := m.experiments[req.Msg.GetExperimentId()]
	if !ok {
		return nil, connect.NewError(connect.CodeNotFound, fmt.Errorf("experiment not found"))
	}
	return connect.NewResponse(exp), nil
}

type reconcilerEnv struct {
	client     flagsv1connect.FeatureFlagServiceClient
	mockStore  *store.MockStore
	auditStore *store.MockAuditStore
	mgmtH     *reconcilerMgmtHandler
	mgmtClient managementv1connect.ExperimentManagementServiceClient
}

func setupReconcilerTest(t *testing.T) *reconcilerEnv {
	t.Helper()

	mgmtH := newReconcilerMgmtHandler()
	mgmtMux := http.NewServeMux()
	path, handler := managementv1connect.NewExperimentManagementServiceHandler(mgmtH)
	mgmtMux.Handle(path, handler)
	mgmtServer := httptest.NewServer(mgmtMux)
	t.Cleanup(mgmtServer.Close)

	mgmtClient := managementv1connect.NewExperimentManagementServiceClient(http.DefaultClient, mgmtServer.URL)

	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)
	svc := NewFlagServiceFull(mockStore, auditStore, mgmtClient, "default")

	mux := http.NewServeMux()
	flagPath, flagHandler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(flagPath, flagHandler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)

	return &reconcilerEnv{
		client:     client,
		mockStore:  mockStore,
		auditStore: auditStore,
		mgmtH:     mgmtH,
		mgmtClient: mgmtClient,
	}
}

func createAndPromoteFlagForReconciler(t *testing.T, env *reconcilerEnv, name string) (string, string) {
	t.Helper()
	ctx := context.Background()

	created, err := env.client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              name,
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	resp, err := env.client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	require.NoError(t, err)

	return created.Msg.GetFlagId(), resp.Msg.GetExperimentId()
}

func TestReconciler_ResolvesCompletedExperiment(t *testing.T) {
	env := setupReconcilerTest(t)
	ctx := context.Background()

	flagID, expID := createAndPromoteFlagForReconciler(t, env, "reconcile-concluded")
	env.mgmtH.experiments[expID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	r := NewReconciler(env.mockStore, env.auditStore, env.mgmtClient, time.Minute, ResolutionRolloutFull)
	r.RunOnce(ctx)

	f, err := env.mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 1.0, f.RolloutPercentage)
	assert.True(t, f.Enabled)
	assert.False(t, f.ResolvedAt.IsZero())
}

func TestReconciler_SkipsRunningExperiment(t *testing.T) {
	env := setupReconcilerTest(t)
	ctx := context.Background()

	flagID, expID := createAndPromoteFlagForReconciler(t, env, "reconcile-running")
	env.mgmtH.experiments[expID].State = commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING

	r := NewReconciler(env.mockStore, env.auditStore, env.mgmtClient, time.Minute, ResolutionRolloutFull)
	r.RunOnce(ctx)

	f, err := env.mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 0.5, f.RolloutPercentage) // unchanged
	assert.True(t, f.ResolvedAt.IsZero())      // not resolved
}

func TestReconciler_SkipsAlreadyResolved(t *testing.T) {
	env := setupReconcilerTest(t)
	ctx := context.Background()

	flagID, expID := createAndPromoteFlagForReconciler(t, env, "reconcile-already-resolved")
	env.mgmtH.experiments[expID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	// Manually set ResolvedAt to simulate already-resolved flag.
	env.mockStore.SetResolvedAt(flagID, time.Now().Add(-time.Hour))

	r := NewReconciler(env.mockStore, env.auditStore, env.mgmtClient, time.Minute, ResolutionRolloutFull)
	r.RunOnce(ctx)

	f, err := env.mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 0.5, f.RolloutPercentage) // unchanged — skipped
}

func TestReconciler_RollbackAction(t *testing.T) {
	env := setupReconcilerTest(t)
	ctx := context.Background()

	flagID, expID := createAndPromoteFlagForReconciler(t, env, "reconcile-rollback")
	env.mgmtH.experiments[expID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	r := NewReconciler(env.mockStore, env.auditStore, env.mgmtClient, time.Minute, ResolutionRollback)
	r.RunOnce(ctx)

	f, err := env.mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 0.0, f.RolloutPercentage)
	assert.False(t, f.Enabled)
	assert.False(t, f.ResolvedAt.IsZero())
}

func TestReconciler_HandlesM5Error(t *testing.T) {
	env := setupReconcilerTest(t)
	ctx := context.Background()

	flagID, _ := createAndPromoteFlagForReconciler(t, env, "reconcile-m5-error")
	env.mgmtH.getErr = connect.NewError(connect.CodeInternal, fmt.Errorf("service unavailable"))

	r := NewReconciler(env.mockStore, env.auditStore, env.mgmtClient, time.Minute, ResolutionRolloutFull)
	r.RunOnce(ctx) // Should not panic.

	f, err := env.mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 0.5, f.RolloutPercentage) // unchanged
	assert.True(t, f.ResolvedAt.IsZero())      // not resolved
}

func TestReconciler_NoManagementClient(t *testing.T) {
	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)

	r := NewReconciler(mockStore, auditStore, nil, time.Minute, ResolutionRolloutFull)

	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	// Start returns immediately when management client is nil.
	r.Start(ctx)
}

func TestReconciler_ArchivedExperiment(t *testing.T) {
	env := setupReconcilerTest(t)
	ctx := context.Background()

	flagID, expID := createAndPromoteFlagForReconciler(t, env, "reconcile-archived")
	env.mgmtH.experiments[expID].State = commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED

	r := NewReconciler(env.mockStore, env.auditStore, env.mgmtClient, time.Minute, ResolutionRolloutFull)
	r.RunOnce(ctx)

	f, err := env.mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 1.0, f.RolloutPercentage)
	assert.True(t, f.Enabled)
	assert.False(t, f.ResolvedAt.IsZero())
}

func TestReconciler_AuditEntry(t *testing.T) {
	env := setupReconcilerTest(t)
	ctx := context.Background()

	flagID, expID := createAndPromoteFlagForReconciler(t, env, "reconcile-audit")
	env.mgmtH.experiments[expID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	r := NewReconciler(env.mockStore, env.auditStore, env.mgmtClient, time.Minute, ResolutionRolloutFull)
	r.RunOnce(ctx)

	entries, err := env.auditStore.GetFlagAuditLog(ctx, flagID, 100)
	require.NoError(t, err)

	actions := make(map[string]bool)
	for _, e := range entries {
		actions[e.Action] = true
	}
	assert.True(t, actions["promote_to_experiment"], "should have promote audit entry")
	assert.True(t, actions["auto_resolve_experiment"], "should have auto_resolve audit entry")

	// Verify the auto_resolve entry has the reconciler actor.
	for _, e := range entries {
		if e.Action == "auto_resolve_experiment" {
			assert.Equal(t, "system/reconciler", e.ActorEmail)
		}
	}
}
