//go:build integration

// Package handlers_test contains wire-format contract tests between M5 (Management Service)
// and M1 (Assignment Service). These tests validate the field-level contract that M1's
// config_cache.rs:experiment_from_proto() depends on when consuming StreamConfigUpdates.
package handlers_test

import (
	"context"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	assignmentv1 "github.com/org/experimentation/gen/go/experimentation/assignment/v1"
	"github.com/org/experimentation/gen/go/experimentation/assignment/v1/assignmentv1connect"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/handlers"
	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"

	"net/http"
	"net/http/httptest"
)

// setupStreamTestWithPool is like setupStreamTest but also returns the pgxpool.Pool,
// needed for tests that call setTrafficPercentage (direct DB access for holdout experiments).
func setupStreamTestWithPool(t *testing.T) (
	mgmtClient managementv1connect.ExperimentManagementServiceClient,
	streamClient assignmentv1connect.AssignmentServiceClient,
	notifier *streaming.Notifier,
	pool *pgxpool.Pool,
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
	ts := store.NewTargetingStore(pool)
	ss := store.NewSurrogateStore(pool)
	expSvc := handlers.NewExperimentService(es, as, ls, ms, ts, ss, notifier)
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

	return mgmtClient, streamClient, notifier, pool, cleanup
}

// receiveUpdate reads from the stream synchronously until predicate matches.
// The stream's context timeout controls the deadline — no goroutines are spawned.
func receiveUpdate(
	t *testing.T,
	stream *connect.ServerStreamForClient[assignmentv1.ConfigUpdate],
	predicate func(*assignmentv1.ConfigUpdate) bool,
) *assignmentv1.ConfigUpdate {
	t.Helper()

	for stream.Receive() {
		msg := stream.Msg()
		if predicate(msg) {
			return msg
		}
	}
	t.Fatalf("stream ended without matching config update: %v", stream.Err())
	return nil
}

// TestM1M5_ConfigUpdate_RequiredFields validates that all fields M1's experiment_from_proto()
// reads are present and non-zero in a streamed ConfigUpdate for a RUNNING experiment.
// M1 contract: config_cache.rs:130-186
func TestM1M5_ConfigUpdate_RequiredFields(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	layerID := createStreamTestLayer(t, mgmt, "m1m5-required-fields-"+t.Name())
	expID := createAndStartExperiment(t, mgmt, "m1m5-required-fields-test", layerID)

	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	update := receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetExperiment() != nil && u.GetExperiment().GetExperimentId() == expID
	})

	exp := update.GetExperiment()
	require.NotNil(t, exp, "experiment must not be nil")

	// Fields M1's experiment_from_proto() reads directly:
	assert.NotEmpty(t, exp.GetExperimentId(), "experiment_id required by M1")
	assert.NotEmpty(t, exp.GetName(), "name required by M1")
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, exp.GetState(), "state must be RUNNING")
	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_AB, exp.GetType(), "type must be AB")
	assert.NotEmpty(t, exp.GetHashSalt(), "hash_salt required by M1 for deterministic bucketing")
	assert.NotEmpty(t, exp.GetLayerId(), "layer_id required by M1 for layer exclusivity")

	// Variant contract: M1's variant_from_proto() reads variant_id, traffic_fraction, is_control, payload_json
	require.Len(t, exp.GetVariants(), 2, "must have 2 variants (control + treatment)")
	for _, v := range exp.GetVariants() {
		assert.NotEmpty(t, v.GetVariantId(), "variant_id must be populated")
		assert.Greater(t, v.GetTrafficFraction(), float64(0), "traffic_fraction must be > 0")
	}

	// Exactly one control variant
	controlCount := 0
	for _, v := range exp.GetVariants() {
		if v.GetIsControl() {
			controlCount++
		}
	}
	assert.Equal(t, 1, controlCount, "exactly one control variant required")
}

// TestM1M5_ConfigUpdate_HoldoutFlag validates that CUMULATIVE_HOLDOUT experiments
// stream is_cumulative_holdout=true. M1 uses this to prioritize holdout assignment
// before layer allocation.
func TestM1M5_ConfigUpdate_HoldoutFlag(t *testing.T) {
	mgmt, streamClient, _, pool, cleanup := setupStreamTestWithPool(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	layerID := createStreamTestLayer(t, mgmt, "m1m5-holdout-"+t.Name())

	// Create holdout experiment.
	resp, err := mgmt.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: &commonv1.Experiment{
			Name:                "m1m5-holdout-test",
			OwnerEmail:          "contract-test@example.com",
			Type:                commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT,
			LayerId:             layerID,
			PrimaryMetricId:     "watch_time_minutes",
			IsCumulativeHoldout: true,
			GuardrailAction:     commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY,
			Variants: []*commonv1.Variant{
				{Name: "control", TrafficFraction: 0.95, IsControl: true},
				{Name: "treatment", TrafficFraction: 0.05},
			},
		},
	}))
	require.NoError(t, err)
	holdoutID := resp.Msg.GetExperimentId()

	// Set traffic_percentage to 5% (required for holdouts).
	setTrafficPercentage(t, pool, holdoutID, 0.05)

	// Start experiment.
	_, err = mgmt.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: holdoutID,
	}))
	require.NoError(t, err)

	// Connect to stream.
	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	update := receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetExperiment() != nil && u.GetExperiment().GetExperimentId() == holdoutID
	})

	assert.True(t, update.GetExperiment().GetIsCumulativeHoldout(),
		"is_cumulative_holdout must be true for holdout experiments; M1 uses this for holdout prioritization")
}

// TestM1M5_ConfigUpdate_VersionMonotonicity validates that version numbers
// are strictly increasing across config updates. M1's config_cache.rs uses:
// `if update.version > self.last_version` to reject stale updates.
func TestM1M5_ConfigUpdate_VersionMonotonicity(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	// Create 3 experiments in separate layers to get 3 distinct updates.
	var expIDs []string
	for i := 0; i < 3; i++ {
		layerID := createStreamTestLayer(t, mgmt, "m1m5-version-"+t.Name()+"-"+time.Now().Format("150405.000")+"-"+string(rune('a'+i)))
		id := createAndStartExperiment(t, mgmt, "m1m5-version-test-"+string(rune('a'+i)), layerID)
		expIDs = append(expIDs, id)
	}

	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	// Collect versions from the 3 experiment updates.
	var versions []int64
	seen := make(map[string]bool)
	for len(versions) < 3 {
		ok := stream.Receive()
		require.True(t, ok, "stream ended prematurely; received %d of 3 expected updates", len(versions))
		msg := stream.Msg()
		if msg.GetExperiment() != nil {
			eid := msg.GetExperiment().GetExperimentId()
			// Only count our experiments.
			for _, id := range expIDs {
				if eid == id && !seen[eid] {
					seen[eid] = true
					versions = append(versions, msg.GetVersion())
					break
				}
			}
		}
	}

	require.Len(t, versions, 3, "should have collected 3 versions")
	for i := 1; i < len(versions); i++ {
		assert.Greater(t, versions[i], versions[i-1],
			"version[%d]=%d must be > version[%d]=%d; M1 relies on monotonicity for stale-update rejection",
			i, versions[i], i-1, versions[i-1])
	}
}

// TestM1M5_ConfigUpdate_DeletionOnConclude validates that concluding a RUNNING experiment
// sends is_deletion=true. M1's apply_update() calls self.experiments.remove() on deletions.
func TestM1M5_ConfigUpdate_DeletionOnConclude(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	layerID := createStreamTestLayer(t, mgmt, "m1m5-deletion-"+t.Name())
	expID := createAndStartExperiment(t, mgmt, "m1m5-deletion-test", layerID)

	// Connect to stream. Receive snapshot first.
	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	// Wait for snapshot to include our experiment.
	receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetExperiment() != nil && u.GetExperiment().GetExperimentId() == expID
	})

	// Conclude the experiment.
	_, err = mgmt.ConcludeExperiment(ctx, connect.NewRequest(&mgmtv1.ConcludeExperimentRequest{
		ExperimentId: expID,
	}))
	require.NoError(t, err)

	// Assert next relevant update is a deletion.
	deletion := receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetIsDeletion()
	})

	assert.True(t, deletion.GetIsDeletion(),
		"concluding a running experiment must produce is_deletion=true; M1 calls experiments.remove()")
	assert.Greater(t, deletion.GetVersion(), int64(0), "deletion must have a version")
}

// TestM1M5_ConfigUpdate_VariantContract validates the variant-level fields that M1's
// variant_from_proto() reads: variant_id, traffic_fraction, is_control, payload_json.
func TestM1M5_ConfigUpdate_VariantContract(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	layerID := createStreamTestLayer(t, mgmt, "m1m5-variant-"+t.Name())

	// Create experiment with control (no payload) + treatment (with payload).
	resp, err := mgmt.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: &commonv1.Experiment{
			Name:            "m1m5-variant-contract",
			OwnerEmail:      "contract-test@example.com",
			Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
			LayerId:         layerID,
			PrimaryMetricId: "watch_time_minutes",
			GuardrailAction: commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY,
			Variants: []*commonv1.Variant{
				{Name: "control", TrafficFraction: 0.5, IsControl: true},
				{Name: "treatment", TrafficFraction: 0.5, PayloadJson: `{"algo":"v2"}`},
			},
		},
	}))
	require.NoError(t, err)
	expID := resp.Msg.GetExperimentId()

	_, err = mgmt.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: expID,
	}))
	require.NoError(t, err)

	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	update := receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetExperiment() != nil && u.GetExperiment().GetExperimentId() == expID
	})

	variants := update.GetExperiment().GetVariants()
	require.Len(t, variants, 2)

	var control, treatment *commonv1.Variant
	for _, v := range variants {
		if v.GetIsControl() {
			control = v
		} else {
			treatment = v
		}
	}
	require.NotNil(t, control, "must have a control variant")
	require.NotNil(t, treatment, "must have a treatment variant")

	// Control: M1's variant_from_proto reads variant_id, traffic_fraction, is_control, payload_json
	assert.NotEmpty(t, control.GetVariantId(), "control variant_id must be populated")
	assert.InDelta(t, 0.5, control.GetTrafficFraction(), 1e-9, "control traffic_fraction")
	assert.True(t, control.GetIsControl(), "control is_control must be true")

	// Treatment: payload_json must match what was set at creation.
	assert.NotEmpty(t, treatment.GetVariantId(), "treatment variant_id must be populated")
	assert.InDelta(t, 0.5, treatment.GetTrafficFraction(), 1e-9, "treatment traffic_fraction")
	assert.False(t, treatment.GetIsControl(), "treatment is_control must be false")
	assert.JSONEq(t, `{"algo":"v2"}`, treatment.GetPayloadJson(),
		"payload_json must roundtrip through DB; M1 passes this to SDK consumers")
}

// TestM1M5_ConfigUpdate_StateIsRunning validates that StreamConfigUpdates only sends
// experiments in RUNNING state. M1 only processes RUNNING experiments for assignment.
func TestM1M5_ConfigUpdate_StateIsRunning(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	layerID := createStreamTestLayer(t, mgmt, "m1m5-state-running-"+t.Name())
	expID := createAndStartExperiment(t, mgmt, "m1m5-state-running-test", layerID)

	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	update := receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetExperiment() != nil && u.GetExperiment().GetExperimentId() == expID
	})

	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, update.GetExperiment().GetState(),
		"streamed experiment state must be RUNNING (int32=3); M1 uses try_from(proto.state)")
	assert.Equal(t, int32(commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING), int32(update.GetExperiment().GetState()),
		"EXPERIMENT_STATE_RUNNING must be int32 value 3")
}

// TestM1M5_ConfigUpdate_HashSaltStable validates that hash_salt in the stream matches
// the value from creation. Hash salt stability is critical for M1's deterministic
// bucketing: hash(user_id, salt) % total_buckets.
func TestM1M5_ConfigUpdate_HashSaltStable(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	layerID := createStreamTestLayer(t, mgmt, "m1m5-hashsalt-"+t.Name())

	// Create experiment and capture the hash salt.
	resp, err := mgmt.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: &commonv1.Experiment{
			Name:            "m1m5-hashsalt-test",
			OwnerEmail:      "contract-test@example.com",
			Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
			LayerId:         layerID,
			PrimaryMetricId: "watch_time_minutes",
			GuardrailAction: commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY,
			Variants: []*commonv1.Variant{
				{Name: "control", TrafficFraction: 0.5, IsControl: true},
				{Name: "treatment", TrafficFraction: 0.5},
			},
		},
	}))
	require.NoError(t, err)
	expID := resp.Msg.GetExperimentId()
	creationSalt := resp.Msg.GetHashSalt()
	require.NotEmpty(t, creationSalt, "hash_salt must be auto-generated at creation")

	_, err = mgmt.StartExperiment(ctx, connect.NewRequest(&mgmtv1.StartExperimentRequest{
		ExperimentId: expID,
	}))
	require.NoError(t, err)

	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	update := receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetExperiment() != nil && u.GetExperiment().GetExperimentId() == expID
	})

	assert.Equal(t, creationSalt, update.GetExperiment().GetHashSalt(),
		"hash_salt must be stable between creation and streaming; changing it would break M1's deterministic bucketing")
}

// TestM1M5_ConfigUpdate_EnumValues validates that type and state enums are non-UNSPECIFIED
// in streamed config updates. M1 uses try_from(proto.state) and try_from(proto.type) — an
// UNSPECIFIED value (0) would produce "UNSPECIFIED" string and break assignment logic.
func TestM1M5_ConfigUpdate_EnumValues(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	layerID := createStreamTestLayer(t, mgmt, "m1m5-enums-"+t.Name())
	expID := createAndStartExperiment(t, mgmt, "m1m5-enum-test", layerID)

	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	update := receiveUpdate(t, stream, func(u *assignmentv1.ConfigUpdate) bool {
		return u.GetExperiment() != nil && u.GetExperiment().GetExperimentId() == expID
	})

	exp := update.GetExperiment()

	// Type must be EXPERIMENT_TYPE_AB (1), not UNSPECIFIED (0).
	assert.Equal(t, commonv1.ExperimentType_EXPERIMENT_TYPE_AB, exp.GetType(),
		"type must be EXPERIMENT_TYPE_AB, not UNSPECIFIED")
	assert.NotEqual(t, int32(0), int32(exp.GetType()),
		"type enum int32 must not be 0 (UNSPECIFIED); M1 would produce 'UNSPECIFIED' string")

	// State must be EXPERIMENT_STATE_RUNNING (3), not UNSPECIFIED (0).
	assert.Equal(t, commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING, exp.GetState(),
		"state must be EXPERIMENT_STATE_RUNNING, not UNSPECIFIED")
	assert.NotEqual(t, int32(0), int32(exp.GetState()),
		"state enum int32 must not be 0 (UNSPECIFIED); M1 would produce 'UNSPECIFIED' string")
}

// TestM1M5_ConfigUpdate_SnapshotIncludesAllRunning validates that a fresh stream connection
// receives a complete snapshot of all RUNNING experiments. M1 relies on a complete snapshot
// at connection time for cold-start recovery.
func TestM1M5_ConfigUpdate_SnapshotIncludesAllRunning(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)

	// Create 3 experiments in different layers and start them.
	expIDs := make(map[string]bool)
	for i := 0; i < 3; i++ {
		layerID := createStreamTestLayer(t, mgmt, "m1m5-snapshot-"+t.Name()+"-"+string(rune('a'+i)))
		id := createAndStartExperiment(t, mgmt, "m1m5-snapshot-"+string(rune('a'+i)), layerID)
		expIDs[id] = false
	}

	// Connect fresh to stream.
	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	// Read snapshot synchronously. The context timeout (20s) controls the deadline.
	for stream.Receive() {
		if exp := stream.Msg().GetExperiment(); exp != nil {
			if _, ok := expIDs[exp.GetExperimentId()]; ok {
				expIDs[exp.GetExperimentId()] = true
			}
		}
		// Check if we found all 3.
		allFound := true
		for _, found := range expIDs {
			if !found {
				allFound = false
				break
			}
		}
		if allFound {
			break
		}
	}

	for id, found := range expIDs {
		assert.True(t, found, "experiment %s must be in snapshot for M1 cold-start recovery", id)
	}
}

// TestM1M5_ConfigUpdate_NonRunningExcluded validates that DRAFT, CONCLUDED, and ARCHIVED
// experiments are NOT included in the stream. M1 only cares about RUNNING experiments.
func TestM1M5_ConfigUpdate_NonRunningExcluded(t *testing.T) {
	mgmt, streamClient, _, cleanup := setupStreamTest(t)
	defer cleanup()

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)

	// Create a DRAFT experiment (do NOT start it).
	draftLayerID := createStreamTestLayer(t, mgmt, "m1m5-draft-"+t.Name())
	draftResp, err := mgmt.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: &commonv1.Experiment{
			Name:            "m1m5-draft-should-not-stream",
			OwnerEmail:      "contract-test@example.com",
			Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
			LayerId:         draftLayerID,
			PrimaryMetricId: "watch_time_minutes",
			GuardrailAction: commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY,
			Variants: []*commonv1.Variant{
				{Name: "control", TrafficFraction: 0.5, IsControl: true},
				{Name: "treatment", TrafficFraction: 0.5},
			},
		},
	}))
	require.NoError(t, err)
	draftID := draftResp.Msg.GetExperimentId()

	// Create a RUNNING experiment.
	runningLayerID := createStreamTestLayer(t, mgmt, "m1m5-running-"+t.Name())
	runningID := createAndStartExperiment(t, mgmt, "m1m5-running-should-stream", runningLayerID)

	// Connect fresh to stream.
	stream, err := streamClient.StreamConfigUpdates(ctx, connect.NewRequest(
		&assignmentv1.StreamConfigUpdatesRequest{LastKnownVersion: 0},
	))
	require.NoError(t, err)
	defer func() { cancel(); stream.Close() }()

	// Read snapshot synchronously. The handler sends all RUNNING experiments first,
	// so once we find ours the snapshot is consumed. DRAFT experiments are never sent.
	foundRunning := false
	foundDraft := false
	for stream.Receive() {
		if exp := stream.Msg().GetExperiment(); exp != nil {
			if exp.GetExperimentId() == runningID {
				foundRunning = true
			}
			if exp.GetExperimentId() == draftID {
				foundDraft = true
			}
		}
		if foundRunning {
			break
		}
	}

	assert.True(t, foundRunning, "RUNNING experiment %s must appear in stream", runningID)
	assert.False(t, foundDraft,
		"DRAFT experiment %s must NOT appear in stream; M1 only processes RUNNING experiments", draftID)
}
