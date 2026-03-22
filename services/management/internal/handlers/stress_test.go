//go:build integration

package handlers_test

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/types/known/durationpb"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
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

// --- Phase 4: Bucket Reuse Stress Tests ---

// createTestLayerWithBuckets creates a test layer with configurable total buckets
// and cooldown. Useful for tests that need non-default bucket counts.
func createTestLayerWithBuckets(
	t *testing.T,
	client managementv1connect.ExperimentManagementServiceClient,
	name string,
	totalBuckets int32,
	cooldownSeconds int64,
) *commonv1.Layer {
	t.Helper()
	resp, err := client.CreateLayer(context.Background(), connect.NewRequest(&mgmtv1.CreateLayerRequest{
		Layer: &commonv1.Layer{
			Name:                name,
			Description:         "test layer",
			TotalBuckets:        totalBuckets,
			BucketReuseCooldown: &durationpb.Duration{Seconds: cooldownSeconds},
		},
	}))
	require.NoError(t, err)
	return resp.Msg
}

// assertNoOverlap checks that no two active allocations overlap bucket ranges.
func assertNoOverlap(t *testing.T, allocs []*commonv1.LayerAllocation, context string) {
	t.Helper()
	active := filterActiveAllocations(allocs)
	for i := 0; i < len(active); i++ {
		for j := i + 1; j < len(active); j++ {
			a := active[i]
			b := active[j]
			overlaps := a.StartBucket <= b.EndBucket && b.StartBucket <= a.EndBucket
			assert.False(t, overlaps,
				"%s: allocations [%d-%d] (exp %s) and [%d-%d] (exp %s) overlap",
				context, a.StartBucket, a.EndBucket, a.ExperimentId,
				b.StartBucket, b.EndBucket, b.ExperimentId)
		}
	}
}

// TestStress_CooldownBlocksReuse verifies that concluded experiments with a
// cooldown period block bucket reuse until the cooldown expires.
func TestStress_CooldownBlocksReuse(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	pool := env.pool
	ctx := context.Background()

	// Layer with 5-second cooldown.
	layer := createTestLayerWithBuckets(t, client, "stress-cooldown-"+t.Name(), 10000, 5)

	// Create 2 experiments at 50% each → 100% capacity.
	exp1, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("cooldown-a", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, exp1.Msg.ExperimentId, 0.5)

	exp2, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("cooldown-b", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, exp2.Msg.ExperimentId, 0.5)

	// Start both.
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp1.Msg.ExperimentId,
	}))
	require.NoError(t, err)
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp2.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Conclude experiment-1 → released but in cooldown.
	_, err = client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
		ExperimentId: exp1.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Verify SQL: reusable_after > NOW() for experiment-1's allocation.
	var reusableAfter time.Time
	err = pool.QueryRow(ctx,
		`SELECT reusable_after FROM layer_allocations
		 WHERE experiment_id = $1 AND released_at IS NOT NULL`,
		exp1.Msg.ExperimentId).Scan(&reusableAfter)
	require.NoError(t, err, "should find released allocation with reusable_after")
	assert.True(t, reusableAfter.After(time.Now()),
		"reusable_after should be in the future during cooldown")

	// Try to start experiment-3 at 50% → must fail (cooling slots still occupied).
	exp3, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("cooldown-c", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, exp3.Msg.ExperimentId, 0.5)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp3.Msg.ExperimentId,
	}))
	require.Error(t, err, "starting during cooldown should fail")
	assert.Equal(t, connect.CodeResourceExhausted, connect.CodeOf(err),
		"should get ResourceExhausted for insufficient capacity during cooldown")

	// Wait for cooldown to expire.
	time.Sleep(6 * time.Second)

	// Now start experiment-3 → should succeed.
	started, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp3.Msg.ExperimentId,
	}))
	require.NoError(t, err, "starting after cooldown should succeed")
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, started.Msg.State)

	// Verify no overlap between experiment-2 and experiment-3.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)
	assertNoOverlap(t, allocs.Msg.Allocations, "post-cooldown")
}

// TestStress_LayerExhaustionAndRecovery fills a layer to 100%, verifies that
// an 11th experiment fails, then concludes half and starts 5 new ones.
func TestStress_LayerExhaustionAndRecovery(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	pool := env.pool
	ctx := context.Background()

	// Layer with 0-second cooldown, 5000 total buckets.
	layer := createTestLayerWithBuckets(t, client, "stress-exhaust-"+t.Name(), 5000, 0)

	// Create and start 10 experiments at 10% each → 100%.
	const numExperiments = 10
	ids := make([]string, numExperiments)
	for i := 0; i < numExperiments; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("exhaust-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		ids[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, pool, ids[i], 0.1)
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
		"all 10 experiments should start (10%% each = 100%% of 5000 buckets)")

	// 11th experiment at 10% → must fail.
	exp11, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("exhaust-overflow", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, exp11.Msg.ExperimentId, 0.1)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp11.Msg.ExperimentId,
	}))
	require.Error(t, err, "11th experiment should fail — layer at 100%%")
	assert.Equal(t, connect.CodeResourceExhausted, connect.CodeOf(err))

	// Conclude first 5 experiments.
	for i := 0; i < 5; i++ {
		_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
			ExperimentId: ids[i],
		}))
		require.NoError(t, err, "conclude experiment %d", i)
	}

	// Start 5 new experiments at 10% each.
	newIDs := make([]string, 5)
	for i := 0; i < 5; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("exhaust-new-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		newIDs[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, pool, newIDs[i], 0.1)
	}

	for i, id := range newIDs {
		_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
			ExperimentId: id,
		}))
		require.NoError(t, err, "new experiment %d should start after recovery", i)
	}

	// Verify no overlapping allocations across all active experiments.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)
	assertNoOverlap(t, allocs.Msg.Allocations, "post-recovery")

	activeAllocs := filterActiveAllocations(allocs.Msg.Allocations)
	assert.Len(t, activeAllocs, 10,
		"should have 10 active allocations (5 original + 5 new)")
}

// TestStress_FragmentationRecovery creates 8 experiments with varied traffic
// percentages, concludes non-contiguous ones to create fragmentation, then
// verifies a new larger experiment can still be allocated.
func TestStress_FragmentationRecovery(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	pool := env.pool
	ctx := context.Background()

	layer := createTestLayerWithBuckets(t, client, "stress-frag-"+t.Name(), 10000, 0)

	// 8 experiments with varied traffic: 5%, 10%, 15%, 20%, 10%, 15%, 5%, 20% = 100%.
	fractions := []float64{0.05, 0.10, 0.15, 0.20, 0.10, 0.15, 0.05, 0.20}
	ids := make([]string, len(fractions))

	for i, frac := range fractions {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("frag-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		ids[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, pool, ids[i], frac)
	}

	// Start all sequentially (deterministic allocation order for predictable fragmentation).
	for i, id := range ids {
		_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
			ExperimentId: id,
		}))
		require.NoError(t, err, "start experiment %d", i)
	}

	// Conclude indices 1, 3, 5 (10% + 20% + 15% = 45% freed in non-contiguous gaps).
	// With sequential allocation the gaps are:
	//   [500, 1499]  = 1000 buckets (10%, from index 1)
	//   [3000, 4999] = 2000 buckets (20%, from index 3)
	//   [6000, 7499] = 1500 buckets (15%, from index 5)
	// Allocator uses first-fit contiguous, so a 15% (1500 bucket) request fits in the 2000-bucket gap.
	concludeIndices := []int{1, 3, 5}
	for _, idx := range concludeIndices {
		_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
			ExperimentId: ids[idx],
		}))
		require.NoError(t, err, "conclude experiment at index %d", idx)
	}

	// Start new 15% experiment → should succeed (first-fit finds the 2000-bucket gap at [3000, 4999]).
	expNew, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("frag-new-15pct", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, expNew.Msg.ExperimentId, 0.15)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: expNew.Msg.ExperimentId,
	}))
	require.NoError(t, err, "15%% experiment should fit in largest contiguous gap")

	// Verify no overlap with remaining active allocations.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)
	assertNoOverlap(t, allocs.Msg.Allocations, "fragmentation-recovery")

	// 5 original active (indices 0, 2, 4, 6, 7) + 1 new = 6 active.
	activeAllocs := filterActiveAllocations(allocs.Msg.Allocations)
	assert.Len(t, activeAllocs, 6,
		"should have 6 active allocations (5 surviving + 1 new)")
}

// TestStress_ConcurrentAllocDuringCooldown races allocation attempts against
// cooldown expiration. All attempts during cooldown must fail; after cooldown
// they must all succeed.
func TestStress_ConcurrentAllocDuringCooldown(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	pool := env.pool
	ctx := context.Background()

	// Layer with 3-second cooldown.
	layer := createTestLayerWithBuckets(t, client, "stress-concurrent-cool-"+t.Name(), 10000, 3)

	// Create and start 5 experiments at 20% each → 100%.
	const numExperiments = 5
	ids := make([]string, numExperiments)
	for i := 0; i < numExperiments; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("cc-orig-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		ids[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, pool, ids[i], 0.2)
	}

	for i, id := range ids {
		_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
			ExperimentId: id,
		}))
		require.NoError(t, err, "start original experiment %d", i)
	}

	// Conclude all → all in cooldown.
	for i, id := range ids {
		_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
			ExperimentId: id,
		}))
		require.NoError(t, err, "conclude original experiment %d", i)
	}

	// Immediately create 5 new experiments and try to start them concurrently → all must fail.
	duringCooldownIDs := make([]string, numExperiments)
	for i := 0; i < numExperiments; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("cc-during-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		duringCooldownIDs[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, pool, duringCooldownIDs[i], 0.2)
	}

	var failsDuringCooldown atomic.Int32
	var wg sync.WaitGroup
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: duringCooldownIDs[idx],
			}))
			if err != nil && connect.CodeOf(err) == connect.CodeResourceExhausted {
				failsDuringCooldown.Add(1)
			}
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(numExperiments), failsDuringCooldown.Load(),
		"all 5 experiments should fail during cooldown")

	// Wait for cooldown to expire.
	time.Sleep(4 * time.Second)

	// Create 5 fresh experiments and start them concurrently → all must succeed.
	afterCooldownIDs := make([]string, numExperiments)
	for i := 0; i < numExperiments; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("cc-after-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		afterCooldownIDs[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, pool, afterCooldownIDs[i], 0.2)
	}

	var successesAfter atomic.Int32
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: afterCooldownIDs[idx],
			}))
			if err == nil {
				successesAfter.Add(1)
			}
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(numExperiments), successesAfter.Load(),
		"all 5 experiments should succeed after cooldown expires")

	// Verify no overlapping allocations.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)
	assertNoOverlap(t, allocs.Msg.Allocations, "post-cooldown-concurrent")
}

// --- Phase 5 Concurrent State Transition Stress Tests ---
// These tests harden every lifecycle transition under concurrent contention,
// verifying atomicity, audit integrity, and correct error codes.

// TestStress_ConcurrentConclude_100 races 100 goroutines trying to conclude
// the same RUNNING experiment. Exactly one must succeed; all others must get
// FAILED_PRECONDITION. No partial state (e.g., stuck in CONCLUDING) allowed.
func TestStress_ConcurrentConclude_100(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-cc100-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("cc-conclude-100", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	// Start the experiment so it's in RUNNING state.
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	const goroutines = 100
	var successes atomic.Int32
	var preconditionErrors atomic.Int32
	var otherErrors atomic.Int32
	var wg sync.WaitGroup

	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
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
		"exactly 1 goroutine should succeed concluding the experiment")
	assert.Equal(t, int32(goroutines-1), preconditionErrors.Load(),
		"all other goroutines should get FAILED_PRECONDITION")
	assert.Equal(t, int32(0), otherErrors.Load(),
		"no unexpected errors should occur")

	// Verify final state is CONCLUDED (not stuck in CONCLUDING).
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, got.Msg.State,
		"experiment should be in CONCLUDED state after concurrent conclude attempts")

	// Verify audit trail: create(1) + start(2) + conclude(2) = 5 entries total.
	entries := fetchAuditRows(t, env.pool, id)
	concludeEntries := 0
	for _, e := range entries {
		if e.Action == "conclude" {
			concludeEntries++
		}
	}
	assert.Equal(t, 2, concludeEntries,
		"exactly 2 conclude audit entries (RUNNING→CONCLUDING + CONCLUDING→CONCLUDED), got actions: %v",
		auditActions(entries))
}

// TestStress_ConcurrentArchive_50 races 50 goroutines trying to archive
// the same CONCLUDED experiment. Exactly one must succeed.
func TestStress_ConcurrentArchive_50(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-ca50-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("ca-archive-50", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	// Drive to CONCLUDED state.
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	_, err = client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	const goroutines = 50
	var successes atomic.Int32
	var preconditionErrors atomic.Int32
	var otherErrors atomic.Int32
	var wg sync.WaitGroup

	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.ArchiveExperiment(ctx, connect.NewRequest(&mgmtv1.ArchiveExperimentRequest{
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
		"exactly 1 goroutine should succeed archiving the experiment")
	assert.Equal(t, int32(goroutines-1), preconditionErrors.Load(),
		"all other goroutines should get FAILED_PRECONDITION")
	assert.Equal(t, int32(0), otherErrors.Load(),
		"no unexpected errors should occur")

	// Verify terminal state.
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED, got.Msg.State)

	// Verify audit trail: create(1) + start(2) + conclude(2) + archive(1) = 6
	entries := fetchAuditRows(t, env.pool, id)
	archiveEntries := 0
	for _, e := range entries {
		if e.Action == "archive" {
			archiveEntries++
		}
	}
	assert.Equal(t, 1, archiveEntries,
		"exactly 1 archive audit entry, got actions: %v", auditActions(entries))
}

// TestStress_ConcurrentPauseResume races pause and resume operations on the
// same RUNNING experiment from 50 goroutines each (100 total). All should
// succeed since RUNNING→RUNNING is always valid. The audit trail must contain
// exactly as many entries as successful operations.
func TestStress_ConcurrentPauseResume(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-pr-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("pr-concurrent", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	const pauseGoroutines = 50
	const resumeGoroutines = 50
	var pauseSuccesses atomic.Int32
	var resumeSuccesses atomic.Int32
	var wg sync.WaitGroup

	// Fire pauses and resumes concurrently.
	for i := 0; i < pauseGoroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.PauseExperiment(ctx, connect.NewRequest(&mgmtv1.PauseExperimentRequest{
				ExperimentId: id,
				Reason:       "stress test",
			}))
			if err == nil {
				pauseSuccesses.Add(1)
			}
		}()
	}
	for i := 0; i < resumeGoroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.ResumeExperiment(ctx, connect.NewRequest(&mgmtv1.ResumeExperimentRequest{
				ExperimentId: id,
			}))
			if err == nil {
				resumeSuccesses.Add(1)
			}
		}()
	}
	wg.Wait()

	// All should succeed since the experiment is RUNNING throughout.
	assert.Equal(t, int32(pauseGoroutines), pauseSuccesses.Load(),
		"all pause operations should succeed on RUNNING experiment")
	assert.Equal(t, int32(resumeGoroutines), resumeSuccesses.Load(),
		"all resume operations should succeed on RUNNING experiment")

	// Experiment should still be RUNNING.
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, got.Msg.State)

	// Audit trail must have: create(1) + start(2) + pause(50) + resume(50) = 103.
	entries := fetchAuditRows(t, env.pool, id)
	var pauseCount, resumeCount int
	for _, e := range entries {
		switch e.Action {
		case "pause":
			pauseCount++
		case "resume":
			resumeCount++
		}
	}
	assert.Equal(t, pauseGoroutines, pauseCount,
		"audit trail should have exactly %d pause entries", pauseGoroutines)
	assert.Equal(t, resumeGoroutines, resumeCount,
		"audit trail should have exactly %d resume entries", resumeGoroutines)
}

// TestStress_ConcurrentMixedTransitions fires multiple different transition
// types simultaneously on the same RUNNING experiment: 10 pause, 10 resume,
// 10 conclude, 10 archive, 10 start. Only conclude should succeed (once);
// start/archive must all fail; pause/resume should succeed until conclude wins.
func TestStress_ConcurrentMixedTransitions(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-mixed-trans-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("mixed-trans", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	const perOp = 10
	var concludeSuccesses atomic.Int32
	var startErrors atomic.Int32
	var archiveErrors atomic.Int32
	var wg sync.WaitGroup

	// Conclude attempts.
	for i := 0; i < perOp; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
				ExperimentId: id,
			}))
			if err == nil {
				concludeSuccesses.Add(1)
			}
		}()
	}

	// Start attempts (invalid from RUNNING).
	for i := 0; i < perOp; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: id,
			}))
			if err != nil {
				startErrors.Add(1)
			}
		}()
	}

	// Archive attempts (invalid from RUNNING).
	for i := 0; i < perOp; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.ArchiveExperiment(ctx, connect.NewRequest(&mgmtv1.ArchiveExperimentRequest{
				ExperimentId: id,
			}))
			if err != nil {
				archiveErrors.Add(1)
			}
		}()
	}

	// Pause attempts (valid while RUNNING, fail after conclude).
	for i := 0; i < perOp; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			client.PauseExperiment(ctx, connect.NewRequest(&mgmtv1.PauseExperimentRequest{
				ExperimentId: id,
				Reason:       "mixed stress",
			}))
		}()
	}

	// Resume attempts (valid while RUNNING, fail after conclude).
	for i := 0; i < perOp; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			client.ResumeExperiment(ctx, connect.NewRequest(&mgmtv1.ResumeExperimentRequest{
				ExperimentId: id,
			}))
		}()
	}

	wg.Wait()

	// Exactly one conclude must succeed.
	assert.Equal(t, int32(1), concludeSuccesses.Load(),
		"exactly 1 conclude should succeed")

	// All start and archive attempts must fail.
	assert.Equal(t, int32(perOp), startErrors.Load(),
		"all start attempts should fail on RUNNING/CONCLUDED experiment")
	assert.Equal(t, int32(perOp), archiveErrors.Load(),
		"all archive attempts should fail on RUNNING/CONCLUDED experiment")

	// Final state must be CONCLUDED (the only valid terminal transition from RUNNING).
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, got.Msg.State)

	// Verify no duplicate conclude audit entries.
	entries := fetchAuditRows(t, env.pool, id)
	concludeCount := 0
	for _, e := range entries {
		if e.Action == "conclude" {
			concludeCount++
		}
	}
	assert.Equal(t, 2, concludeCount,
		"exactly 2 conclude audit entries (RUNNING→CONCLUDING + CONCLUDING→CONCLUDED), got actions: %v",
		auditActions(entries))
}

// TestStress_ConcurrentConcludeAndAllocate concludes one experiment and starts
// another on the same layer simultaneously. The layer has exactly 100% capacity
// used by the concluding experiment. The new experiment must wait for the
// conclude to release capacity (or fail if it races ahead).
func TestStress_ConcurrentConcludeAndAllocate(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	pool := env.pool
	ctx := context.Background()

	// Layer with 0-second cooldown so released buckets are immediately reusable.
	layer := createTestLayerWithBuckets(t, client, "stress-ca-"+t.Name(), 10000, 0)

	// Experiment-A at 50%.
	expA, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("ca-a", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, expA.Msg.ExperimentId, 0.5)
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: expA.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Experiment-B at 50%.
	expB, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("ca-b", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, expB.Msg.ExperimentId, 0.5)
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: expB.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Layer is now 100% full. Prepare experiment-C at 50%.
	expC, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("ca-c", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, pool, expC.Msg.ExperimentId, 0.5)

	// Concurrently: conclude A (freeing 50%) and start C (needing 50%).
	var concludeErr error
	var startErr error
	var wg sync.WaitGroup

	wg.Add(2)
	go func() {
		defer wg.Done()
		_, concludeErr = client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
			ExperimentId: expA.Msg.ExperimentId,
		}))
	}()
	go func() {
		defer wg.Done()
		_, startErr = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
			ExperimentId: expC.Msg.ExperimentId,
		}))
	}()
	wg.Wait()

	// The conclude must always succeed.
	assert.NoError(t, concludeErr, "conclude should succeed")

	// The start may succeed or fail depending on race order.
	if startErr != nil {
		// If it failed, it should be ResourceExhausted (raced ahead of conclude).
		assert.Equal(t, connect.CodeResourceExhausted, connect.CodeOf(startErr),
			"start failure should be ResourceExhausted, not a different error")

		// Experiment-C should be back in DRAFT (rolled back from STARTING).
		gotC, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
			ExperimentId: expC.Msg.ExperimentId,
		}))
		require.NoError(t, err)
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, gotC.Msg.State,
			"failed start should roll back to DRAFT")
	} else {
		// If it succeeded, verify no overlapping allocations.
		allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
			LayerId: layer.LayerId,
		}))
		require.NoError(t, err)
		assertNoOverlap(t, allocs.Msg.Allocations, "conclude-and-allocate")

		gotC, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
			ExperimentId: expC.Msg.ExperimentId,
		}))
		require.NoError(t, err)
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, gotC.Msg.State)
	}

	// Experiment-A must be CONCLUDED either way.
	gotA, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: expA.Msg.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, gotA.Msg.State)

	// Experiment-B must still be RUNNING, unaffected.
	gotB, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: expB.Msg.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, gotB.Msg.State)
}

// TestStress_ConcurrentFullLifecycleRace drives 10 experiments through their
// entire lifecycle (create→start→conclude→archive) concurrently on the same
// layer with 10% traffic each. This tests the full pipeline under contention,
// verifying that all transitions complete without deadlocks or inconsistencies.
func TestStress_ConcurrentFullLifecycleRace(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	pool := env.pool
	ctx := context.Background()

	layer := createTestLayer(t, client, "stress-flr-"+t.Name(), 0)

	const numExperiments = 10
	ids := make([]string, numExperiments)
	for i := 0; i < numExperiments; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("flr-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		ids[i] = exp.Msg.ExperimentId
		setTrafficPercentage(t, pool, ids[i], 0.1)
	}

	// All start concurrently.
	var startSuccesses atomic.Int32
	var wg sync.WaitGroup
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: ids[idx],
			}))
			if err == nil {
				startSuccesses.Add(1)
			}
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(numExperiments), startSuccesses.Load(),
		"all experiments should start (10%% each = 100%%)")

	// All conclude concurrently.
	var concludeSuccesses atomic.Int32
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
				ExperimentId: ids[idx],
			}))
			if err == nil {
				concludeSuccesses.Add(1)
			}
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(numExperiments), concludeSuccesses.Load(),
		"all experiments should conclude")

	// All archive concurrently.
	var archiveSuccesses atomic.Int32
	for i := 0; i < numExperiments; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, err := client.ArchiveExperiment(ctx, connect.NewRequest(&mgmtv1.ArchiveExperimentRequest{
				ExperimentId: ids[idx],
			}))
			if err == nil {
				archiveSuccesses.Add(1)
			}
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(numExperiments), archiveSuccesses.Load(),
		"all experiments should archive")

	// All should be ARCHIVED with complete audit trails.
	for i, id := range ids {
		got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
			ExperimentId: id,
		}))
		require.NoError(t, err)
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED, got.Msg.State,
			"experiment %d should be ARCHIVED", i)

		entries := fetchAuditRows(t, pool, id)
		// create(1) + start(2) + conclude(2) + archive(1) = 6
		assert.Len(t, entries, 6,
			"experiment %d should have 6 audit entries, got actions: %v",
			i, auditActions(entries))
	}

	// All allocations should be released.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)
	activeAllocs := filterActiveAllocations(allocs.Msg.Allocations)
	assert.Len(t, activeAllocs, 0,
		"all allocations should be released after archiving all experiments")
}
