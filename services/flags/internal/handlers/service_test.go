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
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func setupTest(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.MockStore) {
	t.Helper()
	mockStore := store.NewMockStore()
	svc := NewFlagService(mockStore)
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, mockStore
}

func TestCreateAndGetFlag(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	resp, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "test-flag",
			Description:       "A test flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)
	assert.NotEmpty(t, resp.Msg.GetFlagId())
	assert.Equal(t, "test-flag", resp.Msg.GetName())
	assert.Equal(t, flagsv1.FlagType_FLAG_TYPE_BOOLEAN, resp.Msg.GetType())
	assert.Equal(t, 0.5, resp.Msg.GetRolloutPercentage())

	getResp, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{
		FlagId: resp.Msg.GetFlagId(),
	}))
	require.NoError(t, err)
	assert.Equal(t, resp.Msg.GetFlagId(), getResp.Msg.GetFlagId())
	assert.Equal(t, "test-flag", getResp.Msg.GetName())
}

func TestUpdateFlag(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "update-me",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.1,
		},
	}))
	require.NoError(t, err)

	updated, err := client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
		Flag: &flagsv1.Flag{
			FlagId:            created.Msg.GetFlagId(),
			Name:              "update-me",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.2,
			Enabled:           true,
		},
	}))
	require.NoError(t, err)
	assert.Equal(t, 0.2, updated.Msg.GetRolloutPercentage())
	assert.True(t, updated.Msg.GetEnabled())
}

func TestListFlags(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	for i := 0; i < 3; i++ {
		_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
			Flag: &flagsv1.Flag{
				Name:              "flag-" + string(rune('a'+i)),
				Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
				DefaultValue:      "false",
				RolloutPercentage: 0.5,
			},
		}))
		require.NoError(t, err)
	}

	resp, err := client.ListFlags(ctx, connect.NewRequest(&flagsv1.ListFlagsRequest{
		PageSize: 10,
	}))
	require.NoError(t, err)
	assert.Len(t, resp.Msg.GetFlags(), 3)
}

func TestEvaluateFlag_Disabled(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "disabled-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           false,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)

	eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "user_123",
	}))
	require.NoError(t, err)
	assert.Equal(t, "false", eval.Msg.GetValue())
}

func TestEvaluateFlag_FullRollout(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "full-rollout",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)

	eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "user_123",
	}))
	require.NoError(t, err)
	assert.Equal(t, "true", eval.Msg.GetValue())
}

func TestEvaluateFlag_ZeroRollout(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "zero-rollout",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.0,
		},
	}))
	require.NoError(t, err)

	eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "user_123",
	}))
	require.NoError(t, err)
	assert.Equal(t, "false", eval.Msg.GetValue())
}

func TestEvaluateFlag_Deterministic(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "deterministic-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	var results []string
	for i := 0; i < 10; i++ {
		eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
			FlagId: created.Msg.GetFlagId(),
			UserId: "user_deterministic",
		}))
		require.NoError(t, err)
		results = append(results, eval.Msg.GetValue())
	}

	for _, r := range results {
		assert.Equal(t, results[0], r, "evaluation must be deterministic")
	}
}

func TestEvaluateFlags_Bulk(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	for _, name := range []string{"bulk-a", "bulk-b"} {
		_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
			Flag: &flagsv1.Flag{
				Name:              name,
				Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
				DefaultValue:      "false",
				Enabled:           true,
				RolloutPercentage: 1.0,
			},
		}))
		require.NoError(t, err)
	}

	resp, err := client.EvaluateFlags(ctx, connect.NewRequest(&flagsv1.EvaluateFlagsRequest{
		UserId: "user_bulk",
	}))
	require.NoError(t, err)
	assert.Len(t, resp.Msg.GetEvaluations(), 2)
}

func TestPromoteToExperiment(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "promote-me",
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
		PrimaryMetricId: "click_through_rate",
	}))
	require.NoError(t, err)
	assert.NotEmpty(t, resp.Msg.GetExperimentId())
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, resp.Msg.GetState())
	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_AB, resp.Msg.GetType())
	assert.Len(t, resp.Msg.GetVariants(), 2)
}

func TestPromoteToExperiment_DisabledFlag(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "disabled-promote",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           false,
			RolloutPercentage: 0.1,
		},
	}))
	require.NoError(t, err)

	_, err = client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeFailedPrecondition, connect.CodeOf(err))
}

func TestValidation_EmptyName(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:         "",
			Type:         flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue: "false",
		},
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestValidation_InvalidBooleanDefault(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:         "bad-bool",
			Type:         flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue: "maybe",
		},
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestDeleteFlag(t *testing.T) {
	client, mockStore := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "delete-me",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.0,
		},
	}))
	require.NoError(t, err)

	err = mockStore.DeleteFlag(ctx, created.Msg.GetFlagId())
	require.NoError(t, err)

	_, err = client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{
		FlagId: created.Msg.GetFlagId(),
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeNotFound, connect.CodeOf(err))
}
