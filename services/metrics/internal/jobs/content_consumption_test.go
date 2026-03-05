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

func setupContentConsumptionJob(t *testing.T) (*ContentConsumptionJob, *spark.MockExecutor, *querylog.MemWriter) {
	t.Helper()
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(200)
	qlWriter := querylog.NewMemWriter()
	job := NewContentConsumptionJob(cfgStore, renderer, executor, qlWriter)
	return job, executor, qlWriter
}

func TestContentConsumptionJob_Run(t *testing.T) {
	job, executor, qlWriter := setupContentConsumptionJob(t)
	ctx := context.Background()

	result, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", result.ExperimentID)
	assert.Equal(t, int64(200), result.RowsWritten)
	assert.False(t, result.CompletedAt.IsZero())

	// Verify single SQL execution to delta.content_consumption
	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	assert.Equal(t, "delta.content_consumption", calls[0].TargetTable)
	assert.Contains(t, calls[0].SQL, "content_id")
	assert.Contains(t, calls[0].SQL, "watch_time_seconds")
	assert.Contains(t, calls[0].SQL, "unique_viewers")
	assert.Contains(t, calls[0].SQL, "delta.exposures")

	// Verify query log
	entries := qlWriter.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "content_consumption", entries[0].JobType)
	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", entries[0].ExperimentID)
}

func TestContentConsumptionJob_Run_NotFound(t *testing.T) {
	job, _, _ := setupContentConsumptionJob(t)
	ctx := context.Background()
	_, err := job.Run(ctx, "nonexistent")
	assert.Error(t, err)
}

func TestContentConsumptionJob_Run_SQLContainsKeyFields(t *testing.T) {
	job, executor, _ := setupContentConsumptionJob(t)
	ctx := context.Background()

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	calls := executor.GetCalls()
	require.Len(t, calls, 1)
	sql := calls[0].SQL

	assert.True(t, strings.Contains(sql, "GROUP BY content_events.variant_id, content_events.content_id"),
		"Content consumption should GROUP BY variant_id, content_id")
	assert.True(t, strings.Contains(sql, "COUNT(DISTINCT content_events.user_id)"),
		"Content consumption should count unique viewers")
}
