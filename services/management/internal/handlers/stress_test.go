//go:build integration

package handlers_test

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
)

// --- Phase 4 Stress Tests ---
// These tests validate that M5 handles high concurrency correctly:
// - No partial state visible to concurrent readers
// - No overlapping bucket allocations under contention
// - Audit trail complete for every mutation

// TestStress_ConcurrentStartExperiment_100 races 100 goroutines trying to
// start the same DRAFT experiment. Exactly one must succeed; all others must
// get FAILED_PRECONDITION. No partial state (e.g., stuck in STARTING) should
// be visible after all goroutines finish.
func TestStress_ConcurrentStartExperiment_100(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-concurrent-start-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("stress-start-100", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	const goroutines = 100
	var successes atomic.Int32
	var preconditionErrors atomic.Int32
	var otherErrors atomic.Int32
	var wg sync.WaitGroup

	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: id,
			}))
			if err == nil {
				successes.Add(1)
			} else if connect.CodeOf(err) == connect.CodeFailedPrecondition {
				preconditionErrors.Add(1)
			} else {
				otherErrors.Add(1)
			}
		}()
	}
	wg.Wait()

	assert.Equal(t, int32(1), successes.Load(),
		"exactly 1 goroutine should succeed starting the experiment")
	assert.Equal(t, int32(goroutines-1), preconditionErrors.Load(),
		"all other goroutines should get FAILED_PRECONDITION")
	assert.Equal(t, int32(0), otherErrors.Load(),
		"no unexpected errors should occur")

	// Verify final state is RUNNING (not stuck in STARTING or DRAFT).
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, got.Msg.State,
		"experiment should be in RUNNING state after concurrent start attempts")
}

// TestStress_RapidCreateStartConcludeCycles creates and drives many experiments
// through the full lifecycle on the same layer (with 10% traffic each), verifying
// that allocations never overlap and all transitions succeed.
func TestStress_RapidCreateStartConcludeCycles(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-rapid-cycle-"+t.Name(), 0)

	const cycles = 20
	for i := 0; i < cycles; i++ {
		// Create experiment.
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("rapid-cycle-%d", i), layer.LayerId),
		}))
		require.NoError(t, err, "create cycle %d", i)

		// Set 10% traffic so multiple can coexist.
		setTrafficPercentage(t, env.pool, exp.Msg.ExperimentId, 0.1)

		// Start.
		started, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
			ExperimentId: exp.Msg.ExperimentId,
		}))
		require.NoError(t, err, "start cycle %d", i)
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, started.Msg.State)

		// Conclude.
		concluded, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
			ExperimentId: exp.Msg.ExperimentId,
		}))
		require.NoError(t, err, "conclude cycle %d", i)
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, concluded.Msg.State)
	}

	// After all cycles, verify no active allocations remain (all released).
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)

	var activeCount int
	for _, a := range allocs.Msg.Allocations {
		if a.ReleasedAt == nil {
			activeCount++
		}
	}
	assert.Equal(t, 0, activeCount,
		"all allocations should be released after conclude")
}

// TestStress_ConcurrentBucketAllocation_10Percent races 10 goroutines starting
// 10% experiments on the same layer (100% capacity total). All should succeed
// with non-overlapping ranges.
func TestStress_ConcurrentBucketAllocation_10Percent(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-alloc-10pct-"+t.Name(), 0)

	const numExperiments = 10
	ids := make([]string, numExperiments)
	for i := 0; i < numExperiments; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("alloc-10pct-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		ids[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, env.pool, ids[i], 0.1)
	}

	// Start all concurrently.
	var successes atomic.Int32
	var wg sync.WaitGroup
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: ids[idx],
			}))
			if err == nil {
				successes.Add(1)
			}
		}(i)
	}
	wg.Wait()

	assert.Equal(t, int32(numExperiments), successes.Load(),
		"all 10 experiments should start (10%% each = 100%% capacity)")

	// Verify no overlapping allocations.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)
	require.Len(t, allocs.Msg.Allocations, numExperiments)

	// Check pairwise non-overlap.
	for i := 0; i < len(allocs.Msg.Allocations); i++ {
		for j := i + 1; j < len(allocs.Msg.Allocations); j++ {
			a := allocs.Msg.Allocations[i]
			b := allocs.Msg.Allocations[j]
			overlaps := a.StartBucket <= b.EndBucket && b.StartBucket <= a.EndBucket
			assert.False(t, overlaps,
				"allocations %d [%d-%d] and %d [%d-%d] overlap",
				i, a.StartBucket, a.EndBucket, j, b.StartBucket, b.EndBucket)
		}
	}
}

// TestStress_AuditTrailCompleteness drives an experiment through the full
// lifecycle (create → start → pause → resume → conclude → archive) and
// verifies that every transition produces an audit trail entry.
func TestStress_AuditTrailCompleteness(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	pool := env.pool
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-audit-"+t.Name(), 0)

	// 1. Create
	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("audit-completeness", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	// 2. Start (DRAFT → STARTING → RUNNING = 2 audit entries)
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	// 3. Pause (RUNNING → RUNNING, audit only)
	_, err = client.PauseExperiment(ctx, connect.NewRequest(&mgmtv1.PauseExperimentRequest{
		ExperimentId: id,
		Reason:       "stress test pause",
	}))
	require.NoError(t, err)

	// 4. Resume (RUNNING → RUNNING, audit only)
	_, err = client.ResumeExperiment(ctx, connect.NewRequest(&mgmtv1.ResumeExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	// 5. Conclude (RUNNING → CONCLUDING → CONCLUDED = 2 audit entries)
	_, err = client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	// 6. Archive (CONCLUDED → ARCHIVED)
	_, err = client.ArchiveExperiment(ctx, connect.NewRequest(&mgmtv1.ArchiveExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	// Query audit trail.
	rows, err := pool.Query(ctx,
		`SELECT action, previous_state, new_state, actor_email
		 FROM audit_trail
		 WHERE experiment_id = $1
		 ORDER BY created_at ASC`, id)
	require.NoError(t, err)
	defer rows.Close()

	type auditRow struct {
		action    string
		prevState *string
		newState  *string
		actor     string
	}
	var entries []auditRow
	for rows.Next() {
		var r auditRow
		err := rows.Scan(&r.action, &r.prevState, &r.newState, &r.actor)
		require.NoError(t, err)
		entries = append(entries, r)
	}
	require.NoError(t, rows.Err())

	// Expected transitions:
	// 1. create: → DRAFT
	// 2. start: DRAFT → STARTING
	// 3. start: STARTING → RUNNING
	// 4. pause: RUNNING → RUNNING
	// 5. resume: RUNNING → RUNNING
	// 6. conclude: RUNNING → CONCLUDING
	// 7. conclude: CONCLUDING → CONCLUDED
	// 8. archive: CONCLUDED → ARCHIVED
	require.Len(t, entries, 8,
		"expected 8 audit trail entries for full lifecycle (create + 2 start + pause + resume + 2 conclude + archive)")

	// Verify each entry.
	expected := []struct {
		action   string
		newState string
	}{
		{"create", "DRAFT"},
		{"start", "STARTING"},
		{"start", "RUNNING"},
		{"pause", "RUNNING"},
		{"resume", "RUNNING"},
		{"conclude", "CONCLUDING"},
		{"conclude", "CONCLUDED"},
		{"archive", "ARCHIVED"},
	}
	for i, exp := range expected {
		assert.Equal(t, exp.action, entries[i].action,
			"entry %d: expected action %q, got %q", i, exp.action, entries[i].action)
		if entries[i].newState != nil {
			assert.Equal(t, exp.newState, *entries[i].newState,
				"entry %d: expected new_state %q, got %q", i, exp.newState, *entries[i].newState)
		}
	}

	// Verify no entries have "system" as actor (RBAC interceptor should inject real identity).
	for i, e := range entries {
		assert.NotEqual(t, "system", e.actor,
			"entry %d (%s): actor should be the authenticated user, not 'system'", i, e.action)
	}
}

// TestStress_BucketReuseIntegrity rapidly creates, starts, and concludes
// experiments to verify bucket allocations maintain integrity through reuse.
// Uses 0-second cooldown so buckets are immediately reusable.
func TestStress_BucketReuseIntegrity(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	// Layer with 0-second cooldown for immediate reuse.
	layer := createTestLayer(t, client, "stress-reuse-"+t.Name(), 0)

	const rounds = 10
	for round := 0; round < rounds; round++ {
		// Each round: create 5 experiments at 20% traffic, start all, conclude all.
		const perRound = 5
		ids := make([]string, perRound)
		for i := 0; i < perRound; i++ {
			exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
				Experiment: newABExperimentInLayer(
					fmt.Sprintf("reuse-r%d-e%d", round, i), layer.LayerId),
			}))
			require.NoError(t, err)
			ids[i] = exp.Msg.ExperimentId
			setTrafficPercentage(t, env.pool, ids[i], 0.2)
		}

		// Start all.
		for i, id := range ids {
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: id,
			}))
			require.NoError(t, err, "round %d, start experiment %d", round, i)
		}

		// Verify no overlapping allocations.
		allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
			LayerId: layer.LayerId,
		}))
		require.NoError(t, err)

		activeAllocs := filterActiveAllocations(allocs.Msg.Allocations)
		require.Len(t, activeAllocs, perRound,
			"round %d: expected %d active allocations", round, perRound)

		for i := 0; i < len(activeAllocs); i++ {
			for j := i + 1; j < len(activeAllocs); j++ {
				a := activeAllocs[i]
				b := activeAllocs[j]
				overlaps := a.StartBucket <= b.EndBucket && b.StartBucket <= a.EndBucket
				assert.False(t, overlaps,
					"round %d: allocations [%d-%d] and [%d-%d] overlap",
					round, a.StartBucket, a.EndBucket, b.StartBucket, b.EndBucket)
			}
		}

		// Conclude all.
		for i, id := range ids {
			_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
				ExperimentId: id,
			}))
			require.NoError(t, err, "round %d, conclude experiment %d", round, i)
		}
	}
}

// filterActiveAllocations returns allocations that haven't been released.
func filterActiveAllocations(allocs []*commonv1.LayerAllocation) []*commonv1.LayerAllocation {
	var active []*commonv1.LayerAllocation
	for _, a := range allocs {
		if a.ReleasedAt == nil {
			active = append(active, a)
		}
	}
	return active
}

// TestStress_ConcurrentLifecycleOperations fires mixed lifecycle operations
// concurrently on different experiments to verify no cross-experiment interference.
func TestStress_ConcurrentLifecycleOperations(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	const numExperiments = 20
	layers := make([]string, numExperiments)
	ids := make([]string, numExperiments)

	// Each experiment gets its own layer to avoid allocation conflicts.
	for i := 0; i < numExperiments; i++ {
		layer := createTestLayer(t, client, fmt.Sprintf("stress-mixed-%d-%s", i, t.Name()), 0)
		layers[i] = layer.LayerId

		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("mixed-%d", i), layers[i]),
		}))
		require.NoError(t, err)
		ids[i] = exp.Msg.ExperimentId
	}

	// Concurrently start all experiments.
	var wg sync.WaitGroup
	var startErrors atomic.Int32
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: ids[idx],
			}))
			if err != nil {
				startErrors.Add(1)
			}
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(0), startErrors.Load(),
		"all experiments should start (each in own layer)")

	// Verify all are RUNNING.
	for i, id := range ids {
		got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
			ExperimentId: id,
		}))
		require.NoError(t, err)
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, got.Msg.State,
			"experiment %d should be RUNNING", i)
	}

	// Concurrently conclude all experiments.
	var concludeErrors atomic.Int32
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
				ExperimentId: ids[idx],
			}))
			if err != nil {
				concludeErrors.Add(1)
			}
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(0), concludeErrors.Load(),
		"all experiments should conclude")

	// Verify all are CONCLUDED.
	for i, id := range ids {
		got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
			ExperimentId: id,
		}))
		require.NoError(t, err)
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, got.Msg.State,
			"experiment %d should be CONCLUDED", i)
	}
}
