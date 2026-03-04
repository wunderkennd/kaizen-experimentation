package querylog

import (
	"context"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMemWriter_Log(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	err := w.Log(ctx, Entry{
		ExperimentID: "exp-001",
		MetricID:     "metric_a",
		SQLText:      "SELECT avg(value) FROM events WHERE experiment_id = 'exp-001'",
		RowCount:     1500,
		DurationMs:   230,
		JobType:      "daily_metric",
	})
	require.NoError(t, err)

	entries := w.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "exp-001", entries[0].ExperimentID)
	assert.Equal(t, "metric_a", entries[0].MetricID)
	assert.Contains(t, entries[0].SQLText, "SELECT avg")
	assert.Equal(t, int64(1500), entries[0].RowCount)
	assert.Equal(t, int64(230), entries[0].DurationMs)
	assert.Equal(t, "daily_metric", entries[0].JobType)
	assert.False(t, entries[0].ComputedAt.IsZero())
}

func TestMemWriter_GetLogs_FilterByExperiment(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SQL1", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-002", MetricID: "m2", SQLText: "SQL2", JobType: "daily_metric"})
	_ = w.Log(ctx, Entry{ExperimentID: "exp-001", MetricID: "m3", SQLText: "SQL3", JobType: "hourly_guardrail"})

	logs, err := w.GetLogs(ctx, "exp-001", "")
	require.NoError(t, err)
	assert.Len(t, logs, 2)
	for _, l := range logs {
		assert.Equal(t, "exp-001", l.ExperimentID)
	}
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

func TestMemWriter_MultipleEntries(t *testing.T) {
	w := NewMemWriter()
	ctx := context.Background()

	for i := 0; i < 10; i++ {
		_ = w.Log(ctx, Entry{
			ExperimentID: "exp-001",
			MetricID:     "metric_a",
			SQLText:      "SELECT 1",
			JobType:      "daily_metric",
		})
	}

	entries := w.AllEntries()
	assert.Len(t, entries, 10)

	logs, err := w.GetLogs(ctx, "exp-001", "metric_a")
	require.NoError(t, err)
	assert.Len(t, logs, 10)
}
