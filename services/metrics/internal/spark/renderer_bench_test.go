package spark

import (
	"testing"
)

// rendererForBench creates a shared renderer instance for benchmarks.
// The renderer is safe for concurrent use (read-only template execution).
func rendererForBench(b *testing.B) *SQLRenderer {
	b.Helper()
	r, err := NewSQLRenderer()
	if err != nil {
		b.Fatalf("NewSQLRenderer: %v", err)
	}
	return r
}

func standardParams() TemplateParams {
	return TemplateParams{
		ExperimentID:    "e0000000-0000-0000-0000-000000000001",
		MetricID:        "watch_time_minutes",
		SourceEventType: "heartbeat",
		ComputationDate: "2024-01-15",
	}
}

func BenchmarkSQLRenderer_NewSQLRenderer(b *testing.B) {
	for i := 0; i < b.N; i++ {
		_, err := NewSQLRenderer()
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderMean(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderMean(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderProportion(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	p.MetricID = "ctr_recommendation"
	p.SourceEventType = "impression"
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderProportion(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderCount(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderCount(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderRatio(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:         "e0000000-0000-0000-0000-000000000001",
		MetricID:             "rebuffer_rate",
		NumeratorEventType:   "rebuffer_event",
		DenominatorEventType: "playback_minute",
		ComputationDate:      "2024-01-15",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderRatio(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderRatioDeltaMethod(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:         "e0000000-0000-0000-0000-000000000001",
		MetricID:             "rebuffer_rate",
		NumeratorEventType:   "rebuffer_event",
		DenominatorEventType: "playback_minute",
		ComputationDate:      "2024-01-15",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderRatioDeltaMethod(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderCupedCovariate(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	p.CupedEnabled = true
	p.CupedCovariateEventType = "heartbeat"
	p.ExperimentStartDate = "2024-01-08"
	p.CupedLookbackDays = 7
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderCupedCovariate(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderGuardrailMetric(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:    "e0000000-0000-0000-0000-000000000001",
		MetricID:        "error_rate",
		SourceEventType: "playback_error",
		ComputationDate: "2024-01-15",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderGuardrailMetric(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderQoEMetric(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:    "e0000000-0000-0000-0000-000000000004",
		MetricID:        "ttff_mean",
		QoEField:        "time_to_first_frame_ms",
		ComputationDate: "2024-01-15",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderQoEMetric(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderContentConsumption(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderContentConsumption(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderDailyTreatmentEffect(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:     "e0000000-0000-0000-0000-000000000001",
		MetricID:         "watch_time_minutes",
		ComputationDate:  "2024-01-15",
		ControlVariantID: "f0000000-0000-0000-0000-000000000001",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderDailyTreatmentEffect(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderLifecycleMean(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	p.LifecycleEnabled = true
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderLifecycleMean(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderSessionLevelMean(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	p.SessionLevel = true
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderSessionLevelMean(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderSurrogateInput(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:          "e0000000-0000-0000-0000-000000000001",
		ComputationDate:       "2024-01-15",
		InputMetricIDs:        []string{"watch_time_minutes", "stream_start_rate"},
		ObservationWindowDays: 7,
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderSurrogateInput(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderInterleavingScore(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:        "e0000000-0000-0000-0000-000000000003",
		ComputationDate:     "2024-01-15",
		CreditAssignment:    "proportional",
		EngagementEventType: "click",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderInterleavingScore(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderQoEEngagementCorrelation(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:         "e0000000-0000-0000-0000-000000000007",
		ComputationDate:      "2024-01-25",
		QoEFieldA:            "time_to_first_frame_ms",
		EngagementSourceType: "heartbeat",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderQoEEngagementCorrelation(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderPercentile(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:    "e0000000-0000-0000-0000-000000000006",
		MetricID:        "latency_p50_ms",
		SourceEventType: "playback_start",
		ComputationDate: "2024-01-20",
		Percentile:      0.50,
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderPercentile(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkSQLRenderer_RenderCustom(b *testing.B) {
	r := rendererForBench(b)
	p := TemplateParams{
		ExperimentID:    "e0000000-0000-0000-0000-000000000005",
		MetricID:        "power_users_watch_time",
		ComputationDate: "2024-01-15",
		CustomSQL:       "SELECT user_id, AVG(value) AS metric_value FROM delta.metric_events WHERE event_type = 'heartbeat' GROUP BY user_id HAVING COUNT(*) >= 10",
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := r.RenderCustom(p)
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkSQLRenderer_RenderForType_AllTypes benchmarks the type dispatcher.
func BenchmarkSQLRenderer_RenderForType_AllTypes(b *testing.B) {
	r := rendererForBench(b)

	types := []struct {
		name   string
		typ    string
		params TemplateParams
	}{
		{"MEAN", "MEAN", standardParams()},
		{"PROPORTION", "PROPORTION", TemplateParams{
			ExperimentID: "e1", MetricID: "m1", SourceEventType: "impression", ComputationDate: "2024-01-15",
		}},
		{"COUNT", "COUNT", standardParams()},
		{"RATIO", "RATIO", TemplateParams{
			ExperimentID: "e1", MetricID: "m1", NumeratorEventType: "num", DenominatorEventType: "den", ComputationDate: "2024-01-15",
		}},
		{"PERCENTILE", "PERCENTILE", TemplateParams{
			ExperimentID: "e1", MetricID: "m1", SourceEventType: "start", ComputationDate: "2024-01-15", Percentile: 0.95,
		}},
		{"CUSTOM", "CUSTOM", TemplateParams{
			ExperimentID: "e1", MetricID: "m1", ComputationDate: "2024-01-15",
			CustomSQL: "SELECT user_id, AVG(value) AS metric_value FROM delta.metric_events GROUP BY user_id",
		}},
	}

	for _, tc := range types {
		b.Run(tc.name, func(b *testing.B) {
			for i := 0; i < b.N; i++ {
				_, err := r.RenderForType(tc.typ, tc.params)
				if err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}

// BenchmarkSQLRenderer_Parallel tests concurrent template rendering throughput.
func BenchmarkSQLRenderer_Parallel(b *testing.B) {
	r := rendererForBench(b)
	p := standardParams()
	b.ResetTimer()
	b.RunParallel(func(pb *testing.PB) {
		for pb.Next() {
			_, err := r.RenderMean(p)
			if err != nil {
				b.Fatal(err)
			}
		}
	})
}
