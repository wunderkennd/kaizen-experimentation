// Package main is the entry point for the metrics service.
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
	if port == "" {
		port = "50055"
	}

	configPath := os.Getenv("CONFIG_PATH")
	if configPath == "" {
		configPath = "internal/config/testdata/seed_config.json"
	}

	// Load experiment and metric configs from local JSON (mocking M5).
	cfgStore, err := config.LoadFromFile(configPath)
	if err != nil {
		slog.Error("failed to load config", "path", configPath, "error", err)
		os.Exit(1)
	}
	slog.Info("loaded config", "path", configPath)

	// SQL template renderer.
	renderer, err := spark.NewSQLRenderer()
	if err != nil {
		slog.Error("failed to create SQL renderer", "error", err)
		os.Exit(1)
	}

	// SQL executor — use mock for now until Spark cluster is available.
	executor := spark.NewMockExecutor(500)

	// Query log writer — use PostgreSQL if configured, else in-memory.
	var qlWriter querylog.Writer
	pgURL := os.Getenv("POSTGRES_URL")
	if pgURL != "" {
		pool, err := pgxpool.New(context.Background(), pgURL)
		if err != nil {
			slog.Error("failed to connect to PostgreSQL", "error", err)
			os.Exit(1)
		}
		defer pool.Close()
		qlWriter = querylog.NewPgWriter(pool)
		slog.Info("using PostgreSQL query log writer")
	} else {
		qlWriter = querylog.NewMemWriter()
		slog.Warn("POSTGRES_URL not set, using in-memory query log (dev mode)")
	}

	// Wire up the standard metric computation job.
	stdJob := jobs.NewStandardJob(cfgStore, renderer, executor, qlWriter)

	// ConnectRPC handler.
	metricsHandler := handler.NewMetricsHandler(stdJob, qlWriter)

	mux := http.NewServeMux()

	// Health check endpoint.
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, "ok")
	})

	// Register ConnectRPC service handler.
	path, h := metricsv1connect.NewMetricComputationServiceHandler(metricsHandler)
	mux.Handle(path, h)

	srv := &http.Server{
		Addr:    ":" + port,
		Handler: h2c.NewHandler(mux, &http2.Server{}),
	}

	// Graceful shutdown.
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	go func() {
		slog.Info("metrics service starting", "port", port)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("server failed", "error", err)
			os.Exit(1)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down gracefully")
	srv.Shutdown(context.Background())
}
