package handlers

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

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

// fullMockManagementHandler supports both CreateExperiment and GetExperiment.
type fullMockManagementHandler struct {
	managementv1connect.UnimplementedExperimentManagementServiceHandler
	experiments map[string]*commonv1.Experiment
}

func newFullMockManagementHandler() *fullMockManagementHandler {
	return &fullMockManagementHandler{
		experiments: make(map[string]*commonv1.Experiment),
	}
}

func (m *fullMockManagementHandler) CreateExperiment(_ context.Context, req *connect.Request[mgmtv1.CreateExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	exp := req.Msg.GetExperiment()
	result := &commonv1.Experiment{
		ExperimentId:       fmt.Sprintf("exp-%d", len(m.experiments)+1),
		Name:               exp.GetName(),
		Description:        exp.GetDescription(),
		OwnerEmail:         exp.GetOwnerEmail(),
		Type:               exp.GetType(),
		State:              commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT,
		Variants:           exp.GetVariants(),
		PrimaryMetricId:    exp.GetPrimaryMetricId(),
		SecondaryMetricIds: exp.GetSecondaryMetricIds(),
		HashSalt:           "test-salt",
	}
	m.experiments[result.ExperimentId] = result
	return connect.NewResponse(result), nil
}

func (m *fullMockManagementHandler) GetExperiment(_ context.Context, req *connect.Request[mgmtv1.GetExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	exp, ok := m.experiments[req.Msg.GetExperimentId()]
	if !ok {
		return nil, connect.NewError(connect.CodeNotFound, fmt.Errorf("experiment not found"))
	}
	return connect.NewResponse(exp), nil
}

// setupTestWithLinkage creates a test environment with linkage routes registered.
func setupTestWithLinkage(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.MockStore, *fullMockManagementHandler, *httptest.Server) {
	t.Helper()

	mgmtHandler := newFullMockManagementHandler()
	mgmtMux := http.NewServeMux()
	mgmtPath, mgmtH := managementv1connect.NewExperimentManagementServiceHandler(mgmtHandler)
	mgmtMux.Handle(mgmtPath, mgmtH)
	mgmtServer := httptest.NewServer(mgmtMux)
	t.Cleanup(mgmtServer.Close)

	mgmtClient := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, mgmtServer.URL,
	)

	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)
	svc := NewFlagServiceFull(mockStore, auditStore, mgmtClient, "default")

	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	svc.RegisterAuditRoutes(mux)
	svc.RegisterLinkageRoutes(mux)

	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, mockStore, mgmtHandler, server
}

// createAndPromoteFlag is a helper that creates an enabled flag and promotes it.
func createAndPromoteFlag(t *testing.T, client flagsv1connect.FeatureFlagServiceClient, name string) (string, string) {
	t.Helper()
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              name,
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	require.NoError(t, err)

	return created.Msg.GetFlagId(), resp.Msg.GetExperimentId()
}

func TestPromoteToExperiment_LinksFlagToExperiment(t *testing.T) {
	client, mockStore, _, _ := setupTestWithLinkage(t)
	ctx := context.Background()

	flagID, experimentID := createAndPromoteFlag(t, client, "linkage-test")

	// The flag should now have promoted_experiment_id set.
	f, err := mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, experimentID, f.PromotedExperimentID)
	assert.False(t, f.PromotedAt.IsZero())
}

func TestGetFlagByExperiment(t *testing.T) {
	client, mockStore, _, _ := setupTestWithLinkage(t)
	ctx := context.Background()

	flagID, experimentID := createAndPromoteFlag(t, client, "by-experiment-test")

	f, err := mockStore.GetFlagByExperiment(ctx, experimentID)
	require.NoError(t, err)
	assert.Equal(t, flagID, f.FlagID)
	assert.Equal(t, "by-experiment-test", f.Name)
}

func TestGetFlagByExperiment_NotFound(t *testing.T) {
	_, mockStore, _, _ := setupTestWithLinkage(t)
	ctx := context.Background()

	_, err := mockStore.GetFlagByExperiment(ctx, "nonexistent-experiment")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "no flag found")
}

func TestGetPromotedFlags(t *testing.T) {
	client, mockStore, _, _ := setupTestWithLinkage(t)
	ctx := context.Background()

	// Create 3 flags, promote 2.
	createAndPromoteFlag(t, client, "promoted-1")
	createAndPromoteFlag(t, client, "promoted-2")

	_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "not-promoted",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	promoted, err := mockStore.GetPromotedFlags(ctx)
	require.NoError(t, err)
	assert.Len(t, promoted, 2)
}

func TestGetFlagsByTargetingRule(t *testing.T) {
	_, mockStore, _, _ := setupTestWithLinkage(t)
	ctx := context.Background()

	ruleID := "rule-abc-123"

	// Create 3 flags: 2 with the targeting rule, 1 without.
	_, err := mockStore.CreateFlag(ctx, &store.Flag{
		Name: "rule-flag-a", Type: "BOOLEAN", DefaultValue: "false",
		TargetingRuleID: ruleID,
	})
	require.NoError(t, err)

	_, err = mockStore.CreateFlag(ctx, &store.Flag{
		Name: "rule-flag-b", Type: "BOOLEAN", DefaultValue: "false",
		TargetingRuleID: ruleID,
	})
	require.NoError(t, err)

	_, err = mockStore.CreateFlag(ctx, &store.Flag{
		Name: "no-rule-flag", Type: "BOOLEAN", DefaultValue: "false",
	})
	require.NoError(t, err)

	flags, err := mockStore.GetFlagsByTargetingRule(ctx, ruleID)
	require.NoError(t, err)
	assert.Len(t, flags, 2)

	// Empty result for unknown rule.
	flags, err = mockStore.GetFlagsByTargetingRule(ctx, "unknown-rule")
	require.NoError(t, err)
	assert.Len(t, flags, 0)
}

func TestHandleGetPromotedFlags(t *testing.T) {
	client, _, _, server := setupTestWithLinkage(t)

	createAndPromoteFlag(t, client, "http-promoted-1")
	createAndPromoteFlag(t, client, "http-promoted-2")

	resp, err := http.Get(server.URL + "/internal/flags/promoted")
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	var flags []map[string]any
	require.NoError(t, json.NewDecoder(resp.Body).Decode(&flags))
	assert.Len(t, flags, 2)
	assert.NotEmpty(t, flags[0]["promoted_experiment_id"])
}

func TestHandleGetFlagsByTargetingRule(t *testing.T) {
	_, mockStore, _, server := setupTestWithLinkage(t)
	ctx := context.Background()

	ruleID := "targeting-rule-xyz"
	_, err := mockStore.CreateFlag(ctx, &store.Flag{
		Name: "tr-flag", Type: "BOOLEAN", DefaultValue: "false",
		TargetingRuleID: ruleID, Enabled: true,
	})
	require.NoError(t, err)

	// With rule_id.
	resp, err := http.Get(server.URL + "/internal/flags/by-targeting-rule?rule_id=" + ruleID)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	var flags []map[string]any
	require.NoError(t, json.NewDecoder(resp.Body).Decode(&flags))
	assert.Len(t, flags, 1)
	assert.Equal(t, "tr-flag", flags[0]["name"])

	// Missing rule_id → 400.
	resp2, err := http.Get(server.URL + "/internal/flags/by-targeting-rule")
	require.NoError(t, err)
	defer resp2.Body.Close()
	assert.Equal(t, http.StatusBadRequest, resp2.StatusCode)
}

func TestResolvePromotedExperiment_RolloutFull(t *testing.T) {
	client, mockStore, mgmtHandler, server := setupTestWithLinkage(t)
	ctx := context.Background()

	flagID, experimentID := createAndPromoteFlag(t, client, "resolve-full")

	// Simulate experiment conclusion.
	mgmtHandler.experiments[experimentID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id="+flagID+"&action=rollout_full",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	var result map[string]any
	require.NoError(t, json.NewDecoder(resp.Body).Decode(&result))
	assert.Equal(t, "rollout_full", result["action"])
	assert.Equal(t, float64(1.0), result["rollout_percentage"])
	assert.Equal(t, true, result["flag_enabled"])

	// Verify flag was actually updated.
	f, err := mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 1.0, f.RolloutPercentage)
	assert.True(t, f.Enabled)
}

func TestResolvePromotedExperiment_Rollback(t *testing.T) {
	client, mockStore, mgmtHandler, server := setupTestWithLinkage(t)
	ctx := context.Background()

	flagID, experimentID := createAndPromoteFlag(t, client, "resolve-rollback")

	mgmtHandler.experiments[experimentID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id="+flagID+"&action=rollback",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	f, err := mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 0.0, f.RolloutPercentage)
	assert.False(t, f.Enabled)
}

func TestResolvePromotedExperiment_Keep(t *testing.T) {
	client, mockStore, mgmtHandler, server := setupTestWithLinkage(t)
	ctx := context.Background()

	flagID, experimentID := createAndPromoteFlag(t, client, "resolve-keep")

	mgmtHandler.experiments[experimentID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id="+flagID+"&action=keep",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	// Flag should be unchanged.
	f, err := mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 0.5, f.RolloutPercentage)
	assert.True(t, f.Enabled)
}

func TestResolvePromotedExperiment_NotConcluded(t *testing.T) {
	client, _, _, server := setupTestWithLinkage(t)

	flagID, _ := createAndPromoteFlag(t, client, "resolve-running")

	// Experiment is still DRAFT (not concluded).
	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id="+flagID+"&action=rollout_full",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusConflict, resp.StatusCode)
}

func TestResolvePromotedExperiment_NotPromoted(t *testing.T) {
	client, _, _, server := setupTestWithLinkage(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "never-promoted",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id="+created.Msg.GetFlagId()+"&action=rollout_full",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusBadRequest, resp.StatusCode)
}

func TestResolvePromotedExperiment_InvalidAction(t *testing.T) {
	_, _, _, server := setupTestWithLinkage(t)

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id=abc&action=invalid",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusBadRequest, resp.StatusCode)
}

func TestResolvePromotedExperiment_MissingFlagID(t *testing.T) {
	_, _, _, server := setupTestWithLinkage(t)

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?action=rollout_full",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusBadRequest, resp.StatusCode)
}

func TestResolvePromotedExperiment_Archived(t *testing.T) {
	client, mockStore, mgmtHandler, server := setupTestWithLinkage(t)
	ctx := context.Background()

	flagID, experimentID := createAndPromoteFlag(t, client, "resolve-archived")

	// ARCHIVED is also a valid terminal state for resolution.
	mgmtHandler.experiments[experimentID].State = commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id="+flagID+"&action=rollout_full",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	f, err := mockStore.GetFlag(ctx, flagID)
	require.NoError(t, err)
	assert.Equal(t, 1.0, f.RolloutPercentage)
}

func TestResolvePromotedExperiment_AuditEntry(t *testing.T) {
	client, _, mgmtHandler, server := setupTestWithLinkage(t)

	flagID, experimentID := createAndPromoteFlag(t, client, "resolve-audit")
	mgmtHandler.experiments[experimentID].State = commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED

	resp, err := http.Post(
		server.URL+"/internal/flags/resolve?flag_id="+flagID+"&action=rollout_full",
		"application/json", nil,
	)
	require.NoError(t, err)
	defer resp.Body.Close()
	assert.Equal(t, http.StatusOK, resp.StatusCode)

	// Check that audit entries include both promote_to_experiment and resolve_experiment.
	auditResp, err := http.Get(server.URL + "/internal/flags/audit?flag_id=" + flagID)
	require.NoError(t, err)
	defer auditResp.Body.Close()

	var entries []map[string]any
	require.NoError(t, json.NewDecoder(auditResp.Body).Decode(&entries))

	actions := make(map[string]bool)
	for _, e := range entries {
		actions[e["action"].(string)] = true
	}
	assert.True(t, actions["promote_to_experiment"], "should have promote audit entry")
	assert.True(t, actions["resolve_experiment"], "should have resolve audit entry")
}
