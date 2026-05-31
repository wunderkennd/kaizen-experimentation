package handler

import (
	"context"
	"database/sql"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"connectrpc.com/connect"
	"github.com/google/uuid"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"
	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/shadow"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

// setupShadowTestServer spins up a handler with a real MockStore wired in,
// alongside the full set of other deps required by setupTestServer.
func setupShadowTestServer(t *testing.T) (metricsv1connect.MetricComputationServiceClient, *shadow.MockStore) {
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
	gj := jobs.NewGuardrailJob(cfgStore, renderer, executor, qlWriter, publisher, tracker, vp)
	ccj := jobs.NewContentConsumptionJob(cfgStore, renderer, executor, qlWriter)
	mockInputs := surrogate.InputMetrics{}
	surrInputProvider := &jobs.MockInputMetricsProvider{Inputs: mockInputs}
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()
	sj := jobs.NewSurrogateJob(cfgStore, renderer, surrInputProvider, qlWriter, modelLoader, projWriter)
	ilj := jobs.NewInterleavingJob(cfgStore, renderer, executor, qlWriter)

	mockStore := shadow.NewMockStore()
	h := NewMetricsHandler(stdJob, gj, ccj, sj, ilj, nil, qlWriter, WithShadowStore(mockStore))
	mux := http.NewServeMux()
	path, handler := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, handler)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	client := metricsv1connect.NewMetricComputationServiceClient(http.DefaultClient, srv.URL)
	return client, mockStore
}

// setupNoStoreTestServer returns a handler without a shadow store wired (tests
// CodeUnavailable behaviour when shadow store is nil).
func setupNoStoreTestServer(t *testing.T) metricsv1connect.MetricComputationServiceClient {
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
	gj := jobs.NewGuardrailJob(cfgStore, renderer, executor, qlWriter, publisher, tracker, vp)
	ccj := jobs.NewContentConsumptionJob(cfgStore, renderer, executor, qlWriter)
	mockInputs := surrogate.InputMetrics{}
	surrInputProvider := &jobs.MockInputMetricsProvider{Inputs: mockInputs}
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()
	sj := jobs.NewSurrogateJob(cfgStore, renderer, surrInputProvider, qlWriter, modelLoader, projWriter)
	ilj := jobs.NewInterleavingJob(cfgStore, renderer, executor, qlWriter)
	// No WithShadowStore option — store remains nil.
	h := NewMetricsHandler(stdJob, gj, ccj, sj, ilj, nil, qlWriter)
	mux := http.NewServeMux()
	path, handler := metricsv1connect.NewMetricComputationServiceHandler(h)
	mux.Handle(path, handler)
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return metricsv1connect.NewMetricComputationServiceClient(http.DefaultClient, srv.URL)
}

// candidateMetric returns a minimal MetricDefinition for use in tests.
func candidateMetric() *commonv1.MetricDefinition {
	return &commonv1.MetricDefinition{
		MetricId: "candidate_watch_time",
		Name:     "Candidate Watch Time",
		Type:     commonv1.MetricType_METRIC_TYPE_MEAN,
	}
}

// ---- ScheduleShadowComputation tests ----

func TestScheduleShadow_Success(t *testing.T) {
	client, mockStore := setupShadowTestServer(t)
	resp, err := client.ScheduleShadowComputation(context.Background(), connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.NoError(t, err)
	assert.NotEmpty(t, resp.Msg.GetShadowId())

	// Verify the store has exactly one PENDING run.
	runs := mockStore.AllRuns()
	require.Len(t, runs, 1)
	assert.Equal(t, shadow.StatusPending, runs[0].Status)
	assert.Equal(t, "watch_time_minutes", runs[0].OriginalMetricID)
}

func TestScheduleShadow_EmptyOriginalID(t *testing.T) {
	client, _ := setupShadowTestServer(t)
	_, err := client.ScheduleShadowComputation(context.Background(), connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestScheduleShadow_NilCandidate(t *testing.T) {
	client, _ := setupShadowTestServer(t)
	_, err := client.ScheduleShadowComputation(context.Background(), connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  nil,
		},
	))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestScheduleShadow_NoStoreWired(t *testing.T) {
	client := setupNoStoreTestServer(t)
	_, err := client.ScheduleShadowComputation(context.Background(), connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnavailable, connect.CodeOf(err))
}

// ---- GetShadowResults tests ----

func TestGetShadowResults_NotFound(t *testing.T) {
	client, _ := setupShadowTestServer(t)
	unknownID := uuid.New().String()
	_, err := client.GetShadowResults(context.Background(), connect.NewRequest(
		&metricsv1.GetShadowResultsRequest{ShadowId: unknownID},
	))
	require.Error(t, err)
	assert.Equal(t, connect.CodeNotFound, connect.CodeOf(err))
}

func TestGetShadowResults_PopulatesAggregates(t *testing.T) {
	client, mockStore := setupShadowTestServer(t)
	ctx := context.Background()

	// Schedule a shadow run.
	schedResp, err := client.ScheduleShadowComputation(ctx, connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.NoError(t, err)
	shadowID := uuid.MustParse(schedResp.Msg.GetShadowId())

	// Insert 7 days × 2 tuples, all within tolerance.
	for day := 1; day <= 7; day++ {
		for _, variantID := range []string{"v1", "v2"} {
			require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
				ShadowID:        shadowID,
				ExperimentID:    "exp1",
				VariantID:       variantID,
				ComputationDate: fmt.Sprintf("2026-05-%02d", day),
				OriginalValue:   sql.NullFloat64{Float64: 1.0, Valid: true},
				CandidateValue:  sql.NullFloat64{Float64: 1.01, Valid: true},
				WithinTolerance: true,
			}))
		}
	}

	resp, err := client.GetShadowResults(ctx, connect.NewRequest(
		&metricsv1.GetShadowResultsRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)
	assert.Equal(t, int32(7), resp.Msg.GetDaysWithinTolerance())
	assert.Equal(t, int32(7), resp.Msg.GetTotalDays())
	assert.Equal(t, "PENDING", resp.Msg.GetStatus()) // store not transitioned yet
	assert.Len(t, resp.Msg.GetRows(), 14)            // 7 days × 2 variants
}

// ---- PromoteShadowResult tests ----

func TestPromoteShadow_ApprovesWhen7DaysClean(t *testing.T) {
	client, mockStore := setupShadowTestServer(t)
	ctx := context.Background()

	schedResp, err := client.ScheduleShadowComputation(ctx, connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.NoError(t, err)
	shadowID := uuid.MustParse(schedResp.Msg.GetShadowId())

	for day := 1; day <= 7; day++ {
		require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
			ShadowID:        shadowID,
			ExperimentID:    "exp1",
			VariantID:       "v1",
			ComputationDate: fmt.Sprintf("2026-05-%02d", day),
			OriginalValue:   sql.NullFloat64{Float64: 1.0, Valid: true},
			CandidateValue:  sql.NullFloat64{Float64: 1.0, Valid: true},
			WithinTolerance: true,
		}))
	}

	resp, err := client.PromoteShadowResult(ctx, connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)
	assert.Equal(t, "APPROVED", resp.Msg.GetStatus())
	assert.Empty(t, resp.Msg.GetReason())

	// Verify store recorded the transition.
	runs := mockStore.AllRuns()
	require.Len(t, runs, 1)
	assert.Equal(t, shadow.StatusApproved, runs[0].Status)
}

func TestPromoteShadow_RejectsOnTupleFailure(t *testing.T) {
	client, mockStore := setupShadowTestServer(t)
	ctx := context.Background()

	schedResp, err := client.ScheduleShadowComputation(ctx, connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.NoError(t, err)
	shadowID := uuid.MustParse(schedResp.Msg.GetShadowId())

	// 8 days: day 3 fails.
	for day := 1; day <= 8; day++ {
		within := day != 3
		require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
			ShadowID:        shadowID,
			ExperimentID:    "exp1",
			VariantID:       "v1",
			ComputationDate: fmt.Sprintf("2026-05-%02d", day),
			OriginalValue:   sql.NullFloat64{Float64: 1.0, Valid: true},
			CandidateValue:  sql.NullFloat64{Float64: 1.0, Valid: true},
			WithinTolerance: within,
		}))
	}

	resp, err := client.PromoteShadowResult(ctx, connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)
	assert.Equal(t, "REJECTED", resp.Msg.GetStatus())
	assert.Contains(t, resp.Msg.GetReason(), "2026-05-03")

	// Verify store recorded the transition to REJECTED.
	runs := mockStore.AllRuns()
	require.Len(t, runs, 1)
	assert.Equal(t, shadow.StatusRejected, runs[0].Status)
}

func TestPromoteShadow_PendingWhenInsufficientDays(t *testing.T) {
	client, mockStore := setupShadowTestServer(t)
	ctx := context.Background()

	schedResp, err := client.ScheduleShadowComputation(ctx, connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.NoError(t, err)
	shadowID := uuid.MustParse(schedResp.Msg.GetShadowId())

	// Only 3 days of data.
	for day := 1; day <= 3; day++ {
		require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
			ShadowID:        shadowID,
			ExperimentID:    "exp1",
			VariantID:       "v1",
			ComputationDate: fmt.Sprintf("2026-05-%02d", day),
			OriginalValue:   sql.NullFloat64{Float64: 1.0, Valid: true},
			CandidateValue:  sql.NullFloat64{Float64: 1.0, Valid: true},
			WithinTolerance: true,
		}))
	}

	resp, err := client.PromoteShadowResult(ctx, connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)
	assert.Equal(t, "PENDING", resp.Msg.GetStatus())
	assert.Contains(t, resp.Msg.GetReason(), "4 more days")

	// No transition should have been recorded — run stays PENDING.
	runs := mockStore.AllRuns()
	require.Len(t, runs, 1)
	assert.Equal(t, shadow.StatusPending, runs[0].Status)
}

func TestPromoteShadow_AlreadyPromotedReturnsFailedPrecondition(t *testing.T) {
	client, mockStore := setupShadowTestServer(t)
	ctx := context.Background()

	schedResp, err := client.ScheduleShadowComputation(ctx, connect.NewRequest(
		&metricsv1.ScheduleShadowComputationRequest{
			OriginalMetricId: "watch_time_minutes",
			CandidateMetric:  candidateMetric(),
		},
	))
	require.NoError(t, err)
	shadowID := uuid.MustParse(schedResp.Msg.GetShadowId())

	// Pre-set the status to APPROVED to simulate an already-promoted run.
	mockStore.SetStatus(shadowID, shadow.StatusApproved)

	// Insert 7 passing days so EvaluatePromotion returns APPROVED.
	for day := 1; day <= 7; day++ {
		require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
			ShadowID:        shadowID,
			ExperimentID:    "exp1",
			VariantID:       "v1",
			ComputationDate: fmt.Sprintf("2026-05-%02d", day),
			OriginalValue:   sql.NullFloat64{Float64: 1.0, Valid: true},
			CandidateValue:  sql.NullFloat64{Float64: 1.0, Valid: true},
			WithinTolerance: true,
		}))
	}

	// Call PromoteShadowResult — the CAS should fail since the row is already
	// APPROVED (not PENDING or RUNNING).
	_, err = client.PromoteShadowResult(ctx, connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: shadowID.String()},
	))
	require.Error(t, err)
	assert.Equal(t, connect.CodeFailedPrecondition, connect.CodeOf(err))
}
