package jobs

import (
	"context"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

func setupInterleavingJob(t *testing.T) (*InterleavingJob, *spark.MockExecutor, *querylog.MemWriter) {
	t.Helper()
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(300)
	qlWriter := querylog.NewMemWriter()
	job := NewInterleavingJob(cfgStore, renderer, executor, qlWriter)
	return job, executor, qlWriter
}

func TestInterleavingJob_Run(t *testing.T) {
	job, executor, qlWriter := setupInterleavingJob(t)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000003", result.ExperimentID)
	assert.Equal(t, int64(300), result.RowsWritten)
	assert.False(t, result.CompletedAt.IsZero())

	// Verify single SQL execution to delta.interleaving_scores
	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	assert.Equal(t, "delta.interleaving_scores", calls[0].TargetTable)
	assert.Contains(t, calls[0].SQL, "interleaving_provenance")
	assert.Contains(t, calls[0].SQL, "algorithm_scores")
	assert.Contains(t, calls[0].SQL, "winning_algorithm_id")

	// Verify query log
	entries := qlWriter.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "interleaving_score", entries[0].JobType)
	assert.Equal(t, "e0000000-0000-0000-0000-000000000003", entries[0].ExperimentID)
}

func TestInterleavingJob_Run_UsesConfiguredCreditAssignment(t *testing.T) {
	job, executor, _ := setupInterleavingJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)

	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	sql := calls[0].SQL

	// The seed config has credit_assignment: "proportional" for this experiment
	assert.True(t, strings.Contains(sql, "CAST(COUNT(*) AS DOUBLE) AS credit"),
		"Proportional credit should use COUNT(*)")
}

func TestInterleavingJob_Run_NonInterleavingExperiment(t *testing.T) {
	job, executor, _ := setupInterleavingJob(t)
	ctx := context.Background()

	// homepage_recs_v2 is AB, not INTERLEAVING
	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	// Should return 0 rows and no SQL execution
	assert.Equal(t, int64(0), result.RowsWritten)
	assert.Len(t, executor.GetCalls(), 0)
}

func TestInterleavingJob_Run_NotFound(t *testing.T) {
	job, _, _ := setupInterleavingJob(t)
	ctx := context.Background()
	_, err := job.Run(ctx, "nonexistent")
	assert.Error(t, err)
}

func TestInterleavingJob_Run_SQLContainsKeyFields(t *testing.T) {
	job, executor, _ := setupInterleavingJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)

	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	sql := calls[0].SQL

	// Must join with interleaving_provenance from exposures
	assert.Contains(t, sql, "interleaving_provenance")
	// Must use engagement event type from config
	assert.Contains(t, sql, "click")
	// Must produce per-user scores
	assert.Contains(t, sql, "GROUP BY pus.user_id")
	// Must compute per-algorithm credit
	assert.Contains(t, sql, "source_algorithm_id")
	// Must write to correct table
	assert.Equal(t, "delta.interleaving_scores", calls[0].TargetTable)
}

func TestInterleavingJob_Run_AcceptanceCriteria(t *testing.T) {
	// Acceptance criterion: the SQL template correctly attributes engagement
	// to source algorithms via interleaving_provenance, supporting the
	// invariant that Algorithm A items = 100% engagement → win rate = 1.0
	job, executor, _ := setupInterleavingJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)

	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	sql := calls[0].SQL

	// The SQL must read provenance from exposures
	assert.Contains(t, sql, "eu.interleaving_provenance[ee.content_id] AS source_algorithm_id")
	// The SQL must filter to items that have provenance
	assert.Contains(t, sql, "eu.interleaving_provenance[ee.content_id] IS NOT NULL")
	// The SQL must compute a winning algorithm
	assert.Contains(t, sql, "winning_algorithm_id")
	// The SQL must aggregate total engagements
	assert.Contains(t, sql, "total_engagements")
}
