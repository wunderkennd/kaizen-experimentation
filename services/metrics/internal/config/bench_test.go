package config

import (
	"testing"
)

func loadBenchStore(b *testing.B) *ConfigStore {
	b.Helper()
	cs, err := LoadFromFile("testdata/seed_config.json")
	if err != nil {
		b.Fatalf("LoadFromFile: %v", err)
	}
	return cs
}

// BenchmarkLoadFromFile benchmarks cold-loading the config from disk + JSON parse.
func BenchmarkLoadFromFile(b *testing.B) {
	for i := 0; i < b.N; i++ {
		_, err := LoadFromFile("testdata/seed_config.json")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkConfigStore_GetExperiment benchmarks single experiment lookup.
func BenchmarkConfigStore_GetExperiment(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkConfigStore_GetMetric benchmarks single metric lookup.
func BenchmarkConfigStore_GetMetric(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := cs.GetMetric("watch_time_minutes")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkConfigStore_GetMetricsForExperiment benchmarks fetching all metrics
// for an experiment (includes metric resolution).
func BenchmarkConfigStore_GetMetricsForExperiment(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := cs.GetMetricsForExperiment("e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkConfigStore_GetGuardrailsForExperiment benchmarks guardrail lookup.
func BenchmarkConfigStore_GetGuardrailsForExperiment(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := cs.GetGuardrailsForExperiment("e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkConfigStore_RunningExperimentIDs benchmarks scanning all running experiments.
func BenchmarkConfigStore_RunningExperimentIDs(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = cs.RunningExperimentIDs()
	}
}

// BenchmarkConfigStore_GetSurrogateModelForExperiment benchmarks surrogate model
// lookup (traverses experiment → model_id → model).
func BenchmarkConfigStore_GetSurrogateModelForExperiment(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = cs.GetSurrogateModelForExperiment("e0000000-0000-0000-0000-000000000001")
	}
}

// BenchmarkConfigStore_Parallel_GetExperiment benchmarks concurrent read
// throughput under contention on the RWMutex.
func BenchmarkConfigStore_Parallel_GetExperiment(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	b.RunParallel(func(pb *testing.PB) {
		for pb.Next() {
			_, err := cs.GetExperiment("e0000000-0000-0000-0000-000000000001")
			if err != nil {
				b.Fatal(err)
			}
		}
	})
}

// BenchmarkConfigStore_Parallel_GetMetricsForExperiment benchmarks concurrent
// metric resolution (heavier than single lookup).
func BenchmarkConfigStore_Parallel_GetMetricsForExperiment(b *testing.B) {
	cs := loadBenchStore(b)
	b.ResetTimer()
	b.RunParallel(func(pb *testing.PB) {
		for pb.Next() {
			_, err := cs.GetMetricsForExperiment("e0000000-0000-0000-0000-000000000001")
			if err != nil {
				b.Fatal(err)
			}
		}
	})
}
