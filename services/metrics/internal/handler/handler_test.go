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

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
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
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := jobs.NewMockValueProvider()
	vp.SetVariantValue("rebuffer_rate", "f0000000-0000-0000-0000-000000000001", 0.02)
	vp.SetVariantValue("rebuffer_rate", "f0000000-0000-0000-0000-000000000002", 0.03)
	vp.SetVariantValue("error_rate", "f0000000-0000-0000-0000-000000000001", 0.005)
	vp.SetVariantValue("error_rate", "f0000000-0000-0000-0000-000000000002", 0.008)
	gj := jobs.NewGuardrailJob(cfgStore, renderer, executor, qlWriter, publisher, tracker, vp)
	h := NewMetricsHandler(stdJob, gj, qlWriter)
	mux := http.NewServeMux()
	path, handler := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, handler)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	client := metricsv1connect.NewMetricComputationServiceClient(http.DefaultClient, srv.URL)
	return client, qlWriter
}

func TestComputeMetrics(t *testing.T) {
	client, qlWriter := setupTestServer(t)
	resp, err := client.ComputeMetrics(context.Background(), connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.NoError(t, err)
	assert.Equal(t, int32(4), resp.Msg.GetMetricsComputed())
	assert.Len(t, qlWriter.AllEntries(), 7)
}

func TestComputeMetrics_EmptyID(t *testing.T) {
	client, _ := setupTestServer(t)
	_, err := client.ComputeMetrics(context.Background(), connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: ""}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestComputeMetrics_NotFound(t *testing.T) {
	client, _ := setupTestServer(t)
	_, err := client.ComputeMetrics(context.Background(), connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: "nonexistent"}))
	require.Error(t, err)
}

func TestGetQueryLog(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()
	_, _ = client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	resp, err := client.GetQueryLog(ctx, connect.NewRequest(&metricsv1.GetQueryLogRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.NoError(t, err)
	assert.Len(t, resp.Msg.GetEntries(), 7)
}

func TestGetQueryLog_FilterByMetric(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()
	_, _ = client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	resp, err := client.GetQueryLog(ctx, connect.NewRequest(&metricsv1.GetQueryLogRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001", MetricId: "watch_time_minutes"}))
	require.NoError(t, err)
	assert.Len(t, resp.Msg.GetEntries(), 2)
}

func TestExportNotebook(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()
	_, _ = client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	resp, err := client.ExportNotebook(ctx, connect.NewRequest(&metricsv1.ExportNotebookRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001", NotebookFormat: "jupyter"}))
	require.NoError(t, err)
	assert.Contains(t, resp.Msg.GetFilename(), ".ipynb")
	var nb export.Notebook
	err = json.Unmarshal(resp.Msg.GetNotebookContent(), &nb)
	require.NoError(t, err)
	assert.Equal(t, 4, nb.NBFormat)
	assert.Equal(t, 16, len(nb.Cells))
}

func TestExportNotebook_NoLogs(t *testing.T) {
	client, _ := setupTestServer(t)
	_, err := client.ExportNotebook(context.Background(), connect.NewRequest(&metricsv1.ExportNotebookRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeNotFound, connect.CodeOf(err))
}

func TestComputeGuardrailMetrics_Success(t *testing.T) {
	client, qlWriter := setupTestServer(t)
	resp, err := client.ComputeGuardrailMetrics(context.Background(), connect.NewRequest(&metricsv1.ComputeGuardrailMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.NoError(t, err)
	assert.Equal(t, int32(2), resp.Msg.GetMetricsComputed())
	guardrailCount := 0
	for _, e := range qlWriter.AllEntries() {
		if e.JobType == "hourly_guardrail" { guardrailCount++ }
	}
	assert.Equal(t, 2, guardrailCount)
}

func TestComputeGuardrailMetrics_EmptyID(t *testing.T) {
	client, _ := setupTestServer(t)
	_, err := client.ComputeGuardrailMetrics(context.Background(), connect.NewRequest(&metricsv1.ComputeGuardrailMetricsRequest{ExperimentId: ""}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestComputeGuardrailMetrics_NoGuardrails(t *testing.T) {
	client, _ := setupTestServer(t)
	resp, err := client.ComputeGuardrailMetrics(context.Background(), connect.NewRequest(&metricsv1.ComputeGuardrailMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000003"}))
	require.NoError(t, err)
	assert.Equal(t, int32(0), resp.Msg.GetMetricsComputed())
}
