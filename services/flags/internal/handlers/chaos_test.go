package handlers

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"runtime"
	"sync"
	"sync/atomic"
	"testing"

	"connectrpc.com/connect"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// setupChaosTest creates a test environment with a ChaosStore wrapping a MockStore.
func setupChaosTest(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.ChaosStore, *store.MockStore) {
	t.Helper()
	mock := store.NewMockStore()
	chaos := store.NewChaosStore(mock)
	svc := NewFlagService(chaos)
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, chaos, mock
}

// setupChaosTestWithAudit creates a test environment with ChaosStore + ChaosAuditStore.
func setupChaosTestWithAudit(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.ChaosStore, *store.ChaosAuditStore, *store.MockStore) {
	t.Helper()
	mock := store.NewMockStore()
	chaos := store.NewChaosStore(mock)
	auditMock := store.NewMockAuditStore(mock)
	chaosAudit := store.NewChaosAuditStore(auditMock)
	svc := NewFlagServiceWithAudit(chaos, chaosAudit)
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, chaos, chaosAudit, mock
}

// setupChaosTestWithM5 creates a test environment with ChaosStore + mock M5.
func setupChaosTestWithM5(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.ChaosStore, *store.MockStore, *mockManagementHandler) {
	t.Helper()

	mgmtHandler := &mockManagementHandler{}
	mgmtMux := http.NewServeMux()
	mgmtPath, mgmtH := managementv1connect.NewExperimentManagementServiceHandler(mgmtHandler)
	mgmtMux.Handle(mgmtPath, mgmtH)
	mgmtServer := httptest.NewServer(mgmtMux)
	t.Cleanup(mgmtServer.Close)

	mgmtClient := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient,
		mgmtServer.URL,
	)

	mock := store.NewMockStore()
	chaos := store.NewChaosStore(mock)
	svc := NewFlagServiceFull(chaos, nil, mgmtClient)
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, chaos, mock, mgmtHandler
}

func createTestFlag(t *testing.T, client flagsv1connect.FeatureFlagServiceClient, name string) string {
	t.Helper()
	resp, err := client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              name,
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)
	return resp.Msg.GetFlagId()
}

// ========================================================================
// Scenario 1: Store failure injection
// ========================================================================

func TestChaos_CreateFlag_FailureNoPartialState(t *testing.T) {
	client, chaos, mock := setupChaosTest(t)
	ctx := context.Background()

	// Inject failure on CreateFlag.
	chaos.SetFailure("CreateFlag", store.ChaosConfig{Mode: store.FailAlways})

	_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "should-not-exist",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.5,
		},
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))

	// Verify no partial state — store should be empty.
	flags, _, err := mock.ListFlags(ctx, 100, "")
	require.NoError(t, err)
	assert.Empty(t, flags)
}

func TestChaos_UpdateFlag_FailureOriginalUnchanged(t *testing.T) {
	client, chaos, _ := setupChaosTest(t)
	ctx := context.Background()

	// Create a flag successfully.
	flagID := createTestFlag(t, client, "update-chaos")

	// Get original state.
	original, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: flagID}))
	require.NoError(t, err)
	originalRollout := original.Msg.GetRolloutPercentage()

	// Inject failure on UpdateFlag.
	chaos.SetFailure("UpdateFlag", store.ChaosConfig{Mode: store.FailAlways})

	_, err = client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
		Flag: &flagsv1.Flag{
			FlagId:            flagID,
			Name:              "update-chaos",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.9,
			Enabled:           true,
		},
	}))
	assert.Error(t, err)

	// Clear failure to verify original state.
	chaos.ClearAllFailures()
	after, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: flagID}))
	require.NoError(t, err)
	assert.Equal(t, originalRollout, after.Msg.GetRolloutPercentage(), "flag should be unchanged after failed update")
}

func TestChaos_EvaluateFlag_GetFlagFailure(t *testing.T) {
	client, chaos, _ := setupChaosTest(t)
	ctx := context.Background()

	flagID := createTestFlag(t, client, "eval-chaos")

	// Inject failure on GetFlag (used by EvaluateFlag).
	chaos.SetFailure("GetFlag", store.ChaosConfig{Mode: store.FailAlways})

	_, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: flagID,
		UserId: "user_123",
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}

func TestChaos_EvaluateFlags_GetAllEnabledFailure(t *testing.T) {
	client, chaos, _ := setupChaosTest(t)
	ctx := context.Background()

	createTestFlag(t, client, "bulk-chaos")

	// Inject failure on GetAllEnabledFlags (used by EvaluateFlags).
	chaos.SetFailure("GetAllEnabledFlags", store.ChaosConfig{Mode: store.FailAlways})

	_, err := client.EvaluateFlags(ctx, connect.NewRequest(&flagsv1.EvaluateFlagsRequest{
		UserId: "user_123",
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}

// ========================================================================
// Scenario 2: PromoteToExperiment atomicity
// ========================================================================

func TestChaos_Promote_M5Fails_FlagUnlinked(t *testing.T) {
	client, _, mock, mgmtHandler := setupChaosTestWithM5(t)
	ctx := context.Background()

	flagID := createTestFlag(t, client, "promote-chaos")

	// Configure M5 to fail.
	mgmtHandler.returnErr = connect.NewError(connect.CodeUnavailable, fmt.Errorf("M5 down"))

	_, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          flagID,
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	assert.Error(t, err)

	// Flag should be unchanged — not linked to any experiment (check via store directly).
	f, err := mock.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Empty(t, f.PromotedExperimentID, "flag should not be linked after M5 failure")
}

func TestChaos_Promote_LinkFlagFails_NonFatal(t *testing.T) {
	client, chaos, mock, _ := setupChaosTestWithM5(t)
	ctx := context.Background()

	flagID := createTestFlag(t, client, "link-chaos")

	// Inject failure on LinkFlagToExperiment (called after M5 succeeds).
	chaos.SetFailure("LinkFlagToExperiment", store.ChaosConfig{Mode: store.FailAlways})

	// M5 succeeds, but link fails — should still return experiment (non-fatal).
	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          flagID,
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	require.NoError(t, err, "promote should succeed even if linkage fails")
	assert.NotEmpty(t, resp.Msg.GetExperimentId())

	// Verify flag is NOT linked (check via store directly — linkage failed).
	chaos.ClearAllFailures()
	f, err := mock.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Empty(t, f.PromotedExperimentID, "linkage should be lost after LinkFlagToExperiment failure")
}

// ========================================================================
// Scenario 3: Store recovery
// ========================================================================

func TestChaos_StoreRecovery_FailThenRecover(t *testing.T) {
	client, chaos, _ := setupChaosTest(t)
	ctx := context.Background()

	// Phase 1: Create flags successfully.
	id1 := createTestFlag(t, client, "recovery-1")
	id2 := createTestFlag(t, client, "recovery-2")

	// Phase 2: Inject failures — creates should fail.
	chaos.SetFailure("CreateFlag", store.ChaosConfig{Mode: store.FailAlways})

	_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "should-fail",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.5,
		},
	}))
	assert.Error(t, err)

	// Phase 3: Clear failures — store recovers.
	chaos.ClearAllFailures()

	id3 := createTestFlag(t, client, "recovery-3")

	// All successful flags should be retrievable.
	for _, id := range []string{id1, id2, id3} {
		resp, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: id}))
		require.NoError(t, err)
		assert.NotEmpty(t, resp.Msg.GetName())
	}

	// Failed flag should not exist.
	listResp, err := client.ListFlags(ctx, connect.NewRequest(&flagsv1.ListFlagsRequest{PageSize: 100}))
	require.NoError(t, err)
	assert.Len(t, listResp.Msg.GetFlags(), 3, "only 3 successfully created flags should exist")
}

// ========================================================================
// Scenario 4: Concurrent CRUD with random failures
// ========================================================================

func TestChaos_ConcurrentCRUDWithRandomFailures(t *testing.T) {
	client, chaos, _ := setupChaosTest(t)
	ctx := context.Background()

	// Pre-create some flags for update/evaluate targets.
	var targetIDs []string
	for i := 0; i < 5; i++ {
		id := createTestFlag(t, client, fmt.Sprintf("concurrent-target-%d", i))
		targetIDs = append(targetIDs, id)
	}

	// Inject 10% random failure on writes.
	chaos.SetFailure("CreateFlag", store.ChaosConfig{Mode: store.FailRandom, Probability: 0.1})
	chaos.SetFailure("UpdateFlag", store.ChaosConfig{Mode: store.FailRandom, Probability: 0.1})

	var wg sync.WaitGroup
	var createSuccesses, updateSuccesses, evalSuccesses atomic.Int64
	var createFailures, updateFailures, evalFailures atomic.Int64

	// 20 create goroutines.
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
				Flag: &flagsv1.Flag{
					Name:              fmt.Sprintf("chaos-create-%d", idx),
					Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
					DefaultValue:      "false",
					Enabled:           true,
					RolloutPercentage: 0.5,
				},
			}))
			if err != nil {
				createFailures.Add(1)
			} else {
				createSuccesses.Add(1)
			}
		}(i)
	}

	// 20 update goroutines.
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			targetID := targetIDs[idx%len(targetIDs)]
			_, err := client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
				Flag: &flagsv1.Flag{
					FlagId:            targetID,
					Name:              fmt.Sprintf("concurrent-target-%d", idx%len(targetIDs)),
					Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
					DefaultValue:      "false",
					Enabled:           true,
					RolloutPercentage: float64(idx%10) / 10.0,
				},
			}))
			if err != nil {
				updateFailures.Add(1)
			} else {
				updateSuccesses.Add(1)
			}
		}(i)
	}

	// 20 evaluate goroutines (no failure injection — reads should always work).
	for i := 0; i < 20; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			targetID := targetIDs[idx%len(targetIDs)]
			_, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
				FlagId: targetID,
				UserId: fmt.Sprintf("user-%d", idx),
			}))
			if err != nil {
				evalFailures.Add(1)
			} else {
				evalSuccesses.Add(1)
			}
		}(i)
	}

	wg.Wait()

	// Verify: no panics, no races (enforced by -race flag).
	t.Logf("creates: %d success / %d fail", createSuccesses.Load(), createFailures.Load())
	t.Logf("updates: %d success / %d fail", updateSuccesses.Load(), updateFailures.Load())
	t.Logf("evaluates: %d success / %d fail", evalSuccesses.Load(), evalFailures.Load())

	assert.Greater(t, createSuccesses.Load(), int64(0), "some creates should succeed")
	assert.Greater(t, updateSuccesses.Load(), int64(0), "some updates should succeed")
	assert.Equal(t, int64(20), evalSuccesses.Load()+evalFailures.Load(), "all evaluate goroutines should complete")
}

// ========================================================================
// Scenario 5: Audit store failure isolation
// ========================================================================

func TestChaos_AuditStoreFailure_CRUDStillWorks(t *testing.T) {
	client, _, chaosAudit, _ := setupChaosTestWithAudit(t)
	ctx := context.Background()

	// Fail all audit writes.
	chaosAudit.SetFailAll(fmt.Errorf("chaos: audit store down"))

	// CRUD should still work — audit failures are logged, not propagated.
	flagID := createTestFlag(t, client, "audit-chaos")

	_, err := client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
		Flag: &flagsv1.Flag{
			FlagId:            flagID,
			Name:              "audit-chaos",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.8,
			Enabled:           true,
		},
	}))
	require.NoError(t, err)

	resp, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: flagID}))
	require.NoError(t, err)
	assert.Equal(t, 0.8, resp.Msg.GetRolloutPercentage())

	eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: flagID,
		UserId: "user_456",
	}))
	require.NoError(t, err)
	assert.NotEmpty(t, eval.Msg.GetValue())
}

// ========================================================================
// Scenario 6: Context cancellation
// ========================================================================

func TestChaos_ContextCancellation_NoPartialState(t *testing.T) {
	client, _, mock := setupChaosTest(t)

	// Cancel the context before the call.
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "cancelled-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.5,
		},
	}))
	assert.Error(t, err, "canceled context should cause an error")

	// Store should have no flags.
	flags, _, err := mock.ListFlags(context.Background(), 100, "")
	require.NoError(t, err)
	assert.Empty(t, flags, "no flag should exist after canceled context")
}

func TestChaos_ContextCancellation_NoGoroutineLeak(t *testing.T) {
	client, _, _ := setupChaosTest(t)

	// Baseline goroutine count.
	runtime.GC()
	baseline := runtime.NumGoroutine()

	// Issue 20 requests with canceled contexts.
	for i := 0; i < 20; i++ {
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
			Flag: &flagsv1.Flag{
				Name:              fmt.Sprintf("leak-test-%d", i),
				Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
				DefaultValue:      "false",
				RolloutPercentage: 0.5,
			},
		}))
	}

	// Allow goroutines to settle.
	runtime.GC()
	runtime.Gosched()

	current := runtime.NumGoroutine()
	// Allow a generous margin (10 goroutines) for background activity.
	assert.LessOrEqual(t, current, baseline+10,
		"goroutine count should be stable after canceled requests (baseline=%d, current=%d)", baseline, current)
}

// ========================================================================
// Scenario 7: Server restart simulation
// ========================================================================

func TestChaos_ServerRestart_StateSurvives(t *testing.T) {
	mock := store.NewMockStore()
	ctx := context.Background()

	// Start server 1, create flags.
	chaos1 := store.NewChaosStore(mock)
	svc1 := NewFlagService(chaos1)
	mux1 := http.NewServeMux()
	path1, h1 := flagsv1connect.NewFeatureFlagServiceHandler(svc1)
	mux1.Handle(path1, h1)
	server1 := httptest.NewServer(mux1)
	client1 := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server1.URL)

	id1 := createTestFlag(t, client1, "restart-1")
	id2 := createTestFlag(t, client1, "restart-2")

	// "Stop" server 1.
	server1.Close()

	// Start server 2 with same store.
	chaos2 := store.NewChaosStore(mock)
	svc2 := NewFlagService(chaos2)
	mux2 := http.NewServeMux()
	path2, h2 := flagsv1connect.NewFeatureFlagServiceHandler(svc2)
	mux2.Handle(path2, h2)
	server2 := httptest.NewServer(mux2)
	t.Cleanup(server2.Close)
	client2 := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server2.URL)

	// Flags should be accessible from new server.
	for _, id := range []string{id1, id2} {
		resp, err := client2.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: id}))
		require.NoError(t, err)
		assert.NotEmpty(t, resp.Msg.GetName())
	}

	// Can create new flags on new server.
	id3 := createTestFlag(t, client2, "restart-3")
	resp, err := client2.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: id3}))
	require.NoError(t, err)
	assert.Equal(t, "restart-3", resp.Msg.GetName())
}

func TestChaos_ServerRestart_MultiCycle(t *testing.T) {
	mock := store.NewMockStore()
	ctx := context.Background()

	var allIDs []string

	for cycle := 0; cycle < 3; cycle++ {
		chaos := store.NewChaosStore(mock)
		svc := NewFlagService(chaos)
		mux := http.NewServeMux()
		path, h := flagsv1connect.NewFeatureFlagServiceHandler(svc)
		mux.Handle(path, h)
		server := httptest.NewServer(mux)
		client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)

		// Create a flag in this cycle.
		id := createTestFlag(t, client, fmt.Sprintf("cycle-%d-flag", cycle))
		allIDs = append(allIDs, id)

		// Verify all flags from previous cycles are accessible.
		for _, prevID := range allIDs {
			resp, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: prevID}))
			require.NoError(t, err, "flag %s from previous cycle should be accessible", prevID)
			assert.NotEmpty(t, resp.Msg.GetName())
		}

		// Verify total flag count.
		list, err := client.ListFlags(ctx, connect.NewRequest(&flagsv1.ListFlagsRequest{PageSize: 100}))
		require.NoError(t, err)
		assert.Len(t, list.Msg.GetFlags(), cycle+1, "should have %d flags after cycle %d", cycle+1, cycle)

		// "Restart" — close server.
		server.Close()
	}
}
