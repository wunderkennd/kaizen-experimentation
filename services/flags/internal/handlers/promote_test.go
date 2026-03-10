package handlers

import (
	"context"
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

// mockManagementHandler is a test double for Agent-5's ExperimentManagementService.
type mockManagementHandler struct {
	managementv1connect.UnimplementedExperimentManagementServiceHandler
	lastRequest *commonv1.Experiment
	returnErr   error
}

func (m *mockManagementHandler) CreateExperiment(ctx context.Context, req *connect.Request[mgmtv1.CreateExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	if m.returnErr != nil {
		return nil, m.returnErr
	}
	m.lastRequest = req.Msg.GetExperiment()

	// Contract validation: M5 requires these fields to be non-empty.
	exp := req.Msg.GetExperiment()
	if exp.GetLayerId() == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("layer_id is required"))
	}
	if exp.GetName() == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("name is required"))
	}
	if exp.GetOwnerEmail() == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("owner_email is required"))
	}
	if exp.GetPrimaryMetricId() == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("primary_metric_id is required"))
	}
	result := &commonv1.Experiment{
		ExperimentId:       "exp-from-m5-001",
		Name:               exp.GetName(),
		Description:        exp.GetDescription(),
		OwnerEmail:         exp.GetOwnerEmail(),
		Type:               exp.GetType(),
		State:              commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT,
		Variants:           exp.GetVariants(),
		PrimaryMetricId:    exp.GetPrimaryMetricId(),
		SecondaryMetricIds: exp.GetSecondaryMetricIds(),
		TargetingRuleId:    exp.GetTargetingRuleId(),
		HashSalt:           "m5-generated-salt",
	}

	// M5 assigns variant IDs.
	for i, v := range result.Variants {
		v.VariantId = "m5-var-" + string(rune('a'+i))
	}

	return connect.NewResponse(result), nil
}

// setupTestWithM5 creates a test environment with a real mock M5 management service.
func setupTestWithM5(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.MockStore, *mockManagementHandler) {
	t.Helper()

	// Start mock M5 management service.
	mgmtHandler := &mockManagementHandler{}
	mgmtMux := http.NewServeMux()
	mgmtPath, mgmtH := managementv1connect.NewExperimentManagementServiceHandler(mgmtHandler)
	mgmtMux.Handle(mgmtPath, mgmtH)
	mgmtServer := httptest.NewServer(mgmtMux)
	t.Cleanup(mgmtServer.Close)

	// Create management client pointing to mock M5.
	mgmtClient := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient,
		mgmtServer.URL,
	)

	// Create flag service with management client.
	mockStore := store.NewMockStore()
	svc := NewFlagServiceFull(mockStore, nil, mgmtClient, "default")
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, mockStore, mgmtHandler
}

func TestPromoteToExperiment_LiveM5(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	// Create an enabled flag.
	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-live",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.3,
		},
	}))
	require.NoError(t, err)

	// Promote to experiment — should call real M5.
	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:             created.Msg.GetFlagId(),
		ExperimentType:     commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId:    "click_through_rate",
		SecondaryMetricIds: []string{"session_duration"},
	}))
	require.NoError(t, err)

	// Verify response came from M5 (not mock).
	assert.Equal(t, "exp-from-m5-001", resp.Msg.GetExperimentId())
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, resp.Msg.GetState())
	assert.Equal(t, "m5-generated-salt", resp.Msg.GetHashSalt())
	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_AB, resp.Msg.GetType())
	assert.Len(t, resp.Msg.GetVariants(), 2)
	assert.Equal(t, "click_through_rate", resp.Msg.GetPrimaryMetricId())
	assert.Equal(t, []string{"session_duration"}, resp.Msg.GetSecondaryMetricIds())

	// Verify M5 received correct experiment data.
	require.NotNil(t, mgmtHandler.lastRequest)
	assert.Contains(t, mgmtHandler.lastRequest.GetName(), "promote-live")
	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_AB, mgmtHandler.lastRequest.GetType())
	assert.Equal(t, "click_through_rate", mgmtHandler.lastRequest.GetPrimaryMetricId())
	assert.Equal(t, "default", mgmtHandler.lastRequest.GetLayerId())

	// Verify variants: control (70%) + treatment (30%) from rollout percentage.
	variants := mgmtHandler.lastRequest.GetVariants()
	require.Len(t, variants, 2)
	assert.Equal(t, "control", variants[0].GetName())
	assert.InDelta(t, 0.7, variants[0].GetTrafficFraction(), 0.001)
	assert.True(t, variants[0].GetIsControl())
	assert.Equal(t, "treatment", variants[1].GetName())
	assert.InDelta(t, 0.3, variants[1].GetTrafficFraction(), 0.001)
	assert.False(t, variants[1].GetIsControl())
}

func TestPromoteToExperiment_M5Error(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	// Configure M5 to return an error.
	mgmtHandler.returnErr = connect.NewError(connect.CodeInvalidArgument, assert.AnError)

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-error",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	// Promote should fail — flag state unchanged.
	_, err = client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}

func TestPromoteToExperiment_MockFallback(t *testing.T) {
	// Use setupTest (no management client) — should fall back to mock.
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-mock",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.1,
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	require.NoError(t, err)

	// Mock generates UUID-format experiment ID (not "exp-from-m5-001").
	assert.NotEqual(t, "exp-from-m5-001", resp.Msg.GetExperimentId())
	assert.NotEmpty(t, resp.Msg.GetExperimentId())
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, resp.Msg.GetState())
	assert.Len(t, resp.Msg.GetVariants(), 2)
}

func TestPromoteToExperiment_WithVariants(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "multi-variant-promote",
			Type:              flagsv1.FlagType_FLAG_TYPE_STRING,
			DefaultValue:      "red",
			Enabled:           true,
			RolloutPercentage: 1.0,
			Variants: []*flagsv1.FlagVariant{
				{Value: "red", TrafficFraction: 0.34},
				{Value: "blue", TrafficFraction: 0.33},
				{Value: "green", TrafficFraction: 0.33},
			},
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_MULTIVARIATE,
		PrimaryMetricId: "conversion",
	}))
	require.NoError(t, err)

	assert.Equal(t, "exp-from-m5-001", resp.Msg.GetExperimentId())
	assert.Len(t, resp.Msg.GetVariants(), 3)

	// Verify M5 received 3 variants with correct fractions.
	require.NotNil(t, mgmtHandler.lastRequest)
	variants := mgmtHandler.lastRequest.GetVariants()
	require.Len(t, variants, 3)
	assert.Equal(t, "variant_0", variants[0].GetName())
	assert.True(t, variants[0].GetIsControl())
	assert.InDelta(t, 0.34, variants[0].GetTrafficFraction(), 0.001)
	assert.Equal(t, "variant_1", variants[1].GetName())
	assert.False(t, variants[1].GetIsControl())
}
