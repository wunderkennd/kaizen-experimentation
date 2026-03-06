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
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
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
	ccj := jobs.NewContentConsumptionJob(cfgStore, renderer, executor, qlWriter)
	// Surrogate job: provide mock inputs so projections are computed for homepage_recs_v2.
	mockInputs := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"watch_time_minutes": 45.0, "stream_start_rate": 0.8},
		"f0000000-0000-0000-0000-000000000002": {"watch_time_minutes": 52.0, "stream_start_rate": 0.85},
	}
	surrInputProvider := &jobs.MockInputMetricsProvider{Inputs: mockInputs}
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()
	sj := jobs.NewSurrogateJob(cfgStore, renderer, surrInputProvider, qlWriter, modelLoader, projWriter)
	ilj := jobs.NewInterleavingJob(cfgStore, renderer, executor, qlWriter)
	h := NewMetricsHandler(stdJob, gj, ccj, sj, ilj, qlWriter)
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
	// 4 daily_metric + 1 delta_method + 2 cuped_covariate + 4 daily_treatment_effect + 1 content_consumption + 1 surrogate_input = 13
	assert.Len(t, qlWriter.AllEntries(), 13)
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
	// 4 daily_metric + 1 delta_method + 2 cuped_covariate + 4 daily_treatment_effect + 1 content_consumption + 1 surrogate_input = 13
	assert.Len(t, resp.Msg.GetEntries(), 13)
}

func TestGetQueryLog_FilterByMetric(t *testing.T) {
	client, _ := setupTestServer(t)
	ctx := context.Background()
	_, _ = client.ComputeMetrics(ctx, connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	resp, err := client.GetQueryLog(ctx, connect.NewRequest(&metricsv1.GetQueryLogRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001", MetricId: "watch_time_minutes"}))
	require.NoError(t, err)
	// daily_metric + cuped_covariate + daily_treatment_effect = 3
	assert.Len(t, resp.Msg.GetEntries(), 3)
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
	// 2 header cells + 2 * 13 query entries = 28 cells
	assert.Equal(t, 28, len(nb.Cells))
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

func TestComputeMetrics_InterleavingExperiment(t *testing.T) {
	client, qlWriter := setupTestServer(t)
	resp, err := client.ComputeMetrics(context.Background(), connect.NewRequest(&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000003"}))
	require.NoError(t, err)
	// search_ranking_interleave has search_success_rate (PROPORTION) + ctr_recommendation (PROPORTION + CUPED) = 2 metrics
	assert.Equal(t, int32(2), resp.Msg.GetMetricsComputed())

	// Verify interleaving score query was logged
	entries := qlWriter.AllEntries()
	interleavingCount := 0
	for _, e := range entries {
		if e.JobType == "interleaving_score" {
			interleavingCount++
		}
	}
	assert.Equal(t, 1, interleavingCount, "Should have 1 interleaving_score query log entry")
}
