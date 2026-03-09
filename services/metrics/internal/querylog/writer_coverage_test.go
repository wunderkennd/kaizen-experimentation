package querylog

import (
	"context"
	"sync"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMemWriter_AllEntries_ReturnsCopy(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()
	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SELECT 1", JobType: "daily_metric"})

	entries1 := w.AllEntries()
	entries2 := w.AllEntries()
	require.Len(t, entries1, 1)
	require.Len(t, entries2, 1)

	// Mutating returned slice should not affect internal state.
	entries1[0].ExperimentID = "MODIFIED"
	assert.Equal(t, "exp-001", w.AllEntries()[0].ExperimentID)
}

func TestMemWriter_AllEntries_Empty(t *testing.T) {
	w := NewMemWriter()
	entries := w.AllEntries()
	assert.Empty(t, entries)
}

func TestMemWriter_Log_SetsComputedAt(t *testing.T) {
	w := NewMemWriter()
	err := w.Log(context.Background(), Entry{
		ExperimentID: "exp-001",
		MetricID:     "m1",
		SQLText:      "SELECT 1",
		JobType:      "daily_metric",
	})
	require.NoError(t, err)

	entries := w.AllEntries()
	require.Len(t, entries, 1)
	assert.False(t, entries[0].ComputedAt.IsZero(), "ComputedAt should be set by Log")
}

func TestMemWriter_Log_PreservesAllFields(t *testing.T) {
	w := NewMemWriter()
	err := w.Log(context.Background(), Entry{
		ExperimentID: "exp-001",
		MetricID:     "metric_a",
		SQLText:      "SELECT AVG(x) FROM t",
		RowCount:     42,
		DurationMs:   150,
		JobType:      "hourly_guardrail",
	})
	require.NoError(t, err)

	e := w.AllEntries()[0]
	assert.Equal(t, "exp-001", e.ExperimentID)
	assert.Equal(t, "metric_a", e.MetricID)
	assert.Equal(t, "SELECT AVG(x) FROM t", e.SQLText)
	assert.Equal(t, int64(42), e.RowCount)
	assert.Equal(t, int64(150), e.DurationMs)
	assert.Equal(t, "hourly_guardrail", e.JobType)
}

func TestMemWriter_GetLogs_MultipleExperiments(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	for i := 0; i < 5; i++ {
		_ = w.Log(ctx, Entry{ExperimentID: "exp-A", MetricID: "m1", SQLText: "A", JobType: "daily_metric"})
		_ = w.Log(ctx, Entry{ExperimentID: "exp-B", MetricID: "m1", SQLText: "B", JobType: "daily_metric"})
	}

	logsA, err := w.GetLogs(ctx, "exp-A", "")
	require.NoError(t, err)
	assert.Len(t, logsA, 5)

	logsB, err := w.GetLogs(ctx, "exp-B", "")
	require.NoError(t, err)
	assert.Len(t, logsB, 5)
}

func TestMemWriter_GetLogs_MetricFilterDoesNotMatchOtherExperiment(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	_ = w.Log(ctx, Entry{ExperimentID: "exp-A", MetricID: "shared_metric", SQLText: "A", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-B", MetricID: "shared_metric", SQLText: "B", JobType: "daily_metric"})

	logs, err := w.GetLogs(ctx, "exp-A", "shared_metric")
	require.NoError(t, err)
	assert.Len(t, logs, 1)
	assert.Equal(t, "exp-A", logs[0].ExperimentID)
}

func TestMemWriter_ConcurrentAccess(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()
	var wg sync.WaitGroup
	const goroutines = 50

	wg.Add(goroutines * 2)
	for i := 0; i < goroutines; i++ {
		go func() {
			defer wg.Done()
			_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SELECT 1", JobType: "daily_metric"})
		}()
		go func() {
			defer wg.Done()
			_, _ = w.GetLogs(ctx, "exp-001", "")
			_ = w.AllEntries()
		}()
	}
	wg.Wait()

	assert.Len(t, w.AllEntries(), goroutines)
}

func TestMemWriter_ManyEntries(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	const n = 1000
	for i := 0; i < n; i++ {
		_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SELECT 1", JobType: "daily_metric"})
	}

	entries := w.AllEntries()
	assert.Len(t, entries, n)

	logs, err := w.GetLogs(ctx, "exp-001", "m1")
	require.NoError(t, err)
	assert.Len(t, logs, n)
}

func TestMemWriter_GetLogs_EmptyMetricReturnsAll(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "A", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m2", SQLText: "B", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m3", SQLText: "C", JobType: "daily_metric"})

	logs, err := w.GetLogs(ctx, "exp-001", "")
	require.NoError(t, err)
	assert.Len(t, logs, 3, "empty metric filter should return all entries for experiment")
}
