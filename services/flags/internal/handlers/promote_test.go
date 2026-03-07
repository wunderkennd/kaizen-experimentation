package handlers

import (
	"context"
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

	// Simulate M5 behavior: copy input, assign ID, set state to DRAFT, generate salt.
	exp := req.Msg.GetExperiment()
	result := &commonv1.Experiment{
		ExperimentId:        "exp-from-m5-001",
		Name:                exp.GetName(),
		Description:         exp.GetDescription(),
		OwnerEmail:          exp.GetOwnerEmail(),
		Type:                exp.GetType(),
		State:               commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT,
		Variants:            exp.GetVariants(),
		PrimaryMetricId:     exp.GetPrimaryMetricId(),
		SecondaryMetricIds:  exp.GetSecondaryMetricIds(),
		TargetingRuleId:     exp.GetTargetingRuleId(),
		HashSalt:            "m5-generated-salt",
		SessionConfig:       exp.GetSessionConfig(),
		InterleavingConfig:  exp.GetInterleavingConfig(),
		BanditConfig:        exp.GetBanditConfig(),
		IsCumulativeHoldout: exp.GetIsCumulativeHoldout(),
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
	svc := NewFlagServiceFull(mockStore, nil, mgmtClient)
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

func TestPromoteToExperiment_SessionLevel(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-session",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL,
		PrimaryMetricId: "session_engagement",
	}))
	require.NoError(t, err)

	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL, resp.Msg.GetType())

	// Verify session_config defaults.
	sc := mgmtHandler.lastRequest.GetSessionConfig()
	require.NotNil(t, sc)
	assert.Equal(t, "session_id", sc.GetSessionIdAttribute())
	assert.True(t, sc.GetAllowCrossSessionVariation())
	assert.Equal(t, int32(1), sc.GetMinSessionsPerUser())
}

func TestPromoteToExperiment_Interleaving(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	// Flag with 3 algorithm variants.
	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-interleaving",
			Type:              flagsv1.FlagType_FLAG_TYPE_STRING,
			DefaultValue:      "algo_baseline",
			Enabled:           true,
			RolloutPercentage: 1.0,
			Variants: []*flagsv1.FlagVariant{
				{Value: "algo_baseline", TrafficFraction: 0.34},
				{Value: "algo_candidate_1", TrafficFraction: 0.33},
				{Value: "algo_candidate_2", TrafficFraction: 0.33},
			},
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING,
		PrimaryMetricId: "ndcg_at_10",
	}))
	require.NoError(t, err)

	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING, resp.Msg.GetType())

	// Verify interleaving_config defaults.
	ic := mgmtHandler.lastRequest.GetInterleavingConfig()
	require.NotNil(t, ic)
	assert.Equal(t, commonv1.InterleavingMethod_INTERLEAVING_METHOD_TEAM_DRAFT, ic.GetMethod())
	assert.Equal(t, commonv1.CreditAssignment_CREDIT_ASSIGNMENT_BINARY_WIN, ic.GetCreditAssignment())
	assert.Equal(t, int32(50), ic.GetMaxListSize())
	// Algorithm IDs should be populated from flag variants.
	require.Len(t, ic.GetAlgorithmIds(), 3)
	assert.Equal(t, "algo_baseline", ic.GetAlgorithmIds()[0])
	assert.Equal(t, "algo_candidate_1", ic.GetAlgorithmIds()[1])
	assert.Equal(t, "algo_candidate_2", ic.GetAlgorithmIds()[2])
}

func TestPromoteToExperiment_InterleavingNoVariants(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	// Boolean flag with no explicit variants — should synthesize algorithm IDs.
	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-interleaving-simple",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING,
		PrimaryMetricId: "clicks",
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING, resp.Msg.GetType())

	ic := mgmtHandler.lastRequest.GetInterleavingConfig()
	require.NotNil(t, ic)
	assert.Len(t, ic.GetAlgorithmIds(), 2)
	assert.Equal(t, "algorithm_control", ic.GetAlgorithmIds()[0])
	assert.Equal(t, "algorithm_treatment", ic.GetAlgorithmIds()[1])
}

func TestPromoteToExperiment_MAB(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-mab",
			Type:              flagsv1.FlagType_FLAG_TYPE_STRING,
			DefaultValue:      "layout_a",
			Enabled:           true,
			RolloutPercentage: 1.0,
			Variants: []*flagsv1.FlagVariant{
				{Value: "layout_a", TrafficFraction: 0.5},
				{Value: "layout_b", TrafficFraction: 0.5},
			},
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_MAB,
		PrimaryMetricId: "click_through_rate",
	}))
	require.NoError(t, err)

	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_MAB, resp.Msg.GetType())

	bc := mgmtHandler.lastRequest.GetBanditConfig()
	require.NotNil(t, bc)
	assert.Equal(t, commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING, bc.GetAlgorithm())
	assert.Equal(t, "click_through_rate", bc.GetRewardMetricId())
	assert.InDelta(t, 0.1, bc.GetMinExplorationFraction(), 0.001)
	assert.Equal(t, int32(1000), bc.GetWarmupObservations())

	// Arms from flag variants.
	arms := bc.GetArms()
	require.Len(t, arms, 2)
	assert.Equal(t, "layout_a", arms[0].GetName())
	assert.Equal(t, "layout_b", arms[1].GetName())
}

func TestPromoteToExperiment_ContextualBandit(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-contextual",
			Type:              flagsv1.FlagType_FLAG_TYPE_STRING,
			DefaultValue:      "model_v1",
			Enabled:           true,
			RolloutPercentage: 1.0,
			Variants: []*flagsv1.FlagVariant{
				{Value: "model_v1", TrafficFraction: 0.5},
				{Value: "model_v2", TrafficFraction: 0.5},
			},
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT,
		PrimaryMetricId: "watch_time",
	}))
	require.NoError(t, err)

	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT, resp.Msg.GetType())

	bc := mgmtHandler.lastRequest.GetBanditConfig()
	require.NotNil(t, bc)
	// Contextual bandit defaults to LINEAR_UCB.
	assert.Equal(t, commonv1.BanditAlgorithm_BANDIT_ALGORITHM_LINEAR_UCB, bc.GetAlgorithm())
	assert.Equal(t, "watch_time", bc.GetRewardMetricId())
	assert.Len(t, bc.GetArms(), 2)
}

func TestPromoteToExperiment_CumulativeHoldout(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-holdout",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.95,
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT,
		PrimaryMetricId: "retention_d7",
	}))
	require.NoError(t, err)

	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT, resp.Msg.GetType())
	assert.True(t, resp.Msg.GetIsCumulativeHoldout())

	// Verify M5 received is_cumulative_holdout = true.
	assert.True(t, mgmtHandler.lastRequest.GetIsCumulativeHoldout())
}

func TestPromoteToExperiment_PlaybackQoE(t *testing.T) {
	client, _, mgmtHandler := setupTestWithM5(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-qoe",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	resp, err := client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_PLAYBACK_QOE,
		PrimaryMetricId: "rebuffer_rate",
	}))
	require.NoError(t, err)

	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_PLAYBACK_QOE, resp.Msg.GetType())
	// No type-specific config for QoE — guardrails are added via UpdateExperiment.
	assert.Nil(t, mgmtHandler.lastRequest.GetSessionConfig())
	assert.Nil(t, mgmtHandler.lastRequest.GetInterleavingConfig())
	assert.Nil(t, mgmtHandler.lastRequest.GetBanditConfig())
}
