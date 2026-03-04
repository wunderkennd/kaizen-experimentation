package handler

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"
	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/export"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

func setupTestServer(t *testing.T) (metricsv1connect.MetricComputationServiceClient, *querylog.MemWriter) {
	t.Helper()

	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	stdJob := jobs.NewStandardJob(cfgStore, renderer, executor, qlWriter)
	h := NewMetricsHandler(stdJob, qlWriter)

	mux := http.NewServeMux()
	path, handler := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, handler)

	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)

	client := metricsv1connect.NewMetricComputationServiceClient(
		http.DefaultClient,
		srv.URL,
	)
	return client, qlWriter
}

func TestComputeMetrics(t *testing.T) {
	client, qlWriter := setupTestServer(t)
	ctx := context.Background()

	resp, err := client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
	}))
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", resp.Msg.GetExperimentId())
	// 4 metrics: ctr_recommendation, watch_time_minutes, stream_start_rate, rebuffer_rate (RATIO)
	assert.Equal(t, int32(4), resp.Msg.GetMetricsComputed())
	assert.NotNil(t, resp.Msg.GetCompletedAt())

	// Verify query logs: 4 daily_metric + 1 delta_method + 2 cuped_covariate = 7 entries.
	entries := qlWriter.AllEntries()
	assert.Len(t, entries, 7)
}

func TestComputeMetrics_EmptyID(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()

	_, err := client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: "",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestComputeMetrics_NotFound(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()

	_, err := client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: "nonexistent",
	}))
	require.Error(t, err)
}

func TestGetQueryLog(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()

	// First compute metrics to populate logs.
	_, err := client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
	}))
	require.NoError(t, err)

	// Now get the query log.
	resp, err := client.GetQueryLog(ctx, connect.NewRequest(&metricsv1.GetQueryLogRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
	}))
	require.NoError(t, err)
	assert.Len(t, resp.Msg.GetEntries(), 7)

	for _, entry := range resp.Msg.GetEntries() {
		assert.NotEmpty(t, entry.GetSqlText())
		assert.NotEmpty(t, entry.GetMetricId())
	}
}

func TestGetQueryLog_FilterByMetric(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()

	_, err := client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
	}))
	require.NoError(t, err)

	resp, err := client.GetQueryLog(ctx, connect.NewRequest(&metricsv1.GetQueryLogRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
		MetricId:     "watch_time_minutes",
	}))
	require.NoError(t, err)
	// watch_time_minutes has daily_metric + cuped_covariate = 2 entries
	assert.Len(t, resp.Msg.GetEntries(), 2)
	for _, entry := range resp.Msg.GetEntries() {
		assert.Equal(t, "watch_time_minutes", entry.GetMetricId())
	}
}

func TestExportNotebook(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()

	// Compute metrics first.
	_, err := client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
	}))
	require.NoError(t, err)

	// Export notebook.
	resp, err := client.ExportNotebook(ctx, connect.NewRequest(&metricsv1.ExportNotebookRequest{
		ExperimentId:   "e0000000-0000-0000-0000-000000000001",
		NotebookFormat: "jupyter",
	}))
	require.NoError(t, err)
	assert.NotEmpty(t, resp.Msg.GetFilename())
	assert.Contains(t, resp.Msg.GetFilename(), ".ipynb")

	// Verify notebook content is valid JSON.
	var nb export.Notebook
	err = json.Unmarshal(resp.Msg.GetNotebookContent(), &nb)
	require.NoError(t, err, "notebook content must be valid JSON")
	assert.Equal(t, 4, nb.NBFormat)
	// header + setup + 7 * (description + SQL) = 16 cells
	// (4 daily_metric + 1 delta_method + 2 cuped_covariate = 7 queries)
	assert.Equal(t, 16, len(nb.Cells))
}

func TestExportNotebook_NoLogs(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()

	_, err := client.ExportNotebook(ctx, connect.NewRequest(&metricsv1.ExportNotebookRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeNotFound, connect.CodeOf(err))
}

func TestComputeGuardrailMetrics_Unimplemented(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()

	_, err := client.ComputeGuardrailMetrics(ctx, connect.NewRequest(&metricsv1.ComputeGuardrailMetricsRequest{
		ExperimentId: "e0000000-0000-0000-0000-000000000001",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnimplemented, connect.CodeOf(err))
}
