package handler

import (
	"context"
	"fmt"
	"testing"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

// alwaysFailExecutor always returns an error on both methods.
type alwaysFailExecutor struct{}

func (e *alwaysFailExecutor) ExecuteSQL(_ context.Context, _ string) (*spark.SQLResult, error) {
	return nil, fmt.Errorf("forced executor failure")
}

func (e *alwaysFailExecutor) ExecuteAndWrite(_ context.Context, _ string, _ string) (*spark.SQLResult, error) {
	return nil, fmt.Errorf("forced executor failure")
}

// failingInputProvider always returns an error on Fetch.
type failingInputProvider struct{}

func (p *failingInputProvider) Fetch(_ context.Context, _ string) (surrogate.InputMetrics, error) {
	return nil, fmt.Errorf("forced input provider failure")
}

func loadTestCfg(t *testing.T) *config.ConfigStore {
	t.Helper()
	cs, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	return cs
}

func newTestRenderer(t *testing.T) *spark.SQLRenderer {
	t.Helper()
	r, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	return r
}

// --- Empty ID validation ---

func TestExportNotebook_EmptyID(t *testing.T) {
	client, _ := setupTestServer(t)
	_, err := client.ExportNotebook(context.Background(), connect.NewRequest(
		&metricsv1.ExportNotebookRequest{ExperimentId: ""}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestGetQueryLog_EmptyID(t *testing.T) {
	client, _ := setupTestServer(t)
	_, err := client.GetQueryLog(context.Background(), connect.NewRequest(
		&metricsv1.GetQueryLogRequest{ExperimentId: ""}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestGetQueryLog_NoResults(t *testing.T) {
	client, _ := setupTestServer(t)
	// Valid experiment but no computation run → empty query log.
	resp, err := client.GetQueryLog(context.Background(), connect.NewRequest(
		&metricsv1.GetQueryLogRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.NoError(t, err)
	assert.Len(t, resp.Msg.GetEntries(), 0)
}

// --- Sub-job error paths ---

func TestComputeMetrics_ContentConsumptionFailure(t *testing.T) {
	cfg := loadTestCfg(t)
	renderer := newTestRenderer(t)
	workingExec := spark.NewMockExecutor(500)
	failExec := &alwaysFailExecutor{}
	qlWriter := querylog.NewMemWriter()

	stdJob := jobs.NewStandardJob(cfg, renderer, workingExec, qlWriter)
	// Content consumption will fail immediately.
	ccj := jobs.NewContentConsumptionJob(cfg, renderer, failExec, qlWriter)
	h := NewMetricsHandler(stdJob, nil, ccj, nil, nil, nil, qlWriter)

	_, err := h.ComputeMetrics(context.Background(), connect.NewRequest(
		&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}

func TestComputeMetrics_SurrogateJobFailure(t *testing.T) {
	cfg := loadTestCfg(t)
	renderer := newTestRenderer(t)
	workingExec := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()

	stdJob := jobs.NewStandardJob(cfg, renderer, workingExec, qlWriter)
	ccj := jobs.NewContentConsumptionJob(cfg, renderer, workingExec, qlWriter)
	// Surrogate job with a failing input provider.
	sj := jobs.NewSurrogateJob(cfg, renderer, &failingInputProvider{},
		qlWriter, surrogate.NewMockModelLoader(), surrogate.NewMemProjectionWriter())
	h := NewMetricsHandler(stdJob, nil, ccj, sj, nil, nil, qlWriter)

	// e1 has a surrogate model → surrogate job attempts to fetch → fails.
	_, err := h.ComputeMetrics(context.Background(), connect.NewRequest(
		&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}

func TestComputeMetrics_InterleavingJobFailure(t *testing.T) {
	cfg := loadTestCfg(t)
	renderer := newTestRenderer(t)
	workingExec := spark.NewMockExecutor(500)
	failExec := &alwaysFailExecutor{}
	qlWriter := querylog.NewMemWriter()

	stdJob := jobs.NewStandardJob(cfg, renderer, workingExec, qlWriter)
	// Interleaving job will fail when executing SQL.
	ilj := jobs.NewInterleavingJob(cfg, renderer, failExec, qlWriter)
	h := NewMetricsHandler(stdJob, nil, nil, nil, ilj, nil, qlWriter)

	// e3 is INTERLEAVING type → interleaving job attempts to execute → fails.
	_, err := h.ComputeMetrics(context.Background(), connect.NewRequest(
		&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000003"}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}

// --- Handler with nil sub-jobs ---

func TestComputeMetrics_NilSubJobs(t *testing.T) {
	cfg := loadTestCfg(t)
	renderer := newTestRenderer(t)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()

	stdJob := jobs.NewStandardJob(cfg, renderer, executor, qlWriter)
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := jobs.NewMockValueProvider()
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03)
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.008)
	gj := jobs.NewGuardrailJob(cfg, renderer, executor, qlWriter, publisher, tracker, vp)
	// All sub-jobs nil.
	h := NewMetricsHandler(stdJob, gj, nil, nil, nil, nil, qlWriter)

	// ComputeMetrics should succeed without content consumption, surrogate, or interleaving.
	resp, err := h.ComputeMetrics(context.Background(), connect.NewRequest(
		&metricsv1.ComputeMetricsRequest{ExperimentId: "e0000000-0000-0000-0000-000000000001"}))
	require.NoError(t, err)
	assert.Equal(t, int32(4), resp.Msg.GetMetricsComputed())
}

// --- Guardrail empty ID ---

func TestComputeGuardrailMetrics_NotFound(t *testing.T) {
	client, _ := setupTestServer(t)
	_, err := client.ComputeGuardrailMetrics(context.Background(), connect.NewRequest(
		&metricsv1.ComputeGuardrailMetricsRequest{ExperimentId: "nonexistent-experiment"}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err))
}
