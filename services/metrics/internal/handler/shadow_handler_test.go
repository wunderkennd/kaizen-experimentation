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

// TestGetShadowResults_ExcludesStubRowsFromResponse — the store holds 1 stub row
// (VariantID=="") written by B2 as a dedup marker, plus 2 real result rows
// (VariantID=="treatment" and "control").  GetShadowResults must return only the
// 2 real rows; the stub must be invisible in both Rows and the aggregate counters.
func TestGetShadowResults_ExcludesStubRowsFromResponse(t *testing.T) {
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

	date := "2026-05-01"

	// Stub row: B2 dedup marker.
	require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
		ShadowID:        shadowID,
		ExperimentID:    "exp1",
		VariantID:       "",   // stub marker
		ComputationDate: date,
		OriginalValue:   sql.NullFloat64{Valid: false},
		CandidateValue:  sql.NullFloat64{Valid: false},
		WithinTolerance: false,
	}))

	// Real result rows: B3 differ output.
	for _, variant := range []string{"treatment", "control"} {
		require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
			ShadowID:        shadowID,
			ExperimentID:    "exp1",
			VariantID:       variant,
			ComputationDate: date,
			OriginalValue:   sql.NullFloat64{Float64: 1.0, Valid: true},
			CandidateValue:  sql.NullFloat64{Float64: 1.01, Valid: true},
			WithinTolerance: true,
		}))
	}

	resp, err := client.GetShadowResults(ctx, connect.NewRequest(
		&metricsv1.GetShadowResultsRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)

	// Only the 2 real rows must appear — the stub must be stripped.
	assert.Len(t, resp.Msg.GetRows(), 2,
		"stub row (VariantID=='') must not appear in the operator-facing response")

	// Both real rows belong to "2026-05-01" and pass tolerance.  With only 1 day
	// of real data, EvaluatePromotion returns PENDING (< 7 days).  The aggregate
	// counters must reflect the real rows only: 1 day within tolerance, 1 total.
	assert.Equal(t, int32(1), resp.Msg.GetDaysWithinTolerance(),
		"days_within_tolerance must count only real rows")
	assert.Equal(t, int32(1), resp.Msg.GetTotalDays(),
		"total_days must count only real rows")

	// Verify no stub variant IDs leaked into the response rows.
	for _, row := range resp.Msg.GetRows() {
		assert.NotEmpty(t, row.GetVariantId(),
			"no stub row (empty variant_id) should appear in the response")
	}
}

// C1: TestGetShadowResults_PreservesNullDoubles — a NULL sql.NullFloat64 must
// appear as nil in the proto response, while a valid 0.0 must appear as 0.0.
// Operators need to distinguish "computation failed" (NULL) from "metric is zero".
func TestGetShadowResults_PreservesNullDoubles(t *testing.T) {
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

	// Insert one row: original_value = NULL, candidate_value = 0.0 (valid zero).
	require.NoError(t, mockStore.InsertResult(ctx, shadow.ResultRow{
		ShadowID:        shadowID,
		ExperimentID:    "exp1",
		VariantID:       "v1",
		ComputationDate: "2026-05-01",
		OriginalValue:   sql.NullFloat64{Valid: false},          // genuine NULL
		CandidateValue:  sql.NullFloat64{Float64: 0.0, Valid: true}, // legitimate zero
		WithinTolerance: false,
	}))

	resp, err := client.GetShadowResults(ctx, connect.NewRequest(
		&metricsv1.GetShadowResultsRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)
	require.Len(t, resp.Msg.GetRows(), 1)
	row := resp.Msg.GetRows()[0]
	assert.Nil(t, row.GetOriginalValue(),
		"NULL sql.NullFloat64 must serialize to nil DoubleValue, not 0.0")
	require.NotNil(t, row.GetCandidateValue(),
		"valid 0.0 sql.NullFloat64 must serialize to non-nil DoubleValue")
	assert.InDelta(t, 0.0, row.GetCandidateValue().GetValue(), 1e-9,
		"candidate_value should be 0.0")
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
	// result_id is the approval token consumed by Phase C MigrateMetricDefinition.
	assert.Equal(t, shadowID.String(), resp.Msg.GetResultId())

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

// I5: TestPromoteShadow_IdempotentOnAlreadyApproved — calling PromoteShadowResult
// on an already-APPROVED run returns APPROVED + result_id without error (idempotent
// re-promote; no FailedPrecondition).
func TestPromoteShadow_IdempotentOnAlreadyApproved(t *testing.T) {
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

	// Pre-set status to APPROVED — no results needed; early-return triggers before
	// EvaluatePromotion.
	mockStore.SetStatus(shadowID, shadow.StatusApproved)

	resp, err := client.PromoteShadowResult(ctx, connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)
	assert.Equal(t, "APPROVED", resp.Msg.GetStatus())
	assert.Equal(t, shadowID.String(), resp.Msg.GetResultId())
	assert.Empty(t, resp.Msg.GetReason())
}

// I5: TestPromoteShadow_ReturnsRejectionReasonForAlreadyRejected — calling
// PromoteShadowResult on an already-REJECTED run returns REJECTED + original
// reason without error (idempotent; no FailedPrecondition).
func TestPromoteShadow_ReturnsRejectionReasonForAlreadyRejected(t *testing.T) {
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

	mockStore.SetStatus(shadowID, shadow.StatusRejected)
	mockStore.SetRejectionReason(shadowID, "old reason")

	resp, err := client.PromoteShadowResult(ctx, connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: shadowID.String()},
	))
	require.NoError(t, err)
	assert.Equal(t, "REJECTED", resp.Msg.GetStatus())
	assert.Equal(t, "old reason", resp.Msg.GetReason())
}

// I7: TestPromoteShadow_TransientStoreErrorReturnsInternal — a non-CAS store
// error during Transition must return CodeInternal (NOT CodeFailedPrecondition).
func TestPromoteShadow_TransientStoreErrorReturnsInternal(t *testing.T) {
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

	// Insert 7 contiguous passing days so EvaluatePromotion returns APPROVED.
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

	// Inject a transient non-CAS error into Transition.
	mockStore.SetTransitionErr(fmt.Errorf("connection refused"))

	_, err = client.PromoteShadowResult(ctx, connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: shadowID.String()},
	))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInternal, connect.CodeOf(err),
		"transient store errors must map to CodeInternal, not CodeFailedPrecondition")
}

func TestPromoteShadow_NotFound(t *testing.T) {
	client, _ := setupShadowTestServer(t)
	// A syntactically valid UUID that was never scheduled must return CodeNotFound,
	// not a spurious PENDING (which the old code returned by calling Results on a
	// non-existent shadow_id and getting an empty slice).
	unknownID := uuid.New().String()
	_, err := client.PromoteShadowResult(context.Background(), connect.NewRequest(
		&metricsv1.PromoteShadowResultRequest{ShadowId: unknownID},
	))
	require.Error(t, err)
	assert.Equal(t, connect.CodeNotFound, connect.CodeOf(err))
}
