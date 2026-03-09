//go:build integration

package handlers_test

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync"
	"sync/atomic"
	"testing"

	"connectrpc.com/connect"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/types/known/durationpb"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/handlers"
	"github.com/org/experimentation-platform/services/management/internal/sequential"
	"github.com/org/experimentation-platform/services/management/internal/store"
)

type testEnv struct {
	client managementv1connect.ExperimentManagementServiceClient
	pool   *pgxpool.Pool
}

func setupTestServer(t *testing.T) (testEnv, func()) {
	t.Helper()

	ctx := context.Background()
	pool, err := store.NewPool(ctx)
	require.NoError(t, err)

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	ls := store.NewLayerStore(pool)
	ms := store.NewMetricStore(pool)
	ts := store.NewTargetingStore(pool)
	ss := store.NewSurrogateStore(pool)
	svc := handlers.NewExperimentService(es, as, ls, ms, ts, ss, nil)

	mux := http.NewServeMux()
	path, handler := managementv1connect.NewExperimentManagementServiceHandler(svc)
	mux.Handle(path, handler)

	server := httptest.NewServer(mux)
	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, server.URL,
	)

	return testEnv{client: client, pool: pool}, func() {
		server.Close()
		pool.Close()
	}
}

// setTrafficPercentage sets the traffic_percentage in an experiment's type_config JSONB.
func setTrafficPercentage(t *testing.T, pool *pgxpool.Pool, experimentID string, pct float64) {
	t.Helper()
	_, err := pool.Exec(context.Background(), `
		UPDATE experiments
		SET type_config = type_config || jsonb_build_object('traffic_percentage', $2::float8)
		WHERE experiment_id = $1`, experimentID, pct)
	require.NoError(t, err)
}

func newABExperiment(name string) *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:            name,
		OwnerEmail:      "test@example.com",
		LayerId:         "a0000000-0000-0000-0000-000000000001", // default layer from seed
		PrimaryMetricId: "watch_time_minutes",
		Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		Variants: []*commonv1.Variant{
			{Name: "control", TrafficFraction: 0.5, IsControl: true},
			{Name: "treatment", TrafficFraction: 0.5, IsControl: false},
		},
	}
}

func TestFullLifecycle(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "lifecycle-layer-"+t.Name(), 0)

	// Create
	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("lifecycle-test", layer.LayerId),
	}))
	require.NoError(t, err)
	exp := created.Msg
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, exp.State)
	assert.NotEmpty(t, exp.ExperimentId)
	assert.NotEmpty(t, exp.HashSalt)
	assert.Len(t, exp.Variants, 2)

	// Start: DRAFT → STARTING → RUNNING
	started, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, started.Msg.State)
	assert.NotNil(t, started.Msg.StartedAt)

	// Start again → FAILED_PRECONDITION
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp.ExperimentId,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeFailedPrecondition, connect.CodeOf(err))

	// Conclude: RUNNING → CONCLUDING → CONCLUDED
	concluded, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
		ExperimentId: exp.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, concluded.Msg.State)
	assert.NotNil(t, concluded.Msg.ConcludedAt)

	// Archive: CONCLUDED → ARCHIVED
	archived, err := client.ArchiveExperiment(ctx, connect.NewRequest(&mgmtv1.ArchiveExperimentRequest{
		ExperimentId: exp.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED, archived.Msg.State)
}

func TestConcurrentStart(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "concurrent-start-layer-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("concurrent-start-test", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	var successes atomic.Int32
	var wg sync.WaitGroup
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
				ExperimentId: id,
			}))
			if err == nil {
				successes.Add(1)
			}
		}()
	}
	wg.Wait()
	assert.Equal(t, int32(1), successes.Load(), "exactly 1 goroutine should succeed")
}

func TestListWithPagination(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	for i := 0; i < 5; i++ {
		_, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperiment("list-test-" + string(rune('A'+i))),
		}))
		require.NoError(t, err)
	}

	resp, err := client.ListExperiments(ctx, connect.NewRequest(&mgmtv1.ListExperimentsRequest{
		PageSize: 3,
	}))
	require.NoError(t, err)
	assert.Len(t, resp.Msg.Experiments, 3)
	assert.NotEmpty(t, resp.Msg.NextPageToken)

	resp2, err := client.ListExperiments(ctx, connect.NewRequest(&mgmtv1.ListExperimentsRequest{
		PageSize:  3,
		PageToken: resp.Msg.NextPageToken,
	}))
	require.NoError(t, err)
	assert.True(t, len(resp2.Msg.Experiments) >= 2, "expected at least 2 more items")
}

func TestListWithStateFilter(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	resp, err := client.ListExperiments(ctx, connect.NewRequest(&mgmtv1.ListExperimentsRequest{
		StateFilter: commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT,
		PageSize:    100,
	}))
	require.NoError(t, err)
	for _, exp := range resp.Msg.Experiments {
		assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, exp.State)
	}
}

func TestUpdateOnNonDraft(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "update-nondraft-layer-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("update-on-running-test", layer.LayerId),
	}))
	require.NoError(t, err)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: created.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	_, err = client.UpdateExperiment(ctx, connect.NewRequest(&mgmtv1.UpdateExperimentRequest{
		Experiment: &commonv1.Experiment{
			ExperimentId:    created.Msg.ExperimentId,
			Name:            "updated-name",
			OwnerEmail:      "test@example.com",
			LayerId:         layer.LayerId,
			PrimaryMetricId: "watch_time_minutes",
			Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
			Variants: []*commonv1.Variant{
				{Name: "control", TrafficFraction: 0.5, IsControl: true},
				{Name: "treatment", TrafficFraction: 0.5, IsControl: false},
			},
		},
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeFailedPrecondition, connect.CodeOf(err))
}

func TestValidationErrors(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	t.Run("fractions not summing to 1.0", func(t *testing.T) {
		exp := newABExperiment("bad-fractions")
		exp.Variants[0].TrafficFraction = 0.3
		exp.Variants[1].TrafficFraction = 0.3
		_, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: exp,
		}))
		require.Error(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	})

	t.Run("missing control variant", func(t *testing.T) {
		exp := newABExperiment("no-control")
		exp.Variants[0].IsControl = false
		_, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: exp,
		}))
		require.Error(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	})
}

// --- Bucket Allocation Tests ---

func createTestLayer(t *testing.T, client managementv1connect.ExperimentManagementServiceClient, name string, cooldownSeconds int64) *commonv1.Layer {
	t.Helper()
	resp, err := client.CreateLayer(context.Background(), connect.NewRequest(&mgmtv1.CreateLayerRequest{
		Layer: &commonv1.Layer{
			Name:                name,
			Description:         "test layer",
			TotalBuckets:        10000,
			BucketReuseCooldown: &durationpb.Duration{Seconds: cooldownSeconds},
		},
	}))
	require.NoError(t, err)
	return resp.Msg
}

func newABExperimentInLayer(name, layerID string) *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:            name,
		OwnerEmail:      "test@example.com",
		LayerId:         layerID,
		PrimaryMetricId: "watch_time_minutes",
		Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		Variants: []*commonv1.Variant{
			{Name: "control", TrafficFraction: 0.5, IsControl: true},
			{Name: "treatment", TrafficFraction: 0.5, IsControl: false},
		},
	}
}

func TestBucketAllocation_TwoExperiments(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "alloc-test-two-"+t.Name(), 0)

	// Create 2 experiments and set traffic_percentage to 50% via direct DB update.
	exp1, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("alloc-50-a", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, env.pool, exp1.Msg.ExperimentId, 0.5)

	exp2, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("alloc-50-b", layer.LayerId),
	}))
	require.NoError(t, err)
	setTrafficPercentage(t, env.pool, exp2.Msg.ExperimentId, 0.5)

	// Start both — should succeed with non-overlapping ranges.
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp1.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp2.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Verify allocations via GetLayerAllocations.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)
	require.Len(t, allocs.Msg.Allocations, 2)

	a1 := allocs.Msg.Allocations[0]
	a2 := allocs.Msg.Allocations[1]

	// Verify non-overlapping: a1.end < a2.start (sorted by start_bucket).
	assert.True(t, a1.EndBucket < a2.StartBucket,
		"allocations should not overlap: [%d-%d] vs [%d-%d]",
		a1.StartBucket, a1.EndBucket, a2.StartBucket, a2.EndBucket)

	// Each should be 5000 buckets.
	size1 := a1.EndBucket - a1.StartBucket + 1
	size2 := a2.EndBucket - a2.StartBucket + 1
	assert.Equal(t, int32(5000), size1, "first allocation should be 5000 buckets")
	assert.Equal(t, int32(5000), size2, "second allocation should be 5000 buckets")
}

func TestBucketAllocation_InsufficientCapacity(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, fmt.Sprintf("alloc-test-exhaust-%s", t.Name()), 0)

	// Create and start a 100% experiment (default traffic_percentage).
	exp1, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("exhaust-100", layer.LayerId),
	}))
	require.NoError(t, err)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp1.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Try to start a second experiment → ResourceExhausted.
	exp2, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("exhaust-fail", layer.LayerId),
	}))
	require.NoError(t, err)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp2.Msg.ExperimentId,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeResourceExhausted, connect.CodeOf(err))

	// Verify the failed experiment is back in DRAFT.
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: exp2.Msg.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, got.Msg.State)
}

func TestBucketReuse_AfterCooldown(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	// Create layer with 0s cooldown so reuse is immediate.
	layer := createTestLayer(t, client, fmt.Sprintf("reuse-test-%s", t.Name()), 0)

	// Start a 100% experiment.
	exp1, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("reuse-first", layer.LayerId),
	}))
	require.NoError(t, err)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp1.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Conclude it (releases with 0s cooldown).
	_, err = client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
		ExperimentId: exp1.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	// Verify allocation is released.
	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId:         layer.LayerId,
		IncludeReleased: true,
	}))
	require.NoError(t, err)
	require.Len(t, allocs.Msg.Allocations, 1)
	assert.NotNil(t, allocs.Msg.Allocations[0].ReleasedAt, "allocation should be released")

	// Start a new 100% experiment → should succeed because cooldown expired.
	exp2, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("reuse-second", layer.LayerId),
	}))
	require.NoError(t, err)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp2.Msg.ExperimentId,
	}))
	require.NoError(t, err)
}

func TestConcurrentBucketAllocation(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, fmt.Sprintf("concurrent-alloc-%s", t.Name()), 0)

	// Create 2 experiments (default 100% each).
	var ids [2]string
	for i := 0; i < 2; i++ {
		exp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(fmt.Sprintf("concurrent-%d", i), layer.LayerId),
		}))
		require.NoError(t, err)
		ids[i] = exp.Msg.ExperimentId
	}

	// Race 2 goroutines to start them.
	var successes atomic.Int32
	var wg sync.WaitGroup
	for i := 0; i < 2; i++ {
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

	// Both are 100% so only 1 can succeed.
	assert.Equal(t, int32(1), successes.Load(), "exactly 1 goroutine should succeed with 100% allocation")
}

// --- Pause/Resume Tests ---

func TestPauseExperiment_Running(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "pause-running-layer-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("pause-running-test", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	paused, err := client.PauseExperiment(ctx, connect.NewRequest(&mgmtv1.PauseExperimentRequest{
		ExperimentId: id,
		Reason:       "testing pause",
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, paused.Msg.State,
		"experiment should remain RUNNING after pause (per ADR-005)")

	// Verify audit trail has a "pause" entry.
	var action string
	err = env.pool.QueryRow(ctx,
		`SELECT action FROM audit_trail WHERE experiment_id = $1 AND action = 'pause'`, id,
	).Scan(&action)
	require.NoError(t, err)
	assert.Equal(t, "pause", action)
}

func TestResumeExperiment_AfterPause(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "resume-pause-layer-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("resume-after-pause-test", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	_, err = client.PauseExperiment(ctx, connect.NewRequest(&mgmtv1.PauseExperimentRequest{
		ExperimentId: id,
		Reason:       "testing",
	}))
	require.NoError(t, err)

	resumed, err := client.ResumeExperiment(ctx, connect.NewRequest(&mgmtv1.ResumeExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, resumed.Msg.State)

	// Verify audit trail has a "resume" entry.
	var action string
	err = env.pool.QueryRow(ctx,
		`SELECT action FROM audit_trail WHERE experiment_id = $1 AND action = 'resume'`, id,
	).Scan(&action)
	require.NoError(t, err)
	assert.Equal(t, "resume", action)
}

func TestPauseExperiment_NonRunning(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperiment("pause-draft-test"),
	}))
	require.NoError(t, err)

	_, err = client.PauseExperiment(ctx, connect.NewRequest(&mgmtv1.PauseExperimentRequest{
		ExperimentId: created.Msg.ExperimentId,
		Reason:       "should fail",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeFailedPrecondition, connect.CodeOf(err))
}

func TestResumeExperiment_NonRunning(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperiment("resume-draft-test"),
	}))
	require.NoError(t, err)

	_, err = client.ResumeExperiment(ctx, connect.NewRequest(&mgmtv1.ResumeExperimentRequest{
		ExperimentId: created.Msg.ExperimentId,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeFailedPrecondition, connect.CodeOf(err))
}

func TestPauseExperiment_GuardrailAutoPause(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "guardrail-autopause-layer-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("guardrail-auto-pause-test", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	_, err = client.PauseExperiment(ctx, connect.NewRequest(&mgmtv1.PauseExperimentRequest{
		ExperimentId:         id,
		Reason:               "guardrail breach detected",
		IsGuardrailAutoPause: true,
	}))
	require.NoError(t, err)

	// Verify audit action is "guardrail_auto_pause".
	var action string
	err = env.pool.QueryRow(ctx,
		`SELECT action FROM audit_trail WHERE experiment_id = $1 AND action = 'guardrail_auto_pause'`, id,
	).Scan(&action)
	require.NoError(t, err)
	assert.Equal(t, "guardrail_auto_pause", action)
}

// --- STARTING Validation Gate Tests ---

func TestStartExperiment_BadPrimaryMetric(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "bad-primary-layer-"+t.Name(), 0)

	exp := newABExperimentInLayer("bad-primary-metric-test", layer.LayerId)
	exp.PrimaryMetricId = "nonexistent_metric_xyz"

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: exp,
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	assert.Contains(t, err.Error(), "nonexistent_metric_xyz")

	// Verify rolled back to DRAFT.
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, got.Msg.State)
}

func TestStartExperiment_BadGuardrailMetric(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "bad-guardrail-layer-"+t.Name(), 0)

	exp := newABExperimentInLayer("bad-guardrail-metric-test", layer.LayerId)
	exp.GuardrailConfigs = []*commonv1.GuardrailConfig{
		{
			MetricId:                    "nonexistent_guardrail_metric",
			Threshold:                   0.05,
			ConsecutiveBreachesRequired: 3,
		},
	}

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: exp,
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	assert.Contains(t, err.Error(), "nonexistent_guardrail_metric")
}

// --- Type-Specific Experiment Helpers ---

func newInterleavingExperiment(name, layerID string) *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:            name,
		OwnerEmail:      "test@example.com",
		LayerId:         layerID,
		PrimaryMetricId: "watch_time_minutes",
		Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING,
		Variants: []*commonv1.Variant{
			{Name: "control", TrafficFraction: 0.5, IsControl: true},
			{Name: "treatment", TrafficFraction: 0.5, IsControl: false},
		},
		InterleavingConfig: &commonv1.InterleavingConfig{
			Method:       commonv1.InterleavingMethod_INTERLEAVING_METHOD_TEAM_DRAFT,
			AlgorithmIds: []string{"algo-a", "algo-b"},
		},
	}
}

func newBanditExperiment(name, layerID string) *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:            name,
		OwnerEmail:      "test@example.com",
		LayerId:         layerID,
		PrimaryMetricId: "watch_time_minutes",
		Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_MAB,
		Variants: []*commonv1.Variant{
			{Name: "arm-a", TrafficFraction: 0.5, IsControl: false},
			{Name: "arm-b", TrafficFraction: 0.5, IsControl: false},
		},
		BanditConfig: &commonv1.BanditConfig{
			Algorithm:      commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
			RewardMetricId: "watch_time_minutes",
		},
	}
}

func newSessionExperiment(name, layerID string) *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:            name,
		OwnerEmail:      "test@example.com",
		LayerId:         layerID,
		PrimaryMetricId: "watch_time_minutes",
		Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL,
		Variants: []*commonv1.Variant{
			{Name: "control", TrafficFraction: 0.5, IsControl: true},
			{Name: "treatment", TrafficFraction: 0.5, IsControl: false},
		},
		SessionConfig: &commonv1.SessionConfig{
			SessionIdAttribute: "session_id",
		},
	}
}

// --- Type-Specific Start Validation Tests ---

func TestStartExperiment_BanditBadRewardMetric(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "bandit-bad-reward-layer-"+t.Name(), 0)
	exp := newBanditExperiment("bandit-bad-reward-metric", layer.LayerId)
	exp.BanditConfig.RewardMetricId = "nonexistent_reward_metric_xyz"

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: exp,
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	assert.Contains(t, err.Error(), "nonexistent_reward_metric_xyz")

	// Verify rolled back to DRAFT.
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, got.Msg.State)
}

func TestStartExperiment_InterleavingValid(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "interleaving-valid-layer-"+t.Name(), 0)
	exp := newInterleavingExperiment("interleaving-start-valid", layer.LayerId)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: exp,
	}))
	require.NoError(t, err)

	started, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: created.Msg.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, started.Msg.State)
}

func TestStartExperiment_SessionValid(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "session-valid-layer-"+t.Name(), 0)
	exp := newSessionExperiment("session-start-valid", layer.LayerId)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: exp,
	}))
	require.NoError(t, err)

	started, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: created.Msg.ExperimentId,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, started.Msg.State)
}

func TestLayerCRUD(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	created, err := client.CreateLayer(ctx, connect.NewRequest(&mgmtv1.CreateLayerRequest{
		Layer: &commonv1.Layer{
			Name:                fmt.Sprintf("crud-test-layer-%s", t.Name()),
			Description:         "test layer for CRUD",
			TotalBuckets:        5000,
			BucketReuseCooldown: &durationpb.Duration{Seconds: 3600},
		},
	}))
	require.NoError(t, err)
	assert.NotEmpty(t, created.Msg.LayerId)
	assert.Equal(t, fmt.Sprintf("crud-test-layer-%s", t.Name()), created.Msg.Name)
	assert.Equal(t, int32(5000), created.Msg.TotalBuckets)

	got, err := client.GetLayer(ctx, connect.NewRequest(&mgmtv1.GetLayerRequest{
		LayerId: created.Msg.LayerId,
	}))
	require.NoError(t, err)
	assert.Equal(t, created.Msg.LayerId, got.Msg.LayerId)

	allocs, err := client.GetLayerAllocations(ctx, connect.NewRequest(&mgmtv1.GetLayerAllocationsRequest{
		LayerId: created.Msg.LayerId,
	}))
	require.NoError(t, err)
	assert.Len(t, allocs.Msg.Allocations, 0)
}

// --- Surrogate Model Tests ---

func newSurrogateModel() *commonv1.SurrogateModelConfig {
	return &commonv1.SurrogateModelConfig{
		TargetMetricId:        "90_day_churn_rate",
		InputMetricIds:        []string{"7d_watch_time", "7d_session_freq"},
		ModelType:             commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_LINEAR,
		ObservationWindowDays: 7,
		PredictionHorizonDays: 90,
	}
}

func TestSurrogateCRUD(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	// Create
	created, err := client.CreateSurrogateModel(ctx, connect.NewRequest(&mgmtv1.CreateSurrogateModelRequest{
		Model: newSurrogateModel(),
	}))
	require.NoError(t, err)
	model := created.Msg
	assert.NotEmpty(t, model.ModelId)
	assert.Equal(t, "90_day_churn_rate", model.TargetMetricId)
	assert.Equal(t, []string{"7d_watch_time", "7d_session_freq"}, model.InputMetricIds)
	assert.Equal(t, commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_LINEAR, model.ModelType)
	assert.Equal(t, int32(7), model.ObservationWindowDays)
	assert.Equal(t, int32(90), model.PredictionHorizonDays)
	assert.NotNil(t, model.CreatedAt)

	// Get by ID
	got, err := client.GetSurrogateCalibration(ctx, connect.NewRequest(&mgmtv1.GetSurrogateCalibrationRequest{
		ModelId: model.ModelId,
	}))
	require.NoError(t, err)
	assert.Equal(t, model.ModelId, got.Msg.ModelId)
	assert.Equal(t, model.TargetMetricId, got.Msg.TargetMetricId)

	// Create a second model for pagination testing.
	_, err = client.CreateSurrogateModel(ctx, connect.NewRequest(&mgmtv1.CreateSurrogateModelRequest{
		Model: &commonv1.SurrogateModelConfig{
			TargetMetricId:        "ltv_180d",
			InputMetricIds:        []string{"7d_revenue"},
			ModelType:             commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_GRADIENT_BOOSTED,
			ObservationWindowDays: 14,
			PredictionHorizonDays: 180,
		},
	}))
	require.NoError(t, err)

	// List
	list, err := client.ListSurrogateModels(ctx, connect.NewRequest(&mgmtv1.ListSurrogateModelsRequest{
		PageSize: 1,
	}))
	require.NoError(t, err)
	assert.Len(t, list.Msg.Models, 1)
	assert.NotEmpty(t, list.Msg.NextPageToken)

	// Second page
	list2, err := client.ListSurrogateModels(ctx, connect.NewRequest(&mgmtv1.ListSurrogateModelsRequest{
		PageSize:  1,
		PageToken: list.Msg.NextPageToken,
	}))
	require.NoError(t, err)
	assert.Len(t, list2.Msg.Models, 1)
}

func TestTriggerSurrogateRecalibration(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	// Create a model first.
	created, err := client.CreateSurrogateModel(ctx, connect.NewRequest(&mgmtv1.CreateSurrogateModelRequest{
		Model: newSurrogateModel(),
	}))
	require.NoError(t, err)
	modelID := created.Msg.ModelId

	// Trigger recalibration → success.
	_, err = client.TriggerSurrogateRecalibration(ctx, connect.NewRequest(&mgmtv1.TriggerSurrogateRecalibrationRequest{
		ModelId: modelID,
	}))
	require.NoError(t, err)

	// NOTE: audit_trail.experiment_id has a FK to experiments, so surrogate model
	// operations cannot write audit entries until the schema supports it.

	// Trigger on non-existent model → NOT_FOUND.
	_, err = client.TriggerSurrogateRecalibration(ctx, connect.NewRequest(&mgmtv1.TriggerSurrogateRecalibrationRequest{
		ModelId: "00000000-0000-0000-0000-000000000000",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeNotFound, connect.CodeOf(err))
}

func TestCreateSurrogateModel_Validation(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	t.Run("missing target_metric_id", func(t *testing.T) {
		m := newSurrogateModel()
		m.TargetMetricId = ""
		_, err := client.CreateSurrogateModel(ctx, connect.NewRequest(&mgmtv1.CreateSurrogateModelRequest{
			Model: m,
		}))
		require.Error(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	})

	t.Run("empty input_metric_ids", func(t *testing.T) {
		m := newSurrogateModel()
		m.InputMetricIds = nil
		_, err := client.CreateSurrogateModel(ctx, connect.NewRequest(&mgmtv1.CreateSurrogateModelRequest{
			Model: m,
		}))
		require.Error(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	})

	t.Run("unspecified model_type", func(t *testing.T) {
		m := newSurrogateModel()
		m.ModelType = commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_UNSPECIFIED
		_, err := client.CreateSurrogateModel(ctx, connect.NewRequest(&mgmtv1.CreateSurrogateModelRequest{
			Model: m,
		}))
		require.Error(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	})

	t.Run("prediction_horizon <= observation_window", func(t *testing.T) {
		m := newSurrogateModel()
		m.ObservationWindowDays = 30
		m.PredictionHorizonDays = 30
		_, err := client.CreateSurrogateModel(ctx, connect.NewRequest(&mgmtv1.CreateSurrogateModelRequest{
			Model: m,
		}))
		require.Error(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	})
}

// --- Sequential Auto-Conclude Tests ---

func TestSequentialAutoConclude_ViaConcludeByID(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	// Create and start an AB experiment.
	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperiment("seq-auto-conclude-test"),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	// Set sequential_method directly in DB (simulates a sequential experiment).
	_, err = env.pool.Exec(ctx,
		`UPDATE experiments SET sequential_method = 'MSPRT' WHERE experiment_id = $1`, id)
	require.NoError(t, err)

	// Use the sequential processor with the real expSvc as Concluder.
	es := store.NewExperimentStore(env.pool)
	as := store.NewAuditStore(env.pool)
	ss := store.NewSurrogateStore(env.pool)
	ls := store.NewLayerStore(env.pool)
	ms := store.NewMetricStore(env.pool)
	ts := store.NewTargetingStore(env.pool)
	expSvc := handlers.NewExperimentService(es, as, ls, ms, ts, ss, nil)

	proc := sequential.NewProcessor(es, as, nil, expSvc)

	alert := sequential.BoundaryAlert{
		ExperimentID: id,
		MetricID:     "watch_time_minutes",
		CurrentLook:  5,
		AlphaSpent:   0.045,
	}

	result, procErr := proc.ProcessAlert(ctx, alert)
	require.NoError(t, procErr)
	assert.Equal(t, sequential.ResultConcluded, result)

	// Verify experiment is CONCLUDED.
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, got.Msg.State)

	// Verify audit trail has sequential_boundary_crossed entry.
	var action string
	err = env.pool.QueryRow(ctx,
		`SELECT action FROM audit_trail WHERE experiment_id = $1 AND action = 'sequential_boundary_crossed'`, id,
	).Scan(&action)
	require.NoError(t, err)
	assert.Equal(t, "sequential_boundary_crossed", action)

	// Verify the conclude audit entries have the sequential actor.
	var actor string
	err = env.pool.QueryRow(ctx,
		`SELECT actor_email FROM audit_trail WHERE experiment_id = $1 AND action = 'conclude' AND new_state = 'CONCLUDED'`, id,
	).Scan(&actor)
	require.NoError(t, err)
	assert.Equal(t, "sequential_auto_conclude", actor)
}

// --- Cumulative Holdout Tests ---

func newHoldoutExperiment(name, layerID string) *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:                name,
		OwnerEmail:          "test@example.com",
		LayerId:             layerID,
		PrimaryMetricId:     "watch_time_minutes",
		Type:                commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT,
		IsCumulativeHoldout: true,
		GuardrailAction:     commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY,
		Variants: []*commonv1.Variant{
			{Name: "control", TrafficFraction: 0.95, IsControl: true},
			{Name: "treatment", TrafficFraction: 0.05, IsControl: false},
		},
	}
}

func TestHoldoutLifecycle(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "holdout-lifecycle-"+t.Name(), 0)

	// Create holdout.
	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newHoldoutExperiment("holdout-lifecycle-test", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId
	assert.True(t, created.Msg.IsCumulativeHoldout)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, created.Msg.State)

	// Set traffic_percentage to 5% (required for holdouts).
	setTrafficPercentage(t, env.pool, id, 0.05)

	// Start → RUNNING.
	started, err := client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, started.Msg.State)

	// Conclude → CONCLUDED (holdout retirement).
	concluded, err := client.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED, concluded.Msg.State)

	// Verify holdout_retirement in audit trail.
	var detailsJSON []byte
	err = env.pool.QueryRow(ctx,
		`SELECT details_json FROM audit_trail WHERE experiment_id = $1 AND action = 'conclude' AND new_state = 'CONCLUDING'`,
		id).Scan(&detailsJSON)
	require.NoError(t, err)
	assert.Contains(t, string(detailsJSON), `"holdout_retirement"`)
}

func TestHoldout_BadTrafficPercentage(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "holdout-bad-traffic-"+t.Name(), 0)

	// Create holdout (default traffic_percentage = 100% which is invalid for holdout).
	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newHoldoutExperiment("holdout-bad-traffic", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	// Start without setting traffic_percentage → should fail (default 100% > 5%).
	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
	assert.Contains(t, err.Error(), "CUMULATIVE_HOLDOUT traffic_percentage must be between 1% and 5%")

	// Verify rolled back to DRAFT.
	got, err := client.GetExperiment(ctx, connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT, got.Msg.State)
}

func TestHoldout_TooLowTrafficPercentage(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "holdout-low-traffic-"+t.Name(), 0)

	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newHoldoutExperiment("holdout-low-traffic", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId

	// Set traffic to 0.5% (below 1% minimum).
	setTrafficPercentage(t, env.pool, id, 0.005)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestHoldout_SequentialBypass(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	client := env.client
	ctx := context.Background()

	layer := createTestLayer(t, client, "holdout-seq-bypass-"+t.Name(), 0)

	// Create and start a holdout.
	created, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newHoldoutExperiment("holdout-seq-bypass", layer.LayerId),
	}))
	require.NoError(t, err)
	id := created.Msg.ExperimentId
	setTrafficPercentage(t, env.pool, id, 0.05)

	_, err = client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	// Set sequential_method in DB (simulates config).
	_, err = env.pool.Exec(ctx,
		`UPDATE experiments SET sequential_method = 'MSPRT' WHERE experiment_id = $1`, id)
	require.NoError(t, err)

	// Wire up sequential processor with mock concluder.
	es := store.NewExperimentStore(env.pool)
	as := store.NewAuditStore(env.pool)
	concluder := &mockConcluder{}
	proc := sequential.NewProcessor(es, as, nil, concluder)

	alert := sequential.BoundaryAlert{
		ExperimentID: id,
		MetricID:     "watch_time_minutes",
		CurrentLook:  5,
	}

	result, err := proc.ProcessAlert(ctx, alert)
	require.NoError(t, err)
	assert.Equal(t, sequential.ResultSkipped, result,
		"holdout should skip auto-conclude")
	assert.Len(t, concluder.calls, 0)
}

// mockConcluder tracks ConcludeByID calls for testing holdout sequential bypass.
type mockConcluder struct {
	calls []mockConcludeCall
}

type mockConcludeCall struct {
	ID    string
	Actor string
}

func (m *mockConcluder) ConcludeByID(_ context.Context, id, actor string, _ map[string]any) error {
	m.calls = append(m.calls, mockConcludeCall{ID: id, Actor: actor})
	return nil
}
