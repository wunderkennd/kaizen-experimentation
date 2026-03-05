package main

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"github.com/jackc/pgx/v5/pgxpool"
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"

	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/handler"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)
	port := os.Getenv("PORT")
	if port == "" { port = "50055" }
	configPath := os.Getenv("CONFIG_PATH")
	if configPath == "" { configPath = "internal/config/testdata/seed_config.json" }
	cfgStore, err := config.LoadFromFile(configPath)
	if err != nil { slog.Error("failed to load config", "error", err); os.Exit(1) }
	renderer, err := spark.NewSQLRenderer()
	if err != nil { slog.Error("failed to create SQL renderer", "error", err); os.Exit(1) }
	executor := spark.NewMockExecutor(500)
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
	gPublisher := alerts.NewKafkaPublisher("guardrail_alerts")
	gTracker := alerts.NewBreachTracker()
	gVP := jobs.NewMockValueProvider()
	gJob := jobs.NewGuardrailJob(cfgStore, renderer, executor, qlWriter, gPublisher, gTracker, gVP)
	metricsHandler := handler.NewMetricsHandler(stdJob, gJob, qlWriter)
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(200); fmt.Fprint(w, "ok") })
	path, h := metricsv1connect.NewMetricComputationServiceHandler(metricsHandler)
	mux.Handle(path, h)
	srv := &http.Server{Addr: ":" + port, Handler: h2c.NewHandler(mux, &http2.Server{})}
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	go func() {
		slog.Info("metrics service starting", "port", port)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed { slog.Error("server failed", "error", err); os.Exit(1) }
	}()
	<-ctx.Done()
	slog.Info("shutting down")
	srv.Shutdown(context.Background())
}
