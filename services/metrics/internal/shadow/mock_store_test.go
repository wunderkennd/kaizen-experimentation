package shadow

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/google/uuid"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// TestMockStore_ListNeedingComputation_Empty: no runs → empty slice.
func TestMockStore_ListNeedingComputation_Empty(t *testing.T) {
	m := NewMockStore()
	runs, err := m.ListNeedingComputation(context.Background(), "exp_a", "2026-06-01")
	require.NoError(t, err)
	assert.Empty(t, runs)
}

// TestMockStore_ListNeedingComputation_PendingNoResults: one PENDING run with no
// result rows for the (experimentID, date) pair → returned.
func TestMockStore_ListNeedingComputation_PendingNoResults(t *testing.T) {
	m := NewMockStore()
	id, err := m.Schedule(context.Background(), "orig_metric_1", json.RawMessage(`{}`))
	require.NoError(t, err)

	runs, err := m.ListNeedingComputation(context.Background(), "exp_a", "2026-06-01")
	require.NoError(t, err)
	require.Len(t, runs, 1)
	assert.Equal(t, id, runs[0].ShadowID)
	assert.Equal(t, StatusPending, runs[0].Status)
}

// TestMockStore_ListNeedingComputation_ExcludesAlreadyDone: a PENDING run that
// already has a result row for the (experimentID, computationDate) pair is NOT
// returned — this is the dedup gate for the B2 stub row.
func TestMockStore_ListNeedingComputation_ExcludesAlreadyDone(t *testing.T) {
	m := NewMockStore()
	id, err := m.Schedule(context.Background(), "orig_metric_1", json.RawMessage(`{}`))
	require.NoError(t, err)

	// Insert a stub result row for this shadow on the target (exp, date) pair.
	err = m.InsertResult(context.Background(), ResultRow{
		ShadowID:        id,
		ExperimentID:    "exp1",
		VariantID:       "",
		ComputationDate: "2026-06-01",
		WithinTolerance: false, // stub marker
	})
	require.NoError(t, err)

	// Same exp+date → excluded.
	runs, err := m.ListNeedingComputation(context.Background(), "exp1", "2026-06-01")
	require.NoError(t, err)
	assert.Empty(t, runs, "run with existing stub result for (exp, date) must be excluded")

	// Different experiment on same date → NOT excluded.
	runs, err = m.ListNeedingComputation(context.Background(), "exp2", "2026-06-01")
	require.NoError(t, err)
	require.Len(t, runs, 1, "run without stub for exp2 must still be returned")
	assert.Equal(t, id, runs[0].ShadowID)
}

// TestMockStore_ListNeedingComputation_DifferentDateNotExcluded: a result row
// exists for a DIFFERENT date — the shadow must still be returned.
func TestMockStore_ListNeedingComputation_DifferentDateNotExcluded(t *testing.T) {
	m := NewMockStore()
	id, err := m.Schedule(context.Background(), "orig_metric_1", json.RawMessage(`{}`))
	require.NoError(t, err)

	// Stub result exists for 2026-06-01, but we're querying for 2026-06-02.
	err = m.InsertResult(context.Background(), ResultRow{
		ShadowID:        id,
		ExperimentID:    "exp1",
		ComputationDate: "2026-06-01",
	})
	require.NoError(t, err)

	runs, err := m.ListNeedingComputation(context.Background(), "exp1", "2026-06-02")
	require.NoError(t, err)
	require.Len(t, runs, 1, "run with result on different date must still be returned")
	assert.Equal(t, id, runs[0].ShadowID)
}

// TestMockStore_ListNeedingComputation_ExcludesNonPending: RUNNING / FAILED /
// APPROVED / REJECTED runs are not returned regardless of result rows.
func TestMockStore_ListNeedingComputation_ExcludesNonPending(t *testing.T) {
	m := NewMockStore()
	ctx := context.Background()

	idRunning, err := m.Schedule(ctx, "orig_running", json.RawMessage(`{}`))
	require.NoError(t, err)
	m.SetStatus(idRunning, StatusRunning)

	idFailed, err := m.Schedule(ctx, "orig_failed", json.RawMessage(`{}`))
	require.NoError(t, err)
	m.SetStatus(idFailed, StatusFailed)

	idApproved, err := m.Schedule(ctx, "orig_approved", json.RawMessage(`{}`))
	require.NoError(t, err)
	m.SetStatus(idApproved, StatusApproved)

	idRejected, err := m.Schedule(ctx, "orig_rejected", json.RawMessage(`{}`))
	require.NoError(t, err)
	m.SetStatus(idRejected, StatusRejected)

	runs, err := m.ListNeedingComputation(ctx, "exp_a", "2026-06-01")
	require.NoError(t, err)
	assert.Empty(t, runs, "only PENDING runs should be returned")
}

// TestMockStore_ListNeedingComputation_MultiplePendingPartiallyExcluded:
// two PENDING runs, one already has a result for the date.
func TestMockStore_ListNeedingComputation_MultiplePendingPartiallyExcluded(t *testing.T) {
	m := NewMockStore()
	ctx := context.Background()

	id1, err := m.Schedule(ctx, "orig1", json.RawMessage(`{}`))
	require.NoError(t, err)
	id2, err := m.Schedule(ctx, "orig2", json.RawMessage(`{}`))
	require.NoError(t, err)

	// Only id1 has been computed already for 2026-06-01.
	require.NoError(t, m.InsertResult(ctx, ResultRow{
		ShadowID:        id1,
		ExperimentID:    "exp1",
		VariantID:       "v1",
		ComputationDate: "2026-06-01",
	}))

	runs, err := m.ListNeedingComputation(ctx, "exp1", "2026-06-01")
	require.NoError(t, err)
	require.Len(t, runs, 1)
	assert.Equal(t, id2, runs[0].ShadowID, "only id2 must be returned; id1 is already done")

	// Verify id2 is returned with a copy (not a pointer alias).
	runs[0].Status = StatusFailed // mutate the copy
	check, _ := m.Get(ctx, id2)
	assert.Equal(t, StatusPending, check.Status, "mutation of returned copy must not affect store state")
}

// TestMockStore_Transition_CASSuccess: PENDING → RUNNING succeeds.
func TestMockStore_Transition_CASSuccess(t *testing.T) {
	m := NewMockStore()
	ctx := context.Background()

	id, err := m.Schedule(ctx, "orig1", json.RawMessage(`{}`))
	require.NoError(t, err)

	require.NoError(t, m.Transition(ctx, id, StatusPending, StatusRunning, ""))

	run, err := m.Get(ctx, id)
	require.NoError(t, err)
	assert.Equal(t, StatusRunning, run.Status)
}

// TestMockStore_Transition_CASFailure: wrong from-state returns ErrCASFailure.
func TestMockStore_Transition_CASFailure(t *testing.T) {
	m := NewMockStore()
	ctx := context.Background()

	id, err := m.Schedule(ctx, "orig1", json.RawMessage(`{}`))
	require.NoError(t, err)

	err = m.Transition(ctx, id, StatusRunning, StatusPending, "")
	require.Error(t, err)
	assert.True(t, IsCASFailure(err), "must wrap ErrCASFailure")
}

// TestMockStore_Transition_UnknownID returns ErrCASFailure.
func TestMockStore_Transition_UnknownID(t *testing.T) {
	m := NewMockStore()
	err := m.Transition(context.Background(), uuid.New(), StatusPending, StatusRunning, "")
	require.Error(t, err)
	assert.True(t, IsCASFailure(err))
}
