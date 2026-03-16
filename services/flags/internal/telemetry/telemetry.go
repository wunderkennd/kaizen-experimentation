// Package telemetry provides OpenTelemetry tracing and Prometheus metrics for the flags service.
package telemetry

import (
	"context"
	"net/http"
	"os"

	"github.com/prometheus/client_golang/prometheus/promhttp"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc"
	prometheusexporter "go.opentelemetry.io/otel/exporters/prometheus"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	semconv "go.opentelemetry.io/otel/semconv/v1.26.0"
)

// Metrics holds the custom OTel instruments for the flags service.
type Metrics struct {
	FlagEvaluationsTotal metric.Int64Counter
	FlagPromotionsTotal  metric.Int64Counter
	ReconcilerRunsTotal  metric.Int64Counter
	ReconcilerDuration   metric.Float64Histogram
}

// Init sets up OpenTelemetry tracing (OTLP → Jaeger) and metrics (Prometheus exporter).
// Returns Metrics, a cleanup function, and any initialization error.
func Init(ctx context.Context) (*Metrics, func(), error) {
	serviceName := os.Getenv("OTEL_SERVICE_NAME")
	if serviceName == "" {
		serviceName = "flags-service"
	}

	res, err := resource.New(ctx,
		resource.WithAttributes(semconv.ServiceName(serviceName)),
	)
	if err != nil {
		return nil, nil, err
	}

	var cleanups []func()
	rollback := func() {
		for i := len(cleanups) - 1; i >= 0; i-- {
			cleanups[i]()
		}
	}

	// Trace provider: OTLP gRPC exporter → Jaeger. Skip if endpoint is empty.
	otlpEndpoint := os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT")
	if otlpEndpoint != "" {
		traceExporter, err := otlptracegrpc.New(ctx,
			otlptracegrpc.WithEndpoint(otlpEndpoint),
			otlptracegrpc.WithInsecure(),
		)
		if err != nil {
			return nil, nil, err
		}

		tp := sdktrace.NewTracerProvider(
			sdktrace.WithBatcher(traceExporter),
			sdktrace.WithResource(res),
		)
		otel.SetTracerProvider(tp)
		cleanups = append(cleanups, func() { _ = tp.Shutdown(context.Background()) })
	}

	// W3C Trace Context propagation.
	otel.SetTextMapPropagator(propagation.TraceContext{})

	// Meter provider: Prometheus exporter.
	promExporter, err := prometheusexporter.New()
	if err != nil {
		rollback()
		return nil, nil, err
	}

	mp := sdkmetric.NewMeterProvider(
		sdkmetric.WithReader(promExporter),
		sdkmetric.WithResource(res),
	)
	otel.SetMeterProvider(mp)
	cleanups = append(cleanups, func() { _ = mp.Shutdown(context.Background()) })

	// Create instruments.
	meter := mp.Meter("flags-service")

	flagEvals, err := meter.Int64Counter("flag_evaluations_total",
		metric.WithDescription("Total flag evaluations by result"),
	)
	if err != nil {
		rollback()
		return nil, nil, err
	}

	flagPromotions, err := meter.Int64Counter("flag_promotions_total",
		metric.WithDescription("Total flag promotions to experiments"),
	)
	if err != nil {
		rollback()
		return nil, nil, err
	}

	reconcilerRuns, err := meter.Int64Counter("reconciler_runs_total",
		metric.WithDescription("Total reconciler actions per flag"),
	)
	if err != nil {
		rollback()
		return nil, nil, err
	}

	reconcilerDuration, err := meter.Float64Histogram("reconciler_duration_seconds",
		metric.WithDescription("Duration of reconciler passes"),
		metric.WithExplicitBucketBoundaries(0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 5.0, 30.0, 60.0),
	)
	if err != nil {
		rollback()
		return nil, nil, err
	}

	cleanup := func() {
		for i := len(cleanups) - 1; i >= 0; i-- {
			cleanups[i]()
		}
	}

	return &Metrics{
		FlagEvaluationsTotal: flagEvals,
		FlagPromotionsTotal:  flagPromotions,
		ReconcilerRunsTotal:  reconcilerRuns,
		ReconcilerDuration:   reconcilerDuration,
	}, cleanup, nil
}

// PrometheusHandler returns an HTTP handler that serves Prometheus metrics.
func PrometheusHandler() http.Handler {
	return promhttp.Handler()
}
