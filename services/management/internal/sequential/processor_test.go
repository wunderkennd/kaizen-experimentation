//go:build integration

package sequential_test

import (
	"context"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/management/internal/sequential"
	"github.com/org/experimentation-platform/services/management/internal/store"
)

// mockConcluder tracks ConcludeByID calls for testing.
type mockConcluder struct {
	calls []concludeCall
	err   error
}

type concludeCall struct {
	ID           string
	Actor        string
	ExtraDetails map[string]any
}

func (m *mockConcluder) ConcludeByID(_ context.Context, id, actor string, extraDetails map[string]any) error {
	m.calls = append(m.calls, concludeCall{ID: id, Actor: actor, ExtraDetails: extraDetails})
	return m.err
}

func newPool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	pool, err := store.NewPool(context.Background())
	require.NoError(t, err)
	t.Cleanup(pool.Close)
	return pool
}

// createSequentialExperiment creates a RUNNING experiment with sequential_method set.
func createSequentialExperiment(t *testing.T, pool *pgxpool.Pool, name, seqMethod string) string {
	t.Helper()
	ctx := context.Background()

	var id string
	err := pool.QueryRow(ctx, `
		INSERT INTO experiments (
			name, owner_email, type, layer_id, primary_metric_id,
			state, sequential_method, started_at
		) VALUES ($1, 'test@example.com', 'AB', 'a0000000-0000-0000-0000-000000000001',
			'watch_time_minutes', 'RUNNING', $2, NOW())
		RETURNING experiment_id`,
		name, seqMethod,
	).Scan(&id)
	require.NoError(t, err)

	// Insert required variants.
	_, err = pool.Exec(ctx, `
		INSERT INTO variants (experiment_id, name, traffic_fraction, is_control)
		VALUES ($1, 'control', 0.5, true), ($1, 'treatment', 0.5, false)`, id)
	require.NoError(t, err)

	return id
}

// createNonSequentialExperiment creates a RUNNING experiment without sequential_method.
func createNonSequentialExperiment(t *testing.T, pool *pgxpool.Pool, name string) string {
	t.Helper()
	ctx := context.Background()

	var id string
	err := pool.QueryRow(ctx, `
		INSERT INTO experiments (
			name, owner_email, type, layer_id, primary_metric_id,
			state, started_at
		) VALUES ($1, 'test@example.com', 'AB', 'a0000000-0000-0000-0000-000000000001',
			'watch_time_minutes', 'RUNNING', NOW())
		RETURNING experiment_id`, name,
	).Scan(&id)
	require.NoError(t, err)

	_, err = pool.Exec(ctx, `
		INSERT INTO variants (experiment_id, name, traffic_fraction, is_control)
		VALUES ($1, 'control', 0.5, true), ($1, 'treatment', 0.5, false)`, id)
	require.NoError(t, err)

	return id
}

func TestProcessAlert_SequentialBoundaryCrossed(t *testing.T) {
	pool := newPool(t)
	ctx := context.Background()

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	concluder := &mockConcluder{}
	proc := sequential.NewProcessor(es, as, nil, concluder)

	expID := createSequentialExperiment(t, pool, "seq-boundary-test", "MSPRT")

	alert := sequential.BoundaryAlert{
		ExperimentID:   expID,
		MetricID:       "watch_time_minutes",
		CurrentLook:    3,
		AlphaSpent:     0.04,
		AlphaRemaining: 0.01,
		AdjustedPValue: 0.003,
		DetectedAt:     time.Now(),
	}

	result, err := proc.ProcessAlert(ctx, alert)
	require.NoError(t, err)
	assert.Equal(t, sequential.ResultConcluded, result)

	// Verify concluder was called.
	require.Len(t, concluder.calls, 1)
	assert.Equal(t, expID, concluder.calls[0].ID)
	assert.Equal(t, "sequential_auto_conclude", concluder.calls[0].Actor)
	assert.Equal(t, "sequential_boundary_crossed", concluder.calls[0].ExtraDetails["trigger"])

	// Verify audit trail has a "sequential_boundary_crossed" entry.
	var action string
	err = pool.QueryRow(ctx,
		`SELECT action FROM audit_trail WHERE experiment_id = $1 AND action = 'sequential_boundary_crossed'`, expID,
	).Scan(&action)
	require.NoError(t, err)
	assert.Equal(t, "sequential_boundary_crossed", action)
}

func TestProcessAlert_NonSequentialExperiment(t *testing.T) {
	pool := newPool(t)
	ctx := context.Background()

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	concluder := &mockConcluder{}
	proc := sequential.NewProcessor(es, as, nil, concluder)

	expID := createNonSequentialExperiment(t, pool, "non-seq-test")

	alert := sequential.BoundaryAlert{
		ExperimentID: expID,
		MetricID:     "watch_time_minutes",
		CurrentLook:  1,
		DetectedAt:   time.Now(),
	}

	result, err := proc.ProcessAlert(ctx, alert)
	require.NoError(t, err)
	assert.Equal(t, sequential.ResultSkipped, result)
	assert.Len(t, concluder.calls, 0, "concluder should not have been called")
}

func TestProcessAlert_ExperimentNotFound(t *testing.T) {
	pool := newPool(t)
	ctx := context.Background()

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	concluder := &mockConcluder{}
	proc := sequential.NewProcessor(es, as, nil, concluder)

	alert := sequential.BoundaryAlert{
		ExperimentID: "00000000-0000-0000-0000-000000000000",
		MetricID:     "watch_time_minutes",
		DetectedAt:   time.Now(),
	}

	result, err := proc.ProcessAlert(ctx, alert)
	require.NoError(t, err)
	assert.Equal(t, sequential.ResultSkipped, result)
}

func TestProcessAlert_ExperimentNotRunning(t *testing.T) {
	pool := newPool(t)
	ctx := context.Background()

	// Create a DRAFT experiment with sequential method.
	var id string
	err := pool.QueryRow(ctx, `
		INSERT INTO experiments (
			name, owner_email, type, layer_id, primary_metric_id,
			state, sequential_method
		) VALUES ('seq-draft-test', 'test@example.com', 'AB',
			'a0000000-0000-0000-0000-000000000001', 'watch_time_minutes',
			'DRAFT', 'MSPRT')
		RETURNING experiment_id`,
	).Scan(&id)
	require.NoError(t, err)

	_, err = pool.Exec(ctx, `
		INSERT INTO variants (experiment_id, name, traffic_fraction, is_control)
		VALUES ($1, 'control', 0.5, true), ($1, 'treatment', 0.5, false)`, id)
	require.NoError(t, err)

	es := store.NewExperimentStore(pool)
	as := store.NewAuditStore(pool)
	concluder := &mockConcluder{}
	proc := sequential.NewProcessor(es, as, nil, concluder)

	alert := sequential.BoundaryAlert{
		ExperimentID: id,
		MetricID:     "watch_time_minutes",
		DetectedAt:   time.Now(),
	}

	result, err := proc.ProcessAlert(ctx, alert)
	require.NoError(t, err)
	assert.Equal(t, sequential.ResultSkipped, result)
}
