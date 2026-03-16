package main

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/prometheus/client_golang/prometheus/promhttp"
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"

	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	_ "github.com/org/experimentation-platform/services/metrics/internal/metrics"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/handler"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/recalconsumer"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)
	port := os.Getenv("PORT")
	if port == "" { port = "50056" } // M3 Metrics — matches .env.example METRICS_SERVICE_PORT
	configPath := os.Getenv("CONFIG_PATH")
	if configPath == "" { configPath = "internal/config/testdata/seed_config.json" }
	cfgStore, err := config.LoadFromFile(configPath)
	if err != nil { slog.Error("failed to load config", "error", err); os.Exit(1) }
	renderer, err := spark.NewSQLRenderer()
	if err != nil { slog.Error("failed to create SQL renderer", "error", err); os.Exit(1) }
	baseExecutor := spark.NewMockExecutor(500)
	executor := spark.NewRetryExecutor(baseExecutor, spark.DefaultRetryConfig())
	var qlWriter querylog.Writer
	pgURL := os.Getenv("POSTGRES_URL")
	if pgURL != "" {
		pool, err := pgxpool.New(context.Background(), pgURL)
		if err != nil { slog.Error("failed to connect to PostgreSQL", "error", err); os.Exit(1) }
		defer pool.Close()
		qlWriter = querylog.NewPgWriter(pool)
	} else {
		qlWriter = querylog.NewMemWriter()
		slog.Warn("POSTGRES_URL not set, using in-memory query log")
	}
	stdJob := jobs.NewStandardJob(cfgStore, renderer, executor, qlWriter)
	kafkaBrokers := os.Getenv("KAFKA_BROKERS")
	if kafkaBrokers == "" {
		kafkaBrokers = "localhost:9092"
	}
	brokers := strings.Split(kafkaBrokers, ",")
	gPublisher := alerts.NewKafkaPublisher(brokers, "guardrail_alerts")
	gTracker := alerts.NewBreachTracker()
	gVP := jobs.NewMockValueProvider()
	gJob := jobs.NewGuardrailJob(cfgStore, renderer, executor, qlWriter, gPublisher, gTracker, gVP)
	ccJob := jobs.NewContentConsumptionJob(cfgStore, renderer, executor, qlWriter)
	// Surrogate metric framework: mock input provider + mock model loader for dev.
	mockInputProvider := &jobs.MockInputMetricsProvider{}
	modelLoader := surrogate.NewMockModelLoader()
	projWriter := surrogate.NewMemProjectionWriter()
	surrJob := jobs.NewSurrogateJob(cfgStore, renderer, mockInputProvider, qlWriter, modelLoader, projWriter)
	ilJob := jobs.NewInterleavingJob(cfgStore, renderer, executor, qlWriter)
	calibUpdater := surrogate.NewMemCalibrationUpdater()
	recalJob := jobs.NewRecalibrationJob(cfgStore, renderer, mockInputProvider, qlWriter, projWriter, calibUpdater)
	metricsHandler := handler.NewMetricsHandler(stdJob, gJob, ccJob, surrJob, ilJob, recalJob, qlWriter)
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(200); fmt.Fprint(w, "ok") })
	path, h := metricsv1connect.NewMetricComputationServiceHandler(metricsHandler)
	mux.Handle(path, h)
	srv := &http.Server{Addr: ":" + port, Handler: h2c.NewHandler(mux, &http2.Server{})}
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	// Start Kafka consumer for surrogate recalibration requests from M5.
	recalConsumer := recalconsumer.NewConsumer(brokers, recalJob, cfgStore)
	recalConsumer.Start(ctx)
	defer recalConsumer.Close()
	// Start Prometheus metrics HTTP server on a separate port.
	metricsPort := os.Getenv("METRICS_PORT")
	if metricsPort == "" { metricsPort = "50059" } // Prometheus scrape endpoint — must differ from main PORT (50056) and Prometheus server (9090)
	metricsMux := http.NewServeMux()
	metricsMux.Handle("/metrics", promhttp.Handler())
	metricsSrv := &http.Server{Addr: ":" + metricsPort, Handler: metricsMux}
	go func() {
		slog.Info("prometheus metrics server starting", "port", metricsPort)
		if err := metricsSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed { slog.Error("metrics server failed", "error", err) }
	}()
	go func() {
		slog.Info("metrics service starting", "port", port)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed { slog.Error("server failed", "error", err); os.Exit(1) }
	}()
	<-ctx.Done()
	slog.Info("shutting down")
	metricsSrv.Shutdown(context.Background())
	srv.Shutdown(context.Background())
}
