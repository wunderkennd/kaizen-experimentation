package export

import (
	"fmt"
	"strings"
	"testing"

	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
)

func makeEntries(n int) []querylog.Entry {
	entries := make([]querylog.Entry, n)
	for i := range entries {
		entries[i] = querylog.Entry{
			ExperimentID: "e0000000-0000-0000-0000-000000000001",
			MetricID:     fmt.Sprintf("metric_%d", i),
			SQLText:      fmt.Sprintf("SELECT user_id, AVG(value) AS metric_value FROM delta.metric_events WHERE experiment_id = 'e1' AND event_type = 'heartbeat_%d' AND event_date = '2024-01-15' GROUP BY user_id", i),
			RowCount:     int64(500 + i),
			DurationMs:   int64(100 + i*10),
			JobType:      "daily_metric",
		}
	}
	return entries
}

// BenchmarkGenerateNotebook_1Entry benchmarks notebook generation for a single query.
func BenchmarkGenerateNotebook_1Entry(b *testing.B) {
	entries := makeEntries(1)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := GenerateNotebook("e0000000-0000-0000-0000-000000000001", entries)
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkGenerateNotebook_10Entries benchmarks notebook generation for a
// typical experiment computation (10 queries: 4 metrics + CUPED + delta + treatment effects).
func BenchmarkGenerateNotebook_10Entries(b *testing.B) {
	entries := makeEntries(10)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := GenerateNotebook("e0000000-0000-0000-0000-000000000001", entries)
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkGenerateNotebook_50Entries benchmarks notebook generation at scale
// (50 queries: complex experiment with many metric types, guardrails, and extras).
func BenchmarkGenerateNotebook_50Entries(b *testing.B) {
	entries := makeEntries(50)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := GenerateNotebook("e0000000-0000-0000-0000-000000000001", entries)
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkGenerateNotebook_LargeSQL benchmarks notebook generation with large
// SQL payloads (simulates complex custom SQL queries).
func BenchmarkGenerateNotebook_LargeSQL(b *testing.B) {
	entries := make([]querylog.Entry, 10)
	largeSQL := "SELECT " + strings.Repeat("column_name_that_is_fairly_long, ", 100) + "user_id FROM delta.metric_events WHERE experiment_id = 'e1' GROUP BY user_id"
	for i := range entries {
		entries[i] = querylog.Entry{
			ExperimentID: "e0000000-0000-0000-0000-000000000001",
			MetricID:     fmt.Sprintf("metric_%d", i),
			SQLText:      largeSQL,
			RowCount:     500,
			DurationMs:   100,
			JobType:      "daily_metric",
		}
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := GenerateNotebook("e0000000-0000-0000-0000-000000000001", entries)
		if err != nil {
			b.Fatal(err)
		}
	}
}
