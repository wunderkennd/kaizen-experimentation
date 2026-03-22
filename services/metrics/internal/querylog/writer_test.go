package querylog

import (
	"context"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMemWriter_Log(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	err := w.Log(ctx, Entry{
		ExperimentID: "exp-001",
		MetricID:     "metric_a",
		SQLText:      "SELECT 1",
		RowCount:     100,
		DurationMs:   50,
		JobType:      "daily_metric",
	})
	require.NoError(t, err)

	entries := w.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "exp-001", entries[0].ExperimentID)
	assert.Equal(t, "metric_a", entries[0].MetricID)
	assert.Equal(t, "SELECT 1", entries[0].SQLText)
	assert.False(t, entries[0].ComputedAt.IsZero())
}

func TestMemWriter_GetLogs_FilterByExperiment(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SQL1", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-002", MetricID: "m2", SQLText: "SQL2", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m3", SQLText: "SQL3", JobType: "daily_metric"})

	logs, err := w.GetLogs(ctx, "exp-001", "")
	require.NoError(t, err)
	assert.Len(t, logs, 2)
}

func TestMemWriter_GetLogs_FilterByMetric(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SQL1", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m2", SQLText: "SQL2", JobType: "daily_metric"})

	logs, err := w.GetLogs(ctx, "exp-001", "m1")
	require.NoError(t, err)
	assert.Len(t, logs, 1)
	assert.Equal(t, "m1", logs[0].MetricID)
}

func TestMemWriter_GetLogs_NoResults(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	logs, err := w.GetLogs(ctx, "nonexistent", "")
	require.NoError(t, err)
	assert.Empty(t, logs)
}

// ---------------------------------------------------------------------------
// GetLogsFiltered tests
// ---------------------------------------------------------------------------

func seedEntries(t *testing.T, w *MemWriter) {
	t.Helper()
	ctx := context.Background()
	base := time.Date(2024, 1, 1, 0, 0, 0, 0, time.UTC)
	entries := []Entry{
		{ExperimentID: "exp-1", MetricID: "m1", SQLText: "SELECT 1", RowCount: 10, DurationMs: 100, JobType: "daily_metric"},
		{ExperimentID: "exp-1", MetricID: "m2", SQLText: "SELECT 2", RowCount: 20, DurationMs: 200, JobType: "hourly_guardrail"},
		{ExperimentID: "exp-1", MetricID: "m1", SQLText: "SELECT 3", RowCount: 30, DurationMs: 300, JobType: "daily_metric"},
		{ExperimentID: "exp-2", MetricID: "m1", SQLText: "SELECT 4", RowCount: 40, DurationMs: 400, JobType: "daily_metric"},
		{ExperimentID: "exp-1", MetricID: "m3", SQLText: "SELECT 5", RowCount: 50, DurationMs: 500, JobType: "cuped_covariate"},
	}
	for i, e := range entries {
		require.NoError(t, w.Log(ctx, e))
		// Override ComputedAt for deterministic ordering.
		w.mu.Lock()
		w.entries[i].ComputedAt = base.Add(time.Duration(i) * time.Hour)
		w.mu.Unlock()
	}
}

func TestLogFilter_ByExperimentID(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	entries, _, err := w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "exp-1"})
	require.NoError(t, err)
	assert.Len(t, entries, 4, "exp-1 has 4 entries")

	entries, _, err = w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "exp-2"})
	require.NoError(t, err)
	assert.Len(t, entries, 1, "exp-2 has 1 entry")
}

func TestLogFilter_ByMetricID(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	entries, _, err := w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "exp-1", MetricID: "m1"})
	require.NoError(t, err)
	assert.Len(t, entries, 2, "exp-1/m1 has 2 entries")
}

func TestLogFilter_ByJobType(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	entries, _, err := w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "exp-1", JobType: "daily_metric"})
	require.NoError(t, err)
	assert.Len(t, entries, 2, "exp-1 daily_metric has 2 entries")

	entries, _, err = w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "exp-1", JobType: "hourly_guardrail"})
	require.NoError(t, err)
	assert.Len(t, entries, 1, "exp-1 hourly_guardrail has 1 entry")
}

func TestLogFilter_ByDateRange(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()
	base := time.Date(2024, 1, 1, 0, 0, 0, 0, time.UTC)

	// After 1.5 hours: should include entries at 2h, 4h (exp-1 only)
	entries, _, err := w.GetLogsFiltered(ctx, LogFilter{
		ExperimentID: "exp-1",
		After:        base.Add(90 * time.Minute),
	})
	require.NoError(t, err)
	assert.Len(t, entries, 2, "exp-1 after 1.5h has 2 entries (2h, 4h)")

	// Before 2.5 hours: should include entries at 0h, 1h, 2h (exp-1 only)
	entries, _, err = w.GetLogsFiltered(ctx, LogFilter{
		ExperimentID: "exp-1",
		Before:       base.Add(150 * time.Minute),
	})
	require.NoError(t, err)
	assert.Len(t, entries, 3, "exp-1 before 2.5h has 3 entries (0h, 1h, 2h)")

	// Combined range: 0.5h to 3.5h — entries at 1h, 2h (exp-1 only; 3h is exp-2)
	entries, _, err = w.GetLogsFiltered(ctx, LogFilter{
		ExperimentID: "exp-1",
		After:        base.Add(30 * time.Minute),
		Before:       base.Add(210 * time.Minute),
	})
	require.NoError(t, err)
	assert.Len(t, entries, 2, "exp-1 between 0.5h and 3.5h has 2 entries (1h, 2h)")
}

func TestLogFilter_Pagination(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	// Page 1: size 2
	entries, nextToken, err := w.GetLogsFiltered(ctx, LogFilter{
		ExperimentID: "exp-1",
		PageSize:     2,
	})
	require.NoError(t, err)
	assert.Len(t, entries, 2)
	assert.NotEmpty(t, nextToken, "should have next page token")

	// Page 2: use token
	entries2, nextToken2, err := w.GetLogsFiltered(ctx, LogFilter{
		ExperimentID: "exp-1",
		PageSize:     2,
		PageToken:    nextToken,
	})
	require.NoError(t, err)
	assert.Len(t, entries2, 2)
	assert.Empty(t, nextToken2, "last page should not have next token")

	// Entries should not overlap
	for _, e1 := range entries {
		for _, e2 := range entries2 {
			assert.NotEqual(t, e1.SQLText, e2.SQLText, "pages should not overlap")
		}
	}
}

func TestLogFilter_DefaultPageSize(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	entries, nextToken, err := w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "exp-1"})
	require.NoError(t, err)
	assert.Len(t, entries, 4)
	assert.Empty(t, nextToken)
}

func TestLogFilter_SortOrder(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	entries, _, err := w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "exp-1"})
	require.NoError(t, err)
	require.Len(t, entries, 4)

	for i := 1; i < len(entries); i++ {
		assert.True(t, entries[i-1].ComputedAt.After(entries[i].ComputedAt) || entries[i-1].ComputedAt.Equal(entries[i].ComputedAt),
			"entries should be sorted by computed_at desc")
	}
}

func TestLogFilter_CombinedFilters(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	entries, _, err := w.GetLogsFiltered(ctx, LogFilter{
		ExperimentID: "exp-1",
		MetricID:     "m1",
		JobType:      "daily_metric",
		PageSize:     10,
	})
	require.NoError(t, err)
	assert.Len(t, entries, 2, "exp-1/m1/daily_metric has 2 entries")
}

func TestLogFilter_NoResults(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	entries, nextToken, err := w.GetLogsFiltered(ctx, LogFilter{ExperimentID: "nonexistent"})
	require.NoError(t, err)
	assert.Empty(t, entries)
	assert.Empty(t, nextToken)
}

// ---------------------------------------------------------------------------
// PurgeOldLogs tests
// ---------------------------------------------------------------------------

func TestPurge_RemovesOldEntries(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()
	base := time.Date(2024, 1, 1, 0, 0, 0, 0, time.UTC)

	purged, err := w.PurgeOldLogs(ctx, base.Add(150*time.Minute))
	require.NoError(t, err)
	assert.Equal(t, int64(3), purged, "should purge 3 entries (0h, 1h, 2h)")

	remaining := w.AllEntries()
	assert.Len(t, remaining, 2, "2 entries should remain (3h, 4h)")
}

func TestPurge_NothingToPurge(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	purged, err := w.PurgeOldLogs(ctx, time.Date(2023, 1, 1, 0, 0, 0, 0, time.UTC))
	require.NoError(t, err)
	assert.Equal(t, int64(0), purged)
	assert.Len(t, w.AllEntries(), 5)
}

func TestPurge_AllEntries(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	purged, err := w.PurgeOldLogs(ctx, time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC))
	require.NoError(t, err)
	assert.Equal(t, int64(5), purged)
	assert.Empty(t, w.AllEntries())
}

func TestLogFilter_MaxPageSize(t *testing.T) {
	w := NewMemWriter()
	seedEntries(t, w)
	ctx := context.Background()

	// PageSize > 1000 should be clamped to 1000
	entries, _, err := w.GetLogsFiltered(ctx, LogFilter{
		ExperimentID: "exp-1",
		PageSize:     5000,
	})
	require.NoError(t, err)
	assert.Len(t, entries, 4, "all 4 entries returned, clamped page size of 1000 is still larger")
}
