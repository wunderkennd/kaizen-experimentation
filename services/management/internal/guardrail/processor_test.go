//go:build integration

package guardrail_test

import (
	"context"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/management/internal/guardrail"
	"github.com/org/experimentation-platform/services/management/internal/store"
)

func setupProcessor(t *testing.T) (*guardrail.Processor, *pgxpool.Pool) {
	t.Helper()
	ctx := context.Background()
	pool, err := store.NewPool(ctx)
	require.NoError(t, err)
	t.Cleanup(func() {
		done := make(chan struct{})
		go func() {
			pool.Close()
			close(done)
		}()
		select {
		case <-done:
		case <-time.After(5 * time.Second):
			t.Log("pool.Close() timed out after 5s — possible connection leak")
		}
	})

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	proc := guardrail.NewProcessor(es, as, nil)
	return proc, pool
}

// createTestLayerForGuardrail creates an isolated layer for each test to avoid bucket exhaustion.
func createTestLayerForGuardrail(t *testing.T, pool *pgxpool.Pool, name string) string {
	t.Helper()
	var layerID string
	err := pool.QueryRow(context.Background(),
		`INSERT INTO layers (name, description, total_buckets) VALUES ($1, $2, 10000) RETURNING layer_id`,
		name, "guardrail test layer",
	).Scan(&layerID)
	require.NoError(t, err)
	return layerID
}

// createRunningExperiment creates a DRAFT experiment and transitions it to RUNNING.
func createRunningExperiment(t *testing.T, pool *pgxpool.Pool, name, guardrailAction string) string {
	t.Helper()
	ctx := context.Background()

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	ls := store.NewLayerStore(pool)

	// Create a per-test layer to avoid bucket exhaustion across tests.
	layerID := createTestLayerForGuardrail(t, pool, "guardrail-layer-"+name)

	tx, err := es.BeginTx(ctx)
	require.NoError(t, err)

	exp, err := es.Insert(ctx, tx, store.ExperimentRow{
		Name:            name,
		Description:     "guardrail test",
		OwnerEmail:      "test@example.com",
		Type:            "AB",
		State:           "DRAFT",
		LayerID:         layerID,
		PrimaryMetricID: "watch_time_minutes",
		GuardrailAction: guardrailAction,
	})
	require.NoError(t, err)

	err = es.InsertVariants(ctx, tx, []store.VariantRow{
		{ExperimentID: exp.ExperimentID, Name: "control", TrafficFraction: 0.5, IsControl: true},
		{ExperimentID: exp.ExperimentID, Name: "treatment", TrafficFraction: 0.5, IsControl: false},
	})
	require.NoError(t, err)
	require.NoError(t, tx.Commit(ctx))

	// Transition to STARTING.
	tx2, err := es.BeginTx(ctx)
	require.NoError(t, err)
	_, err = es.TransitionState(ctx, tx2, exp.ExperimentID, "DRAFT", "STARTING", "")
	require.NoError(t, err)
	require.NoError(t, as.Insert(ctx, tx2, store.AuditEntry{
		ExperimentID: exp.ExperimentID, Action: "start", ActorEmail: "system",
		PreviousState: "DRAFT", NewState: "STARTING",
	}))
	require.NoError(t, tx2.Commit(ctx))

	// Allocate and transition to RUNNING.
	tx3, err := es.BeginTx(ctx)
	require.NoError(t, err)
	_, err = ls.GetLayerByIDForUpdate(ctx, tx3, layerID)
	require.NoError(t, err)

	_, err = ls.InsertAllocation(ctx, tx3, store.AllocationRow{
		LayerID: layerID, ExperimentID: exp.ExperimentID,
		StartBucket: 0, EndBucket: 4999,
	})
	require.NoError(t, err)

	_, err = es.TransitionState(ctx, tx3, exp.ExperimentID, "STARTING", "RUNNING", "started_at")
	require.NoError(t, err)
	require.NoError(t, tx3.Commit(ctx))

	return exp.ExperimentID
}

func newAlert(experimentID string) guardrail.Alert {
	return guardrail.Alert{
		ExperimentID:           experimentID,
		MetricID:               "error_rate",
		VariantID:              "treatment-variant-id",
		CurrentValue:           0.015,
		Threshold:              0.01,
		ConsecutiveBreachCount: 3,
		DetectedAt:             time.Now().UTC(),
	}
}

func TestProcessAlert_AutoPause(t *testing.T) {
	proc, pool := setupProcessor(t)
	ctx := context.Background()

	expID := createRunningExperiment(t, pool, "auto-pause-test-"+t.Name(), "AUTO_PAUSE")

	result, err := proc.ProcessAlert(ctx, newAlert(expID))
	require.NoError(t, err)
	assert.Equal(t, guardrail.ResultPaused, result)

	// Verify audit trail has guardrail_auto_pause entry.
	var action string
	err = pool.QueryRow(ctx,
		`SELECT action FROM audit_trail WHERE experiment_id = $1 AND action = 'guardrail_auto_pause'`,
		expID).Scan(&action)
	require.NoError(t, err)
	assert.Equal(t, "guardrail_auto_pause", action)
}

func TestProcessAlert_AlertOnly(t *testing.T) {
	proc, pool := setupProcessor(t)
	ctx := context.Background()

	expID := createRunningExperiment(t, pool, "alert-only-test-"+t.Name(), "ALERT_ONLY")

	result, err := proc.ProcessAlert(ctx, newAlert(expID))
	require.NoError(t, err)
	assert.Equal(t, guardrail.ResultAlertOnly, result)

	// Verify audit trail has guardrail_alert (not guardrail_auto_pause).
	var action string
	err = pool.QueryRow(ctx,
		`SELECT action FROM audit_trail WHERE experiment_id = $1 AND action = 'guardrail_alert'`,
		expID).Scan(&action)
	require.NoError(t, err)
	assert.Equal(t, "guardrail_alert", action)

	// Verify no auto_pause entry exists.
	var count int
	err = pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM audit_trail WHERE experiment_id = $1 AND action = 'guardrail_auto_pause'`,
		expID).Scan(&count)
	require.NoError(t, err)
	assert.Equal(t, 0, count)
}

func TestProcessAlert_NotRunning(t *testing.T) {
	proc, pool := setupProcessor(t)
	ctx := context.Background()

	// Create a DRAFT experiment (not started).
	es := store.NewExperimentStore(pool)
	tx, err := es.BeginTx(ctx)
	require.NoError(t, err)
	exp, err := es.Insert(ctx, tx, store.ExperimentRow{
		Name:            "not-running-test-" + t.Name(),
		Description:     "guardrail test",
		OwnerEmail:      "test@example.com",
		Type:            "AB",
		State:           "DRAFT",
		LayerID:         "a0000000-0000-0000-0000-000000000001",
		PrimaryMetricID: "watch_time_minutes",
		GuardrailAction: "AUTO_PAUSE",
	})
	require.NoError(t, err)
	require.NoError(t, tx.Commit(ctx))

	result, err := proc.ProcessAlert(ctx, newAlert(exp.ExperimentID))
	require.NoError(t, err)
	assert.Equal(t, guardrail.ResultSkipped, result)
}

func TestProcessAlert_UnknownExperiment(t *testing.T) {
	proc, _ := setupProcessor(t)
	ctx := context.Background()

	result, err := proc.ProcessAlert(ctx, newAlert("00000000-0000-0000-0000-000000000000"))
	require.NoError(t, err)
	assert.Equal(t, guardrail.ResultSkipped, result)
}

func TestProcessAlert_AuditDetails(t *testing.T) {
	proc, pool := setupProcessor(t)
	ctx := context.Background()

	expID := createRunningExperiment(t, pool, "audit-details-test-"+t.Name(), "AUTO_PAUSE")

	alert := guardrail.Alert{
		ExperimentID:           expID,
		MetricID:               "rebuffer_rate",
		VariantID:              "variant-123",
		CurrentValue:           0.08,
		Threshold:              0.05,
		ConsecutiveBreachCount: 5,
		DetectedAt:             time.Date(2026, 3, 4, 12, 0, 0, 0, time.UTC),
	}

	_, err := proc.ProcessAlert(ctx, alert)
	require.NoError(t, err)

	// Verify the audit details contain the breach info.
	var detailsJSON []byte
	err = pool.QueryRow(ctx,
		`SELECT details_json FROM audit_trail WHERE experiment_id = $1 AND action = 'guardrail_auto_pause'`,
		expID).Scan(&detailsJSON)
	require.NoError(t, err)
	assert.Contains(t, string(detailsJSON), `"rebuffer_rate"`)
	assert.Contains(t, string(detailsJSON), `"variant-123"`)
	assert.Contains(t, string(detailsJSON), `0.08`)
	assert.Contains(t, string(detailsJSON), `0.05`)
}
