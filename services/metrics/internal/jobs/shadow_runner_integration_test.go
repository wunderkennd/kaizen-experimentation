//go:build integration

package jobs

// shadow_runner_integration_test.go — integration tests for the B2 shadow-run
// computation path in StandardJob.Run (ADR-026 Phase 3 #437).
//
// These tests require a running Postgres instance (see TEST_DATABASE_URL).
// They exercise the PgStore + MockExecutor path rather than a real Spark cluster
// because:
//   - The shadow runner's correctness is in the Pg transitions, not SQL content
//   - delta.metric_summaries assertions check that the executor received the SQL
//     with the shadow_id as metric_id — a real cluster is not required for that
//
// Run with:
//   TEST_DATABASE_URL="postgres://..." go test -tags integration ./metrics/internal/jobs/...

import (
	"context"
	"encoding/json"
	"os"
	"strings"
	"testing"

	"google.golang.org/protobuf/encoding/protojson"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/shadow"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/status"
)

func newIntegTestPool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		dsn = "postgres://experimentation:localdev@localhost:5432/experimentation?sslmode=disable"
	}
	pool, err := pgxpool.New(context.Background(), dsn)
	require.NoError(t, err)
	t.Cleanup(pool.Close)
	return pool
}

func cleanupShadow(t *testing.T, pool *pgxpool.Pool, ids ...interface{}) {
	t.Helper()
	ctx := context.Background()
	for _, id := range ids {
		_, _ = pool.Exec(ctx, `DELETE FROM metric_shadow_run_results WHERE shadow_id = $1`, id)
		_, _ = pool.Exec(ctx, `DELETE FROM metric_shadow_runs WHERE shadow_id = $1`, id)
	}
}

// TestStandardJob_ShadowRun_Integration_HappyPath: insert a PENDING shadow row
// via PgStore, run StandardJob, assert:
//  1. Row transitions back to PENDING (RUNNING → PENDING success path).
//  2. MockExecutor received SQL containing shadow_id as metric_id.
//  3. querylog has a shadow_run entry for the shadow UUID.
func TestStandardJob_ShadowRun_Integration_HappyPath(t *testing.T) {
	pool := newIntegTestPool(t)
	store := shadow.NewPgStore(pool)
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "integ_filtered_mean_candidate",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "heartbeat",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "platform = 'mobile'",
				ValueColumn: "duration_ms",
			},
		},
	}
	candidateBytes, err := protojson.Marshal(candidate)
	require.NoError(t, err)

	shadowID, err := store.Schedule(ctx, "watch_time", json.RawMessage(candidateBytes))
	require.NoError(t, err)
	t.Cleanup(func() { cleanupShadow(t, pool, shadowID) })

	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	executor := spark.NewMockExecutor(42)
	ql := querylog.NewMemWriter()
	sw := status.NewMockWriter()

	job := NewStandardJob(cfgStore, renderer, executor, ql,
		WithStatusWriter(sw),
		WithShadowStore(store),
	)

	_, runErr := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, runErr)

	// Assert transition: PENDING → (RUNNING) → PENDING (success).
	run, err := store.Get(ctx, shadowID)
	require.NoError(t, err)
	require.NotNil(t, run)
	assert.Equal(t, shadow.StatusPending, run.Status,
		"successful shadow must land back in PENDING for tomorrow's pass")

	// Assert executor received SQL with shadow_id as metric_id.
	shadowIDStr := shadowID.String()
	var shadowCall *spark.MockCall
	for _, c := range executor.GetCalls() {
		if strings.Contains(c.SQL, shadowIDStr) {
			cc := c
			shadowCall = &cc
			break
		}
	}
	require.NotNil(t, shadowCall, "executor must have been called with shadow_id as metric_id")
	assert.Equal(t, "delta.metric_summaries", shadowCall.TargetTable,
		"shadow SQL must target delta.metric_summaries")

	// Assert querylog has a shadow_run entry.
	var shadowEntry *querylog.Entry
	for _, e := range ql.AllEntries() {
		if e.JobType == "shadow_run" {
			ee := e
			shadowEntry = &ee
			break
		}
	}
	require.NotNil(t, shadowEntry, "querylog must have a shadow_run entry")
	assert.Equal(t, shadowIDStr, shadowEntry.MetricID)
}

// TestStandardJob_ShadowRun_Integration_AlreadyComputedExcluded: after a result
// row is inserted for today's date (simulating B3), ListNeedingComputation must
// exclude the shadow and the pass must not re-compute it.
func TestStandardJob_ShadowRun_Integration_AlreadyComputedExcluded(t *testing.T) {
	pool := newIntegTestPool(t)
	store := shadow.NewPgStore(pool)
	ctx := context.Background()

	candidate := &commonv1.MetricDefinition{
		MetricId:        "integ_filtered_mean_already_done",
		Type:            commonv1.MetricType_METRIC_TYPE_FILTERED_MEAN,
		SourceEventType: "heartbeat",
		TypeConfig: &commonv1.MetricDefinition_FilteredMean{
			FilteredMean: &commonv1.FilteredMeanConfig{
				FilterSql:   "platform = 'tv'",
				ValueColumn: "duration_ms",
			},
		},
	}
	candidateBytes, err := protojson.Marshal(candidate)
	require.NoError(t, err)

	shadowID, err := store.Schedule(ctx, "watch_time", json.RawMessage(candidateBytes))
	require.NoError(t, err)
	t.Cleanup(func() { cleanupShadow(t, pool, shadowID) })

	// Pre-insert a result for today — simulates B3 having already run.
	// We use a far-future date to avoid accidentally matching real time.Now().
	const alreadyDoneDate = "2099-01-01"
	require.NoError(t, store.InsertResult(ctx, shadow.ResultRow{
		ShadowID:        shadowID,
		ExperimentID:    "integ_exp",
		VariantID:       "v1",
		ComputationDate: alreadyDoneDate,
		WithinTolerance: true,
	}))

	// Confirm ListNeedingComputation excludes the shadow for alreadyDoneDate.
	runs, err := store.ListNeedingComputation(ctx, alreadyDoneDate)
	require.NoError(t, err)
	for _, r := range runs {
		assert.NotEqual(t, shadowID, r.ShadowID,
			"shadow with existing result for date must be excluded from ListNeedingComputation")
	}
}
