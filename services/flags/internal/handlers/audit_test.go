package handlers

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func setupTestWithAudit(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.MockStore, *store.MockAuditStore) {
	t.Helper()
	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)
	svc := NewFlagServiceWithAudit(mockStore, auditStore)
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	svc.RegisterAuditRoutes(mux)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, mockStore, auditStore
}

func TestAudit_CreateFlagRecordsEntry(t *testing.T) {
	client, _, auditStore := setupTestWithAudit(t)
	ctx := context.Background()

	resp, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "audit-create",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	entries := auditStore.Entries()
	require.Len(t, entries, 1)
	assert.Equal(t, "create", entries[0].Action)
	assert.Equal(t, resp.Msg.GetFlagId(), entries[0].FlagID)
	assert.Equal(t, "system", entries[0].ActorEmail)

	var newVal map[string]any
	require.NoError(t, json.Unmarshal(entries[0].NewValue, &newVal))
	assert.Equal(t, "audit-create", newVal["name"])
	assert.Equal(t, float64(0.5), newVal["rollout_percentage"])
}

func TestAudit_UpdateFlagRecordsEntry(t *testing.T) {
	client, _, auditStore := setupTestWithAudit(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "audit-update",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.1,
		},
	}))
	require.NoError(t, err)

	_, err = client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
		Flag: &flagsv1.Flag{
			FlagId:            created.Msg.GetFlagId(),
			Name:              "audit-update",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	entries := auditStore.Entries()
	require.Len(t, entries, 2)
	assert.Equal(t, "rollout_change", entries[1].Action)

	var prevVal, newVal map[string]any
	require.NoError(t, json.Unmarshal(entries[1].PreviousValue, &prevVal))
	require.NoError(t, json.Unmarshal(entries[1].NewValue, &newVal))
	assert.Equal(t, 0.1, prevVal["rollout_percentage"])
	assert.Equal(t, 0.5, newVal["rollout_percentage"])
}

func TestAudit_EnableDisableRecordsSpecificAction(t *testing.T) {
	client, _, auditStore := setupTestWithAudit(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "audit-toggle",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           false,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	_, err = client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
		Flag: &flagsv1.Flag{
			FlagId:            created.Msg.GetFlagId(),
			Name:              "audit-toggle",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	entries := auditStore.Entries()
	require.Len(t, entries, 2)
	assert.Equal(t, "enable", entries[1].Action)

	_, err = client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
		Flag: &flagsv1.Flag{
			FlagId:            created.Msg.GetFlagId(),
			Name:              "audit-toggle",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           false,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)

	entries = auditStore.Entries()
	require.Len(t, entries, 3)
	assert.Equal(t, "disable", entries[2].Action)
}

func TestAudit_PromoteToExperimentRecordsEntry(t *testing.T) {
	client, _, auditStore := setupTestWithAudit(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "audit-promote",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.1,
		},
	}))
	require.NoError(t, err)

	_, err = client.PromoteToExperiment(ctx, connect.NewRequest(&flagsv1.PromoteToExperimentRequest{
		FlagId:          created.Msg.GetFlagId(),
		ExperimentType:  commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		PrimaryMetricId: "ctr",
	}))
	require.NoError(t, err)

	entries := auditStore.Entries()
	require.Len(t, entries, 2)
	assert.Equal(t, "promote_to_experiment", entries[1].Action)
}

func TestAudit_GetFlagAuditLog(t *testing.T) {
	client, _, auditStore := setupTestWithAudit(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "audit-log-test",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.1,
		},
	}))
	require.NoError(t, err)

	for _, pct := range []float64{0.2, 0.3} {
		_, err = client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
			Flag: &flagsv1.Flag{
				FlagId:            created.Msg.GetFlagId(),
				Name:              "audit-log-test",
				Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
				DefaultValue:      "false",
				RolloutPercentage: pct,
			},
		}))
		require.NoError(t, err)
	}

	log, err := auditStore.GetFlagAuditLog(ctx, created.Msg.GetFlagId(), 10)
	require.NoError(t, err)
	assert.Len(t, log, 3)
	assert.Equal(t, "rollout_change", log[0].Action)
	assert.Equal(t, "rollout_change", log[1].Action)
	assert.Equal(t, "create", log[2].Action)
}

func TestStaleFlags_DetectsOldFullRollout(t *testing.T) {
	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)
	ctx := context.Background()

	flag, err := mockStore.CreateFlag(ctx, &store.Flag{
		Name:              "stale-flag",
		Type:              "BOOLEAN",
		DefaultValue:      "false",
		Enabled:           true,
		RolloutPercentage: 1.0,
	})
	require.NoError(t, err)

	mockStore.SetUpdatedAt(flag.FlagID, time.Now().Add(-100*24*time.Hour))

	stale, err := auditStore.GetStaleFlags(ctx, 90*24*time.Hour)
	require.NoError(t, err)
	require.Len(t, stale, 1)
	assert.Equal(t, "stale-flag", stale[0].Name)
	assert.True(t, stale[0].StaleDuration > 90*24*time.Hour)
}

func TestStaleFlags_DoesNotDetectRecentFlags(t *testing.T) {
	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)
	ctx := context.Background()

	_, err := mockStore.CreateFlag(ctx, &store.Flag{
		Name:              "fresh-flag",
		Type:              "BOOLEAN",
		DefaultValue:      "false",
		Enabled:           true,
		RolloutPercentage: 1.0,
	})
	require.NoError(t, err)

	stale, err := auditStore.GetStaleFlags(ctx, 90*24*time.Hour)
	require.NoError(t, err)
	assert.Len(t, stale, 0)
}

func TestStaleFlags_DoesNotDetectPartialRollout(t *testing.T) {
	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)
	ctx := context.Background()

	flag, err := mockStore.CreateFlag(ctx, &store.Flag{
		Name:              "partial-flag",
		Type:              "BOOLEAN",
		DefaultValue:      "false",
		Enabled:           true,
		RolloutPercentage: 0.5,
	})
	require.NoError(t, err)

	mockStore.SetUpdatedAt(flag.FlagID, time.Now().Add(-100*24*time.Hour))

	stale, err := auditStore.GetStaleFlags(ctx, 90*24*time.Hour)
	require.NoError(t, err)
	assert.Len(t, stale, 0)
}

func TestStaleFlags_DoesNotDetectDisabledFlags(t *testing.T) {
	mockStore := store.NewMockStore()
	auditStore := store.NewMockAuditStore(mockStore)
	ctx := context.Background()

	flag, err := mockStore.CreateFlag(ctx, &store.Flag{
		Name:              "disabled-stale-flag",
		Type:              "BOOLEAN",
		DefaultValue:      "false",
		Enabled:           false,
		RolloutPercentage: 1.0,
	})
	require.NoError(t, err)

	mockStore.SetUpdatedAt(flag.FlagID, time.Now().Add(-100*24*time.Hour))

	stale, err := auditStore.GetStaleFlags(ctx, 90*24*time.Hour)
	require.NoError(t, err)
	assert.Len(t, stale, 0)
}

func TestAudit_NoAuditStoreDoesNotFail(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	_, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "no-audit-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)
}
