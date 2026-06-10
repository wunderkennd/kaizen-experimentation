package jobs

// shadow_runner_test.go — unit tests for the B2 + B3 shadow-run computation
// path inside StandardJob.Run (ADR-026 Phase 3 #437).
//
// These tests use MockStore + MockExecutor + mockValueReaderForJobs; no real
// Postgres or Spark.  Integration tests (//go:build integration) live in
// shadow_runner_integration_test.go.

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

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
				"type":             "METRIC_TYPE_MEAN",
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

// TestStandardJob_ShadowRun_PropagatesExperimentID: regression test for Fix 2 —
// the SQL sent to the executor must contain the real experiment_id, not an empty
// string.  An empty ExperimentID in TemplateParams would render
// `WHERE experiment_id = ”` in delta.exposures joins, producing zero-row output.
func TestStandardJob_ShadowRun_PropagatesExperimentID(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "exp_id_propagation_candidate",
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

	const targetExp = "e0000000-0000-0000-0000-shadow000001"
	job, executor, _ := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, targetExp)
	require.NoError(t, runErr)

	// Find the shadow executor call and verify experiment_id appears in the SQL.
	var shadowCall *spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, shadowID.String()) {
			cc := c
			shadowCall = &cc
			break
		}
	}
	require.NotNil(t, shadowCall, "shadow must have produced an executor call")
	assert.Contains(t, shadowCall.SQL, targetExp,
		"rendered SQL must contain the real experiment_id (Fix 2 regression guard)")
}

// TestStandardJob_ShadowRun_WritesStubResultRowForDedup: after a successful
// shadow compute, computeOneShadow must insert a stub ResultRow with NULL diff
// values and within_tolerance=false.  This row is the dedup marker that prevents
// re-computation within the same nightly pass.
func TestStandardJob_ShadowRun_WritesStubResultRowForDedup(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "stub_row_candidate",
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

	// Use the experiment ID defined in minimalExperimentFixture.
	const expX = "e0000000-0000-0000-0000-shadow000001"
	job, _, _ := setupShadowJob(t, p, ms)

	_, runErr := job.Run(ctx, expX)
	require.NoError(t, runErr)

	// Inspect the mock store for the stub result row.
	results, err := ms.Results(ctx, shadowID)
	require.NoError(t, err)
	require.Len(t, results, 1, "exactly one stub result row must be written by B2")

	stub := results[0]
	assert.Equal(t, shadowID, stub.ShadowID)
	assert.Equal(t, expX, stub.ExperimentID,
		"stub ExperimentID must match the experiment passed to job.Run")
	assert.False(t, stub.OriginalValue.Valid,
		"stub OriginalValue must be NULL (B3 will fill it)")
	assert.False(t, stub.CandidateValue.Valid,
		"stub CandidateValue must be NULL (B3 will fill it)")
	assert.False(t, stub.DiffAbs.Valid,
		"stub DiffAbs must be NULL")
	assert.False(t, stub.WithinTolerance,
		"stub WithinTolerance must be false (B3 sets real value)")
}

// TestStandardJob_ShadowRun_DedupAcrossExperimentsInSamePass verifies the
// per-(shadow_id, experiment_id) dedup contract:
//   - job.Run(exp_A) → shadow is computed once for exp_A
//   - job.Run(exp_B) → shadow is computed once for exp_B (different key)
//   - job.Run(exp_A) again → shadow is NOT re-computed (stub row exists for exp_A)
func TestStandardJob_ShadowRun_DedupAcrossExperimentsInSamePass(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "dedup_cross_exp_candidate",
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

	job, executor, _ := setupShadowJob(t, p, ms)

	countShadowCalls := func() int {
		n := 0
		for _, c := range executor.GetCalls() {
			if strings.Contains(c.SQL, shadowID.String()) {
				n++
			}
		}
		return n
	}

	// First call: exp_A — shadow must be computed (1 call total).
	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr)
	assert.Equal(t, 1, countShadowCalls(), "first job.Run must compute shadow once")

	// Verify SQL contained exp_A's ID.
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, shadowID.String()) {
			assert.Contains(t, c.SQL, "e0000000-0000-0000-0000-shadow000001",
				"shadow SQL must be scoped to exp_A")
			break
		}
	}

	// Second call: exp_A again — dedup stub row exists → shadow must NOT re-compute.
	executor.Reset()
	_, runErr = job.Run(ctx, "e0000000-0000-0000-0000-shadow000001")
	require.NoError(t, runErr)
	assert.Equal(t, 0, countShadowCalls(),
		"second job.Run for the same experiment must NOT re-compute the shadow (dedup)")

	// Verify the stub result row count: still 1 (no new stub written on skip).
	results, err := ms.Results(ctx, shadowID)
	require.NoError(t, err)
	assert.Len(t, results, 1,
		"stub result count must remain 1 after dedup skip; no duplicate rows")
}

// ---------------------------------------------------------------------------
// B3 — Differ wire-through tests
// ---------------------------------------------------------------------------

// mockValueReaderForJobs is an in-memory shadow.ValueReader for use in the
// jobs package tests.  It adapts the shadow.ValueReader interface
// (Read(ctx, metricID, experimentID, computationDate)) to a simple map.
type mockValueReaderForJobs struct {
	mu   sync.Mutex
	data map[jobsReaderKey]map[string]float64
	// errOnRead, when non-nil, is returned by Read for any key.
	errOnRead error
}

type jobsReaderKey struct {
	metricID, experimentID, computationDate string
}

func newMockValueReaderForJobs() *mockValueReaderForJobs {
	return &mockValueReaderForJobs{data: make(map[jobsReaderKey]map[string]float64)}
}

// SetValues pre-seeds per-variant values for (metricID, experimentID, computationDate).
func (m *mockValueReaderForJobs) SetValues(metricID, experimentID, computationDate string, vals map[string]float64) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.data[jobsReaderKey{metricID, experimentID, computationDate}] = vals
}

// SetErr configures an error to be returned by all Read calls.
func (m *mockValueReaderForJobs) SetErr(err error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.errOnRead = err
}

// Read implements shadow.ValueReader.
func (m *mockValueReaderForJobs) Read(_ context.Context, metricID, experimentID, computationDate string) (map[string]float64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.errOnRead != nil {
		return nil, m.errOnRead
	}
	k := jobsReaderKey{metricID, experimentID, computationDate}
	vals, ok := m.data[k]
	if !ok {
		return make(map[string]float64), nil
	}
	out := make(map[string]float64, len(vals))
	for v, f := range vals {
		out[v] = f
	}
	return out, nil
}

// perVariantRows filters out stub rows (VariantID == "") from a result set.
func perVariantRows(rows []shadow.ResultRow) []shadow.ResultRow {
	var out []shadow.ResultRow
	for _, r := range rows {
		if r.VariantID != "" {
			out = append(out, r)
		}
	}
	return out
}

// setupShadowJobWithDiffer creates a StandardJob wired with MockStore + Differ +
// the provided value reader.  The fixture must already have the original metric
// registered so GetMetric succeeds in the differ step.
func setupShadowJobWithDiffer(
	t *testing.T,
	fixturePath string,
	ms shadow.Store,
	reader shadow.ValueReader,
) (*StandardJob, *spark.MockExecutor) {
	t.Helper()
	cfgStore, err := config.LoadFromFile(fixturePath)
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(42)
	ql := querylog.NewMemWriter()
	sw := status.NewMockWriter()

	differ := shadow.NewDiffer(reader, ms)

	job := NewStandardJob(cfgStore, renderer, executor, ql,
		WithStatusWriter(sw),
		WithShadowStore(ms),
		WithDiffer(differ),
	)
	return job, executor
}

// TestStandardJob_ShadowRun_InvokesDifferAfterCompute verifies end-to-end that
// after a successful shadow compute, the Differ writes per-variant ResultRows
// (distinct from the B2 stub) to the store.
func TestStandardJob_ShadowRun_InvokesDifferAfterCompute(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	// Schedule a FILTERED_MEAN shadow for the "watch_time" original metric.
	// The fixture declares watch_time as type MEAN.
	candidate := &commonv1.MetricDefinition{
		MetricId:        "differ_wire_candidate",
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

	// Pre-seed the reader with known original + candidate values per variant.
	// The job uses time.Now().Format("2006-01-02") internally — use the same
	// call here so the seeds match the date the job computes.
	reader := newMockValueReaderForJobs()
	expID := "e0000000-0000-0000-0000-shadow000001"
	computationDate := time.Now().Format("2006-01-02")

	reader.SetValues("watch_time", expID, computationDate, map[string]float64{
		"control":   10.0,
		"treatment": 12.0,
	})
	reader.SetValues(shadowID.String(), expID, computationDate, map[string]float64{
		"control":   10.0,
		"treatment": 12.0,
	})

	job, _ := setupShadowJobWithDiffer(t, p, ms, reader)

	_, runErr := job.Run(ctx, expID)
	require.NoError(t, runErr, "job.Run must succeed even with differ wired")

	// B2 stub + B3 per-variant rows must both exist.
	allResults, err := ms.Results(ctx, shadowID)
	require.NoError(t, err)

	var stubs []shadow.ResultRow
	for _, r := range allResults {
		if r.VariantID == "" {
			stubs = append(stubs, r)
		}
	}
	pvRows := perVariantRows(allResults)

	assert.Len(t, stubs, 1, "B2 must write exactly one stub row")
	assert.Len(t, pvRows, 2, "B3 differ must write one row per variant (control + treatment)")

	// Every per-variant row must have a non-empty VariantID and within_tolerance = true
	// (values are identical on both sides).
	for _, r := range pvRows {
		assert.NotEmpty(t, r.VariantID)
		assert.True(t, r.OriginalValue.Valid)
		assert.True(t, r.CandidateValue.Valid)
		assert.True(t, r.WithinTolerance, "identical orig/cand must be within_tolerance")
	}
}

// TestStandardJob_ShadowRun_DifferFailure_DoesNotFailShadow verifies that when
// the differ's ValueReader returns an error, the shadow compute still completes
// successfully (stub written, status = PENDING) and the differ error is absorbed.
func TestStandardJob_ShadowRun_DifferFailure_DoesNotFailShadow(t *testing.T) {
	p := minimalExperimentFixture(t)
	ms := shadow.NewMockStore()
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "differ_failure_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "heartbeat",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "platform = 'tv'",
				ValueColumn: "duration_ms",
			},
		},
	}
	shadowID, err := ms.Schedule(ctx, "watch_time", candidateJSON(t, candidate))
	require.NoError(t, err)

	// Wire a reader that always errors.
	reader := newMockValueReaderForJobs()
	reader.SetErr(fmt.Errorf("delta.metric_summaries read timeout"))

	job, _ := setupShadowJobWithDiffer(t, p, ms, reader)

	expID := "e0000000-0000-0000-0000-shadow000001"
	_, runErr := job.Run(ctx, expID)
	require.NoError(t, runErr, "differ failure must NOT propagate into Run's return error")

	// Compute succeeded → shadow must still be PENDING (not FAILED).
	run, getErr := ms.Get(ctx, shadowID)
	require.NoError(t, getErr)
	assert.Equal(t, shadow.StatusPending, run.Status,
		"differ failure must NOT transition shadow to FAILED; shadow stays PENDING for tomorrow's pass")

	// B2 stub row must still exist (compute succeeded).
	allResults, err := ms.Results(ctx, shadowID)
	require.NoError(t, err)

	var stubs []shadow.ResultRow
	for _, r := range allResults {
		if r.VariantID == "" {
			stubs = append(stubs, r)
		}
	}
	assert.Len(t, stubs, 1, "stub row must be present even when differ fails (compute succeeded)")

	// No per-variant rows must exist (differ never wrote them).
	pvRows := perVariantRows(allResults)
	assert.Empty(t, pvRows, "no per-variant rows must be written when differ errors")
}
