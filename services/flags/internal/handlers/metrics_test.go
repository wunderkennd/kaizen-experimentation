package handlers

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"connectrpc.com/connect"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	"github.com/org/experimentation-platform/services/flags/internal/telemetry"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func setupTestWithMetrics(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.MockStore, *httptest.Server) {
	t.Helper()
	t.Setenv("OTEL_EXPORTER_OTLP_ENDPOINT", "")

	metrics, cleanup, err := telemetry.Init(context.Background())
	require.NoError(t, err)
	t.Cleanup(cleanup)

	mockStore := store.NewMockStore()
	svc := NewFlagService(mockStore).WithMetrics(metrics)

	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	mux.Handle("/metrics", telemetry.PrometheusHandler())
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)
	return client, mockStore, server
}

func TestMetrics_EvaluateFlag_RecordsCounter(t *testing.T) {
	client, _, server := setupTestWithMetrics(t)
	ctx := context.Background()

	// Create an enabled flag with 100% rollout.
	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "metrics-test-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)

	// Evaluate the flag.
	_, err = client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "user_metrics_test",
	}))
	require.NoError(t, err)

	// Scrape the /metrics endpoint.
	resp, err := http.Get(server.URL + "/metrics")
	require.NoError(t, err)
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	require.NoError(t, err)
	metricsBody := string(body)

	assert.Contains(t, metricsBody, "flag_evaluations_total")
	assert.Contains(t, metricsBody, `result="treatment"`)
}

func TestMetrics_EvaluateFlag_ControlOutcome(t *testing.T) {
	client, _, server := setupTestWithMetrics(t)
	ctx := context.Background()

	// Create an enabled flag with 0% rollout — all users get control.
	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "metrics-control-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.0,
		},
	}))
	require.NoError(t, err)

	_, err = client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "user_control_test",
	}))
	require.NoError(t, err)

	resp, err := http.Get(server.URL + "/metrics")
	require.NoError(t, err)
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	require.NoError(t, err)

	assert.Contains(t, string(body), `result="control"`)
}

func TestMetrics_NilSafe(t *testing.T) {
	// Ensure handlers work without metrics (no panic).
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "nil-metrics-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)

	// These should not panic even without metrics.
	_, err = client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "user_nil_metrics",
	}))
	assert.NoError(t, err)

	_, err = client.EvaluateFlags(ctx, connect.NewRequest(&flagsv1.EvaluateFlagsRequest{
		UserId: "user_nil_metrics",
	}))
	assert.NoError(t, err)
}
