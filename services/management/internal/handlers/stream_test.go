//go:build integration

package handlers_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"connectrpc.com/connect"

	assignmentv1 "github.com/org/experimentation/gen/go/experimentation/assignment/v1"
	"github.com/org/experimentation/gen/go/experimentation/assignment/v1/assignmentv1connect"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/handlers"
	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

const defaultLayerID = "a0000000-0000-0000-0000-000000000001"

func setupStreamTest(t *testing.T) (
	mgmtClient managementv1connect.ExperimentManagementServiceClient,
	streamClient assignmentv1connect.AssignmentServiceClient,
	notifier *streaming.Notifier,
	cleanup func(),
) {
	t.Helper()

	ctx := context.Background()
	pool, err := store.NewPool(ctx)
	require.NoError(t, err)

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	ls := store.NewLayerStore(pool)

	dsn := "postgres://experimentation:localdev@localhost:5432/experimentation?sslmode=disable"
	notifier = streaming.NewNotifier(pool, dsn)
	notifier.Start(ctx)

	ms := store.NewMetricStore(pool)
	expSvc := handlers.NewExperimentService(es, as, ls, ms, notifier)
	streamSvc := handlers.NewConfigStreamService(es, notifier)

	mux := http.NewServeMux()
	mgmtPath, mgmtHandler := managementv1connect.NewExperimentManagementServiceHandler(expSvc)
	mux.Handle(mgmtPath, mgmtHandler)
	streamPath, streamHandler := assignmentv1connect.NewAssignmentServiceHandler(streamSvc)
	mux.Handle(streamPath, streamHandler)

	srv := httptest.NewUnstartedServer(mux)
	srv.EnableHTTP2 = true
	srv.StartTLS()

	mgmtClient = managementv1connect.NewExperimentManagementServiceClient(srv.Client(), srv.URL)
	streamClient = assignmentv1connect.NewAssignmentServiceClient(srv.Client(), srv.URL)

	cleanup = func() {
		notifier.Stop()
		srv.Close()
		pool.Close()
	}

	return mgmtClient, streamClient, notifier, cleanup
}

func createAndStartExperiment(t *testing.T, mgmt managementv1connect.ExperimentManagementServiceClient, name string) string {
	t.Helper()
	ctx := context.Background()

	resp, err := mgmt.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: &commonv1.Experiment{
			Name:            name,
			OwnerEmail:      "stream-test@example.com",
			Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
			LayerId:         defaultLayerID,
			PrimaryMetricId: "metric-1",
			GuardrailAction: commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY,
			Variants: []*commonv1.Variant{
				{Name: "control", TrafficFraction: 0.5, IsControl: true},
				{Name: "treatment", TrafficFraction: 0.5},
			},
		},
	}))
	require.NoError(t, err)
	id := resp.Msg.GetExperimentId()

	_, err = mgmt.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: id,
	}))
	require.NoError(t, err)

	return id
}

func TestStreamConfigUpdates_Snapshot(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Create and start an experiment so there's something to snapshot.
	expID := createAndStartExperiment(t, mgmt, "stream-snapshot-test")

	// Connect to stream.
	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer stream.Close()

	// Should receive at least our experiment in the snapshot.
	found := false
	for stream.Receive() {
		update := stream.Msg()
		if update.GetExperiment() != nil && update.GetExperiment().GetExperimentId() == expID {
			found = true
			assert.False(t, update.GetIsDeletion())
			assert.Greater(t, update.GetVersion(), int64(0))
			assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, update.GetExperiment().GetState())
			break
		}
	}
	assert.True(t, found, "expected to find experiment %s in snapshot", expID)
}

func TestStreamConfigUpdates_DeltaOnStart(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	// Connect to stream first (before starting experiment).
	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer stream.Close()

	// Drain snapshot (may be empty or have other experiments).
	// We need to consume the snapshot before we can receive deltas.
	// Give it a moment then start our experiment.
	go func() {
		time.Sleep(500 * time.Millisecond)
		createAndStartExperiment(t, mgmt, "stream-delta-test")
	}()

	// Read updates until we find our delta.
	found := false
	for stream.Receive() {
		update := stream.Msg()
		if update.GetExperiment() != nil && update.GetExperiment().GetName() == "stream-delta-test" {
			found = true
			assert.False(t, update.GetIsDeletion())
			assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, update.GetExperiment().GetState())
			break
		}
	}
	assert.True(t, found, "expected to receive delta update for started experiment")
}

func TestStreamConfigUpdates_DeletionOnConclude(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	// Create and start an experiment.
	expID := createAndStartExperiment(t, mgmt, "stream-deletion-test")

	// Connect to stream.
	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer stream.Close()

	// Drain snapshot, then conclude.
	go func() {
		time.Sleep(500 * time.Millisecond)
		_, err := mgmt.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
			ExperimentId: expID,
		}))
		if err != nil {
			t.Logf("conclude failed: %v", err)
		}
	}()

	// Look for the deletion update.
	found := false
	for stream.Receive() {
		update := stream.Msg()
		if update.GetIsDeletion() {
			found = true
			break
		}
	}
	assert.True(t, found, "expected deletion update after conclude")
}
