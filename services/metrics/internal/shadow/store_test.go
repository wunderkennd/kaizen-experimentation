//go:build integration

package shadow

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func newTestPool(t *testing.T) *pgxpool.Pool {
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

func cleanup(t *testing.T, pool *pgxpool.Pool, shadowID uuid.UUID) {
	t.Helper()
	ctx := context.Background()
	_, _ = pool.Exec(ctx, `DELETE FROM metric_shadow_run_results WHERE shadow_id = $1`, shadowID)
	_, _ = pool.Exec(ctx, `DELETE FROM metric_shadow_runs WHERE shadow_id = $1`, shadowID)
}

// TestPgStore_ScheduleAndGet: round-trip Schedule → Get.
func TestPgStore_ScheduleAndGet(t *testing.T) {
	pool := newTestPool(t)
	store := NewPgStore(pool)
	ctx := context.Background()

	candidate := json.RawMessage(`{"metric_id":"cand_001","name":"candidate"}`)
	shadowID, err := store.Schedule(ctx, "original_metric_001", candidate)
	require.NoError(t, err)
	require.NotEqual(t, uuid.Nil, shadowID)
	t.Cleanup(func() { cleanup(t, pool, shadowID) })

	run, err := store.Get(ctx, shadowID)
	require.NoError(t, err)
	require.NotNil(t, run)
	assert.Equal(t, shadowID, run.ShadowID)
	assert.Equal(t, "original_metric_001", run.OriginalMetricID)
	assert.Equal(t, StatusPending, run.Status)
	assert.JSONEq(t, string(candidate), string(run.CandidateMetric))
	assert.Empty(t, run.RejectionReason)
}

// TestPgStore_GetNotFound: unknown UUID returns nil, nil.
func TestPgStore_GetNotFound(t *testing.T) {
	pool := newTestPool(t)
	store := NewPgStore(pool)
	run, err := store.Get(context.Background(), uuid.New())
	require.NoError(t, err)
	assert.Nil(t, run)
}

// TestPgStore_TransitionCASSuccess: PENDING → RUNNING transitions cleanly.
func TestPgStore_TransitionCASSuccess(t *testing.T) {
	pool := newTestPool(t)
	store := NewPgStore(pool)
	ctx := context.Background()

	shadowID, err := store.Schedule(ctx, "orig_trans_001", json.RawMessage(`{}`))
	require.NoError(t, err)
	t.Cleanup(func() { cleanup(t, pool, shadowID) })

	err = store.Transition(ctx, shadowID, StatusPending, StatusRunning, "")
	require.NoError(t, err)

	run, err := store.Get(ctx, shadowID)
	require.NoError(t, err)
	require.NotNil(t, run)
	assert.Equal(t, StatusRunning, run.Status)
	assert.Empty(t, run.RejectionReason)
}

// TestPgStore_TransitionCASFailure: CAS failure when from-state doesn't match.
func TestPgStore_TransitionCASFailure(t *testing.T) {
	pool := newTestPool(t)
	store := NewPgStore(pool)
	ctx := context.Background()

	shadowID, err := store.Schedule(ctx, "orig_cas_fail", json.RawMessage(`{}`))
	require.NoError(t, err)
	t.Cleanup(func() { cleanup(t, pool, shadowID) })

	// Try to transition from RUNNING → APPROVED when the row is still PENDING.
	err = store.Transition(ctx, shadowID, StatusRunning, StatusApproved, "")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "CAS failure")
}

// TestPgStore_TransitionPersistsRejectionReason: reason stored for REJECTED.
func TestPgStore_TransitionPersistsRejectionReason(t *testing.T) {
	pool := newTestPool(t)
	store := NewPgStore(pool)
	ctx := context.Background()

	shadowID, err := store.Schedule(ctx, "orig_reject_001", json.RawMessage(`{}`))
	require.NoError(t, err)
	t.Cleanup(func() { cleanup(t, pool, shadowID) })

	reason := "2 of 9 days had tuples outside tolerance: 2026-05-12, 2026-05-15"
	err = store.Transition(ctx, shadowID, StatusPending, StatusRejected, reason)
	require.NoError(t, err)

	run, err := store.Get(ctx, shadowID)
	require.NoError(t, err)
	require.NotNil(t, run)
	assert.Equal(t, StatusRejected, run.Status)
	assert.Equal(t, reason, run.RejectionReason)
}

// TestPgStore_InsertResultAndResults: round-trip InsertResult → Results.
func TestPgStore_InsertResultAndResults(t *testing.T) {
	pool := newTestPool(t)
	store := NewPgStore(pool)
	ctx := context.Background()

	shadowID, err := store.Schedule(ctx, "orig_res_001", json.RawMessage(`{}`))
	require.NoError(t, err)
	t.Cleanup(func() { cleanup(t, pool, shadowID) })

	row := ResultRow{
		ShadowID:        shadowID,
		ExperimentID:    "exp_res_001",
		VariantID:       "v1",
		ComputationDate: "2026-05-20",
		OriginalValue:   sql.NullFloat64{Float64: 1.5, Valid: true},
		CandidateValue:  sql.NullFloat64{Float64: 1.52, Valid: true},
		DiffAbs:         sql.NullFloat64{Float64: 0.02, Valid: true},
		DiffRel:         sql.NullFloat64{Float64: 0.0133, Valid: true},
		WithinTolerance: true,
	}
	require.NoError(t, store.InsertResult(ctx, row))

	results, err := store.Results(ctx, shadowID)
	require.NoError(t, err)
	require.Len(t, results, 1)
	got := results[0]
	assert.Equal(t, shadowID, got.ShadowID)
	assert.Equal(t, "exp_res_001", got.ExperimentID)
	assert.Equal(t, "v1", got.VariantID)
	assert.Equal(t, "2026-05-20", got.ComputationDate)
	assert.InDelta(t, 1.5, got.OriginalValue.Float64, 1e-6)
	assert.True(t, got.OriginalValue.Valid)
	assert.InDelta(t, 1.52, got.CandidateValue.Float64, 1e-6)
	assert.True(t, got.CandidateValue.Valid)
	assert.True(t, got.WithinTolerance)
}
