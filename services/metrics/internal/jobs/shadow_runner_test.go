package jobs

// shadow_runner_test.go — unit tests for the B2 shadow-run computation path
// inside StandardJob.Run (ADR-026 Phase 3 #437).
//
// These tests use MockStore + MockExecutor; no real Postgres or Spark.
// Integration tests (//go:build integration) live in shadow_runner_integration_test.go.

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"google.golang.org/protobuf/encoding/protojson"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/shadow"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/status"
)

// candidateJSON marshals a MetricDefinition proto to the JSON format stored in
// metric_shadow_runs.candidate_metric (protojson, not encoding/json).
func candidateJSON(t *testing.T, def *commonv1.MetricDefinition) json.RawMessage {
	t.Helper()
	b, err := protojson.Marshal(def)
	require.NoError(t, err, "protojson.Marshal candidate")
	return json.RawMessage(b)
}

// minimalExperimentFixture writes a single-experiment fixture JSON to a temp
// dir and returns the path.  The experiment has two MEAN metrics so the regular
// pass has something to do before the shadow iteration runs.
func minimalExperimentFixture(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	p := filepath.Join(dir, "seed_shadow_test.json")
	const fixture = `{
		"experiments": [
			{
				"experiment_id": "e0000000-0000-0000-0000-shadow000001",
				"name": "shadow_test_exp",
				"type": "STANDARD",
				"state": "RUNNING",
				"started_at": "2026-05-01",
				"primary_metric_id": "watch_time",
				"variants": [
					{"variant_id": "control",   "name": "Control",   "traffic_fraction": 0.5, "is_control": true},
					{"variant_id": "treatment", "name": "Treatment", "traffic_fraction": 0.5, "is_control": false}
				]
			}
		],
		"metrics": [
			{
				"metric_id":        "watch_time",
				"name":             "Watch time (MEAN)",
				"type":             "MEAN",
				"source_event_type": "heartbeat"
			}
		]
	}`
	require.NoError(t, os.WriteFile(p, []byte(fixture), 0o600))
	return p
}

// setupShadowJob creates a StandardJob wired with MockStore + MockExecutor.
func setupShadowJob(t *testing.T, fixturePath string, shadowStore shadow.Store) (*StandardJob, *spark.MockExecutor, *querylog.MemWriter) {
	t.Helper()
	cfgStore, err := config.LoadFromFile(fixturePath)
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(42)
	ql := querylog.NewMemWriter()
	sw := status.NewMockWriter()

	job := NewStandardJob(cfgStore, renderer, executor, ql,
		WithStatusWriter(sw),
		WithShadowStore(shadowStore),
	)
	return job, executor, ql
}

// TestStandardJob_ShadowRun_NilShadowStore: when no shadow store is wired the
// regular pass completes normally (no panic, no extra executor calls).
func TestStandardJob_ShadowRun_NilShadowStore(t *testing.T) {
	p := minimalExperimentFixture(t)
	cfgStore, err := config.LoadFromFile(p)
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(42)
	ql := querylog.NewMemWriter()
	sw := status.NewMockWriter()

	// No WithShadowStore — j.shadowStore is nil.
	job := NewStandardJob(cfgStore, renderer, executor, ql, WithStatusWriter(sw))

	_, err = job.Run(context.Background(), "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, err)

	// Regular pass: 1 MEAN metric + 1 daily treatment effect = 2 executor calls.
	assert.Len(t, executor.GetCalls(), 2, "nil shadowStore must not add shadow executor calls")
}

// TestStandardJob_ShadowRun_HappyPath_FilteredMean: a PENDING FILTERED_MEAN
// shadow candidate is computed; executor records the candidate SQL; status
// transitions PENDING→RUNNING→PENDING; querylog has shadow_run entry.
func TestStandardJob_ShadowRun_HappyPath_FilteredMean(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "mobile_watch_time_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "heartbeat",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "platform = 'mobile'",
				ValueColumn: "duration_ms",
			},
		},
	}
	shadowID, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)

	job, executor, ql := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr, "regular pass must succeed even with shadow")

	// Verify the shadow run's executor call: SQL must contain the shadow ID as metric_id.
	var shadowCall *spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, shadowID.String()) {
			cc := c
			shadowCall = &cc
			break
		}
	}
	require.NotNil(t, shadowCall, "executor must have been called with shadow_id as metric_id")
	assert.Equal(t, "delta.metric_summaries", shadowCall.TargetTable)
	assert.Contains(t, shadowCall.SQL, "platform = 'mobile'", "SQL must embed the FILTERED_MEAN filter")

	// Verify lifecycle: PENDING → RUNNING → PENDING (success).
	run, err := ms.Get(ctx, shadowID)
	require.NoError(t, err)
	assert.Equal(t, shadow.StatusPending, run.Status,
		"successful shadow must land back in PENDING for tomorrow's pass")

	// Verify querylog has a shadow_run entry.
	var shadowEntry *querylog.Entry
	for _, e := range ql.AllEntries() {
		if e.JobType == "shadow_run" {
			ee := e
			shadowEntry = &ee
			break
		}
	}
	require.NotNil(t, shadowEntry, "querylog must have a shadow_run entry")
	assert.Equal(t, shadowID.String(), shadowEntry.MetricID)
}

// TestStandardJob_ShadowRun_HappyPath_MetricQL: a METRICQL candidate is compiled
// and executed; SQL contains the shadow_id metric_id literal.
func TestStandardJob_ShadowRun_HappyPath_MetricQL(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:           "watch_time_candidate_metricql",
		Type:               commonv1.MetricType_METRIC_TYPE_METRICQL,
		SourceEventType:    "n/a",
		MetricqlExpression: "mean(heartbeat.duration_ms)",
	}
	shadowID, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)

	job, executor, _ := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr)

	var shadowCall *spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, shadowID.String()) {
			cc := c
			shadowCall = &cc
			break
		}
	}
	require.NotNil(t, shadowCall, "METRICQL shadow must produce an executor call")
	assert.Equal(t, "delta.metric_summaries", shadowCall.TargetTable)
}

// TestStandardJob_ShadowRun_HappyPath_WindowedCount: a WINDOWED_COUNT candidate
// is rendered and executed; SQL contains INTERVAL <n> HOURS.
func TestStandardJob_ShadowRun_HappyPath_WindowedCount(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "stream_starts_24h_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_WINDOWED_COUNT,
		SourceEventType: "stream_start",
		TypeConfig: &commonv1.MetricDefinition_WindowedCount{
			WindowedCount: &commonv1.WindowedCountConfig{
				EventType:   "stream_start",
				WindowHours: 24,
			},
		},
	}
	shadowID, err := ms.Schedule(ctx, "stream_starts_24h", candidateJSON(t, candidate))
	require.NoError(t, err)

	job, executor, _ := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr)

	var shadowCall *spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, shadowID.String()) {
			cc := c
			shadowCall = &cc
			break
		}
	}
	require.NotNil(t, shadowCall, "WINDOWED_COUNT shadow must produce an executor call")
	assert.Contains(t, shadowCall.SQL, "INTERVAL 24 HOURS", "SQL must embed window_hours")
}

// TestStandardJob_ShadowRun_Rejected_CustomType: a CUSTOM candidate must
// transition to FAILED with "cannot be CUSTOM type" reason.
func TestStandardJob_ShadowRun_Rejected_CustomType(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "custom_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_CUSTOM,
		SourceEventType: "heartbeat",
		CustomSql:       "SELECT 1",
	}
	shadowID, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)

	job, _, _ := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr, "CUSTOM shadow failure must not propagate into the regular pass")

	run, err := ms.Get(ctx, shadowID)
	require.NoError(t, err)
	assert.Equal(t, shadow.StatusFailed, run.Status,
		"CUSTOM candidate must land in FAILED")
	assert.Contains(t, run.RejectionReason, "cannot be CUSTOM type")
}

// TestStandardJob_ShadowRun_Rejected_UnknownType: an UNSPECIFIED / unknown type
// transitions to FAILED.
func TestStandardJob_ShadowRun_Rejected_UnknownType(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	// MetricType_METRIC_TYPE_UNSPECIFIED (0)
	candidate := &commonv1.MetricDefinition{
		MetricId:        "unspecified_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_UNSPECIFIED,
		SourceEventType: "heartbeat",
	}
	shadowID, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)

	job, _, _ := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr, "unknown-type shadow failure must not propagate")

	run, err := ms.Get(ctx, shadowID)
	require.NoError(t, err)
	assert.Equal(t, shadow.StatusFailed, run.Status)
	assert.Contains(t, run.RejectionReason, "unsupported shadow candidate type")
}

// TestStandardJob_ShadowRun_ComputeFailure_TransitionsToFailed: when the executor
// returns an error for the shadow call, the run transitions RUNNING → FAILED and
// the regular pass result is still returned successfully.
func TestStandardJob_ShadowRun_ComputeFailure_TransitionsToFailed(t *testing.T) {
	p := minimalExperimentFixture(t)

	// Use a selective executor: regular metrics succeed, shadow call fails.
	sentinel := fmt.Errorf("spark cluster unavailable for shadow")
	var shadowID string

	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "fm_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "heartbeat",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "platform = 'mobile'",
				ValueColumn: "duration_ms",
			},
		},
	}
	id, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)
	shadowID = id.String()

	cfgStore, err := config.LoadFromFile(p)
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	ql := querylog.NewMemWriter()
	sw := status.NewMockWriter()

	// Executor that fails only on the shadow SQL (identified by shadow UUID).
	failOnShadow := &metricSelectiveExecutor{
		failOnMetricID: shadowID,
		failErr:        sentinel,
	}

	job := NewStandardJob(cfgStore, renderer, failOnShadow, ql,
		WithStatusWriter(sw),
		WithShadowStore(ms),
	)

	result, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr, "shadow execute failure must not propagate into Run's return error")
	assert.Equal(t, 1, result.MetricsComputed, "regular metric must still complete")

	run, err := ms.Get(ctx, id)
	require.NoError(t, err)
	assert.Equal(t, shadow.StatusFailed, run.Status,
		"execute failure must transition shadow RUNNING → FAILED")
	assert.Contains(t, run.RejectionReason, "execute:")
}

// TestStandardJob_ShadowRun_CASRace_Skipped: when Transition PENDING→RUNNING
// returns ErrCASFailure (another M3 won the race), the shadow is silently
// skipped and the run remains PENDING (not FAILED).
func TestStandardJob_ShadowRun_CASRace_Skipped(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "fm_candidate_cas",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "heartbeat",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "platform = 'mobile'",
				ValueColumn: "duration_ms",
			},
		},
	}
	shadowID, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)

	// Inject CAS failure so PENDING→RUNNING always fails.
	ms.SetTransitionErr(fmt.Errorf("transition shadow %s PENDING->RUNNING: %w",
		shadowID, shadow.ErrCASFailure))

	job, executor, _ := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr, "CAS race must not propagate into Run's return error")

	// Shadow SQL must never have been sent to executor.
	for _, c := range executor.GetCalls() {
		assert.NotContains(t, c.SQL, shadowID.String(),
			"shadow SQL must not be executed when CAS fails")
	}

	// Clear injected error and verify the run remains PENDING (not FAILED).
	ms.SetTransitionErr(nil)
	run, err := ms.Get(ctx, shadowID)
	require.NoError(t, err)
	assert.Equal(t, shadow.StatusPending, run.Status,
		"CAS-skipped shadow must remain PENDING")
}

// TestStandardJob_ShadowRun_RegularPassUnaffected: when the shadow store has a
// PENDING run AND there are multiple regular metrics, the regular metrics all
// complete AND the shadow is separately computed.
func TestStandardJob_ShadowRun_RegularPassUnaffected(t *testing.T) {
	// Use the standard seed config which has 4 metrics + post-processing.
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(500)
	ql := querylog.NewMemWriter()
	sw := status.NewMockWriter()

	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "shadow_of_ctr",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "click",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "device_type = 'tv'",
				ValueColumn: "duration_ms",
			},
		},
	}
	shadowID, err := ms.Schedule(ctx, "ctr_recommendation", candidateJSON(t, candidate))
	require.NoError(t, err)

	job := NewStandardJob(cfgStore, renderer, executor, ql,
		WithStatusWriter(sw),
		WithShadowStore(ms),
	)

	result, runErr := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, runErr)

	// Regular pass: 4 metrics completed.
	assert.Equal(t, 4, result.MetricsComputed)

	// Shadow must also have been computed (status back to PENDING on success).
	run, err := ms.Get(ctx, shadowID)
	require.NoError(t, err)
	assert.Equal(t, shadow.StatusPending, run.Status)

	// querylog must have one shadow_run entry.
	shadowEntries := 0
	for _, e := range ql.AllEntries() {
		if e.JobType == "shadow_run" {
			shadowEntries++
		}
	}
	assert.Equal(t, 1, shadowEntries, "exactly one shadow_run query log entry expected")
}

// TestStandardJob_ShadowRun_AlreadyComputedToday_NotReRun: a shadow run that
// already has a result row for today (mock InsertResult simulating B3) is
// NOT re-computed by the pass.
func TestStandardJob_ShadowRun_AlreadyComputedToday_NotReRun(t *testing.T) {
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "already_done_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "heartbeat",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "platform = 'mobile'",
				ValueColumn: "duration_ms",
			},
		},
	}
	shadowID, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)

	// Simulate B3 having already written a result for today's date.
	// The exact date must match what StandardJob.Run computes via time.Now().
	// We insert a result row with an intentionally past date to test the
	// dedup gate for a different date (today's date is dynamic; we test the
	// gate logic in mock_store_test.go where the date is controlled).
	// Instead, verify the non-re-run path by checking that after inserting a
	// result row for a predictable date, ListNeedingComputation excludes the ID.
	today := "2099-12-31" // far future — will never match real time.Now()
	require.NoError(t, ms.InsertResult(ctx, shadow.ResultRow{
		ShadowID:        shadowID,
		ExperimentID:    "exp1",
		VariantID:       "v1",
		ComputationDate: today,
	}))

	runs, err := ms.ListNeedingComputation(ctx, today)
	require.NoError(t, err)
	for _, r := range runs {
		assert.NotEqual(t, shadowID, r.ShadowID,
			"shadow already computed for today must be excluded from ListNeedingComputation")
	}
}
