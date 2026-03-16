// Package main is the entry point for the orchestration service.
// Provides SQL query logging for M3 transparency and health/readiness probes.
package main

import (
	"context"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/org/experimentation-platform/services/orchestration/internal/handler"
	"github.com/org/experimentation-platform/services/orchestration/internal/querylog"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	port := os.Getenv("PORT")
	if port == "" {
		port = "50058" // Orchestration — distinct from M3/M5/M7
	}

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	// PostgreSQL connection (optional — use MemWriter for dev/testing).
	var (
		qlWriter querylog.Writer
		pgPool   *pgxpool.Pool
	)

	if pgURL := os.Getenv("POSTGRES_URL"); pgURL != "" {
		pool, err := pgxpool.New(ctx, pgURL)
		if err != nil {
			slog.Error("failed to connect to postgres", "error", err)
			os.Exit(1)
		}
		defer pool.Close()

		if err := pool.Ping(ctx); err != nil {
			slog.Error("failed to ping postgres", "error", err)
			os.Exit(1)
		}

		pgPool = pool
		qlWriter = querylog.NewPgWriter(pool)
		slog.Info("using PostgreSQL query log writer")
	} else {
		qlWriter = querylog.NewMemWriter()
		slog.Warn("POSTGRES_URL not set, using in-memory query log (dev mode)")
	}

	h := handler.New(qlWriter, pgPool)
	mux := http.NewServeMux()
	h.Register(mux)

	srv := &http.Server{Addr: ":" + port, Handler: mux}

	go func() {
		slog.Info("orchestration service starting", "port", port)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("server failed", "error", err)
			os.Exit(1)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down gracefully")
	if err := srv.Shutdown(context.Background()); err != nil {
		slog.Error("shutdown error", "error", err)
	}
}
