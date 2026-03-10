package jobs

import (
	"context"
	"io"
	"log/slog"
	"os"
	"testing"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

// TestMain suppresses slog output during benchmark runs to avoid flooding
// stdout with millions of log lines that slow down and bloat output.
func TestMain(m *testing.M) {
	slog.SetDefault(slog.New(slog.NewTextHandler(io.Discard, nil)))
	os.Exit(m.Run())
}

func loadBenchConfig(b *testing.B) *config.ConfigStore {
	b.Helper()
	cs, err := config.LoadFromFile("../config/testdata/seed_config.json")
	if err != nil {
		b.Fatalf("LoadFromFile: %v", err)
	}
	return cs
}

func newBenchRenderer(b *testing.B) *spark.SQLRenderer {
	b.Helper()
	r, err := spark.NewSQLRenderer()
	if err != nil {
		b.Fatalf("NewSQLRenderer: %v", err)
	}
	return r
}

// --- StandardJob benchmarks ---

// BenchmarkStandardJob_Run_4Metrics benchmarks the full standard job for
// homepage_recs_v2 (4 metrics: PROPORTION, MEAN, PROPORTION, RATIO with
// CUPED and daily treatment effects).
func BenchmarkStandardJob_Run_4Metrics(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkStandardJob_Run_2QoEMetrics benchmarks QoE metric rendering
// (playback_qoe_test: ttff_mean + rebuffer_ratio_mean).
func BenchmarkStandardJob_Run_2QoEMetrics(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000004")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkStandardJob_Run_MixedQoEEngagement benchmarks the mixed experiment
// that exercises QoE + session-level + lifecycle + correlation paths.
func BenchmarkStandardJob_Run_MixedQoEEngagement(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000007")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkStandardJob_Run_PercentileMetric benchmarks PERCENTILE metric type.
func BenchmarkStandardJob_Run_PercentileMetric(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000006")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkStandardJob_Run_CustomMetric benchmarks CUSTOM metric type.
func BenchmarkStandardJob_Run_CustomMetric(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000005")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// --- GuardrailJob benchmarks ---

// BenchmarkGuardrailJob_Run benchmarks guardrail checking for homepage_recs_v2
// (2 guardrails: rebuffer_rate, error_rate).
func BenchmarkGuardrailJob_Run(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()

	vp := NewMockValueProvider()
	cv := "f0000000-0000-0000-0000-000000000001"
	tv := "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03)
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.008)

	job := NewGuardrailJob(cfg, renderer, executor, qlWriter, publisher, tracker, vp)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkGuardrailJob_Run_NoGuardrails benchmarks early-exit for experiments
// without guardrails.
func BenchmarkGuardrailJob_Run_NoGuardrails(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := NewMockValueProvider()

	job := NewGuardrailJob(cfg, renderer, executor, qlWriter, publisher, tracker, vp)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		// search_ranking_interleave has no guardrails → fast exit
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// --- InterleavingJob benchmarks ---

// BenchmarkInterleavingJob_Run benchmarks interleaving scoring.
func BenchmarkInterleavingJob_Run(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(300)
	qlWriter := querylog.NewMemWriter()
	job := NewInterleavingJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkInterleavingJob_Run_Skip benchmarks early exit for non-INTERLEAVING.
func BenchmarkInterleavingJob_Run_Skip(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(300)
	qlWriter := querylog.NewMemWriter()
	job := NewInterleavingJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		// AB experiment → immediate return
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// --- ContentConsumptionJob benchmarks ---

func BenchmarkContentConsumptionJob_Run(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	executor := spark.NewMockExecutor(200)
	qlWriter := querylog.NewMemWriter()
	job := NewContentConsumptionJob(cfg, renderer, executor, qlWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		executor.Reset()
		qlWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// --- SurrogateJob benchmarks ---

// BenchmarkSurrogateJob_Run benchmarks surrogate model pipeline (input SQL +
// model load + predict + projection write).
func BenchmarkSurrogateJob_Run(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	qlWriter := querylog.NewMemWriter()

	inputs := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"watch_time_minutes": 42.5, "stream_start_rate": 0.85},
		"f0000000-0000-0000-0000-000000000002": {"watch_time_minutes": 45.0, "stream_start_rate": 0.88},
	}
	inputProvider := &MockInputMetricsProvider{Inputs: inputs}
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()

	job := NewSurrogateJob(cfg, renderer, inputProvider, qlWriter, modelLoader, projWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		qlWriter.Reset()
		projWriter.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkSurrogateJob_Run_NoModel benchmarks early exit when experiment has
// no surrogate model.
func BenchmarkSurrogateJob_Run_NoModel(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	qlWriter := querylog.NewMemWriter()
	inputProvider := &MockInputMetricsProvider{}
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()

	job := NewSurrogateJob(cfg, renderer, inputProvider, qlWriter, modelLoader, projWriter)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		// e3 has no surrogate model
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// --- RecalibrationJob benchmarks ---

// BenchmarkRecalibrationJob_Run benchmarks recalibration with pre-loaded
// projections and actual values.
func BenchmarkRecalibrationJob_Run(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	qlWriter := querylog.NewMemWriter()

	actuals := surrogate.InputMetrics{
		"f0000000-0000-0000-0000-000000000001": {"churn_7d": 0.15},
		"f0000000-0000-0000-0000-000000000002": {"churn_7d": 0.035},
		"f0000000-0000-0000-0000-extra-variant": {"churn_7d": 0.08},
	}
	inputProvider := &MockInputMetricsProvider{Inputs: actuals}
	projWriter := surrogate.NewMemProjectionWriter()
	calibUpdater := surrogate.NewMemCalibrationUpdater()

	// Pre-load projections.
	ctx := context.Background()
	projections := []surrogate.ProjectionRecord{
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-000000000002", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.1175, ComputedAt: time.Now()},
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", VariantID: "f0000000-0000-0000-0000-extra-variant", ModelID: "sm-churn-predictor-001", ProjectedEffect: -0.07, ComputedAt: time.Now()},
	}
	for _, p := range projections {
		_ = projWriter.Write(ctx, p)
	}

	job := NewRecalibrationJob(cfg, renderer, inputProvider, qlWriter, projWriter, calibUpdater)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		qlWriter.Reset()
		calibUpdater.Reset()
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkRecalibrationJob_Run_NoModel benchmarks early exit when experiment
// has no surrogate model.
func BenchmarkRecalibrationJob_Run_NoModel(b *testing.B) {
	cfg := loadBenchConfig(b)
	renderer := newBenchRenderer(b)
	qlWriter := querylog.NewMemWriter()
	inputProvider := &MockInputMetricsProvider{}
	projWriter := surrogate.NewMemProjectionWriter()
	calibUpdater := surrogate.NewMemCalibrationUpdater()

	job := NewRecalibrationJob(cfg, renderer, inputProvider, qlWriter, projWriter, calibUpdater)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		// e3 has no surrogate model → fast exit.
		_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000003")
		if err != nil {
			b.Fatal(err)
		}
	}
}
