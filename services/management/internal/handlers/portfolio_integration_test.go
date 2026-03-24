//go:build integration

package handlers_test

import (
	"context"
	"net/http"
	"testing"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
)

// TestGetPortfolioAllocation_Empty verifies that the RPC returns a well-formed empty
// response when no experiments are RUNNING.
func TestGetPortfolioAllocation_Empty(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	ctx := context.Background()

	resp, err := env.client.GetPortfolioAllocation(ctx, connect.NewRequest(&mgmtv1.GetPortfolioAllocationRequest{}))
	require.NoError(t, err)
	assert.Empty(t, resp.Msg.Allocations)
	assert.Empty(t, resp.Msg.Conflicts)
	assert.NotNil(t, resp.Msg.Stats)
	assert.Equal(t, int32(0), resp.Msg.Stats.RunningCount)
}

// TestGetPortfolioAllocation_SingleRunning creates one experiment, starts it, then
// verifies the portfolio endpoint returns it with the correct metadata.
func TestGetPortfolioAllocation_SingleRunning(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	ctx := context.Background()

	layer := createTestLayer(t, env.client, "portfolio-single-"+t.Name(), 0)

	exp, err := env.client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("portfolio-single-exp", layer.LayerId),
	}))
	require.NoError(t, err)

	_, err = env.client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: exp.Msg.ExperimentId,
	}))
	require.NoError(t, err)

	resp, err := env.client.GetPortfolioAllocation(ctx, connect.NewRequest(&mgmtv1.GetPortfolioAllocationRequest{}))
	require.NoError(t, err)

	// Find our experiment in the allocations.
	var found *mgmtv1.ExperimentAllocation
	for _, a := range resp.Msg.Allocations {
		if a.ExperimentId == exp.Msg.ExperimentId {
			found = a
			break
		}
	}
	require.NotNil(t, found, "started experiment must appear in portfolio allocations")
	assert.Equal(t, int32(3), found.Priority) // default priority
	assert.Greater(t, found.VarianceBudgetShare, 0.0)
	assert.NotEmpty(t, found.Rationale)

	assert.GreaterOrEqual(t, resp.Msg.Stats.RunningCount, int32(1))
}

// TestGetPortfolioAllocation_PriorityOverride verifies that priority_overrides are
// respected in variance budget shares.
func TestGetPortfolioAllocation_PriorityOverride(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	ctx := context.Background()

	layer := createTestLayer(t, env.client, "portfolio-priority-"+t.Name(), 0)

	// Create two experiments with different traffic sizes.
	exp1, err := env.client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("portfolio-high", layer.LayerId),
	}))
	require.NoError(t, err)

	exp2, err := env.client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("portfolio-low", layer.LayerId),
	}))
	require.NoError(t, err)

	// Set different traffic percentages.
	setTrafficPercentage(t, env.pool, exp1.Msg.ExperimentId, 0.3)
	setTrafficPercentage(t, env.pool, exp2.Msg.ExperimentId, 0.3)

	_, err = env.client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{ExperimentId: exp1.Msg.ExperimentId}))
	require.NoError(t, err)

	_, err = env.client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{ExperimentId: exp2.Msg.ExperimentId}))
	require.NoError(t, err)

	// exp1 gets priority 5, exp2 gets priority 1.
	resp, err := env.client.GetPortfolioAllocation(ctx, connect.NewRequest(&mgmtv1.GetPortfolioAllocationRequest{
		PriorityOverrides: map[string]int32{
			exp1.Msg.ExperimentId: 5,
			exp2.Msg.ExperimentId: 1,
		},
	}))
	require.NoError(t, err)

	var alloc1, alloc2 *mgmtv1.ExperimentAllocation
	for _, a := range resp.Msg.Allocations {
		switch a.ExperimentId {
		case exp1.Msg.ExperimentId:
			alloc1 = a
		case exp2.Msg.ExperimentId:
			alloc2 = a
		}
	}
	require.NotNil(t, alloc1)
	require.NotNil(t, alloc2)

	assert.Equal(t, int32(5), alloc1.Priority)
	assert.Equal(t, int32(1), alloc2.Priority)
	// Higher priority → larger variance budget share.
	assert.Greater(t, alloc1.VarianceBudgetShare, alloc2.VarianceBudgetShare)
}

// TestGetPortfolioAllocation_LayerFilter verifies that layer_id filter restricts results.
func TestGetPortfolioAllocation_LayerFilter(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	ctx := context.Background()

	layer1 := createTestLayer(t, env.client, "portfolio-filter-l1-"+t.Name(), 0)
	layer2 := createTestLayer(t, env.client, "portfolio-filter-l2-"+t.Name(), 0)

	// Start experiment in layer1.
	exp1, err := env.client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("portfolio-filter-exp1", layer1.LayerId),
	}))
	require.NoError(t, err)
	_, err = env.client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{ExperimentId: exp1.Msg.ExperimentId}))
	require.NoError(t, err)

	// Start experiment in layer2.
	exp2, err := env.client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: newABExperimentInLayer("portfolio-filter-exp2", layer2.LayerId),
	}))
	require.NoError(t, err)
	_, err = env.client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{ExperimentId: exp2.Msg.ExperimentId}))
	require.NoError(t, err)

	// Query with layer_id filter for layer1 only.
	resp, err := env.client.GetPortfolioAllocation(ctx, connect.NewRequest(&mgmtv1.GetPortfolioAllocationRequest{
		LayerId: layer1.LayerId,
	}))
	require.NoError(t, err)

	for _, a := range resp.Msg.Allocations {
		if a.ExperimentId == exp2.Msg.ExperimentId {
			t.Errorf("layer2 experiment should not appear when filtering by layer1")
		}
	}

	var found bool
	for _, a := range resp.Msg.Allocations {
		if a.ExperimentId == exp1.Msg.ExperimentId {
			found = true
		}
	}
	assert.True(t, found, "layer1 experiment must appear in layer1-filtered response")
}

// TestGetPortfolioAllocation_ConflictDetection starts two experiments with the same
// primary metric in the same layer and checks that a conflict is detected.
func TestGetPortfolioAllocation_ConflictDetection(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()
	ctx := context.Background()

	layer := createTestLayer(t, env.client, "portfolio-conflict-"+t.Name(), 0)

	mkExp := func(name string) string {
		exp, err := env.client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
			Experiment: newABExperimentInLayer(name, layer.LayerId),
		}))
		require.NoError(t, err)
		setTrafficPercentage(t, env.pool, exp.Msg.ExperimentId, 0.3)
		_, err = env.client.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
			ExperimentId: exp.Msg.ExperimentId,
		}))
		require.NoError(t, err)
		return exp.Msg.ExperimentId
	}

	// Both experiments use "watch_time_minutes" as primary metric (set by newABExperimentInLayer).
	id1 := mkExp("portfolio-conflict-a")
	id2 := mkExp("portfolio-conflict-b")

	resp, err := env.client.GetPortfolioAllocation(ctx, connect.NewRequest(&mgmtv1.GetPortfolioAllocationRequest{
		LayerId: layer.LayerId,
	}))
	require.NoError(t, err)

	// Must detect at least a primary metric overlap conflict for our two experiments.
	var conflictFound bool
	for _, c := range resp.Msg.Conflicts {
		sameA := (c.ExperimentIdA == id1 && c.ExperimentIdB == id2)
		sameB := (c.ExperimentIdA == id2 && c.ExperimentIdB == id1)
		if c.ConflictType == mgmtv1.ConflictType_CONFLICT_TYPE_PRIMARY_METRIC_OVERLAP && (sameA || sameB) {
			conflictFound = true
			break
		}
	}
	assert.True(t, conflictFound, "expected primary metric overlap conflict between the two experiments")
	assert.GreaterOrEqual(t, resp.Msg.Stats.ConflictCount, int32(1))
}

// TestGetPortfolioAllocation_RequiresViewer ensures the endpoint rejects unauthenticated requests.
func TestGetPortfolioAllocation_RequiresViewer(t *testing.T) {
	url, _, cleanup := setupTestServerRaw(t)
	defer cleanup()
	ctx := context.Background()

	// Client with no auth headers — should be rejected.
	unauthClient := managementv1connect.NewExperimentManagementServiceClient(http.DefaultClient, url)
	_, err := unauthClient.GetPortfolioAllocation(ctx, connect.NewRequest(&mgmtv1.GetPortfolioAllocationRequest{}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}
