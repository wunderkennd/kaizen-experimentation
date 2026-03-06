// Package main is the entry point for the flags service.
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
	"github.com/org/experimentation-platform/services/flags/internal/handlers"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	port := os.Getenv("PORT")
	if port == "" {
		port = "50055"
	}

	// Database connection pool.
	var pool *pgxpool.Pool
	var flagStore store.Store
	var auditStore store.AuditStore

	if dsn := os.Getenv("DATABASE_URL"); dsn != "" {
		var err error
		pool, err = store.NewPool(ctx)
		if err != nil {
			slog.Error("failed to connect to database", "error", err)
			os.Exit(1)
		}
		defer pool.Close()
		flagStore = store.NewPostgresStore(pool)
		auditStore = store.NewPostgresAuditStore(pool)
		slog.Info("database connected", "dsn_set", true)
	} else {
		flagStore = store.NewMockStore()
		slog.Warn("DATABASE_URL not set — using in-memory mock store (dev mode only)")
	}

	// Build FlagService with optional management client.
	var svc *handlers.FlagService

	mgmtURL := os.Getenv("MANAGEMENT_SERVICE_URL")
	if mgmtURL != "" {
		mgmtClient := managementv1connect.NewExperimentManagementServiceClient(
			http.DefaultClient,
			mgmtURL,
		)
		svc = handlers.NewFlagServiceFull(flagStore, auditStore, mgmtClient)
		slog.Info("management client configured", "url", mgmtURL)
	} else {
		svc = handlers.NewFlagServiceWithAudit(flagStore, auditStore)
		slog.Warn("no MANAGEMENT_SERVICE_URL set — PromoteToExperiment will use mock mode")
	}

	mux := http.NewServeMux()

	// Health check endpoint (liveness).
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, "ok")
	})

	// Readiness check (includes database ping).
	mux.HandleFunc("/readyz", func(w http.ResponseWriter, r *http.Request) {
		if pool != nil {
			if err := pool.Ping(r.Context()); err != nil {
				http.Error(w, "database not ready", http.StatusServiceUnavailable)
				return
			}
		}
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, "ok")
	})

	// ConnectRPC handler.
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)

	// Register audit endpoints.
	svc.RegisterAuditRoutes(mux)

	srv := &http.Server{
		Addr:    ":" + port,
		Handler: mux,
	}

	// Graceful shutdown.
	go func() {
		slog.Info("flags service starting", "port", port, "database", pool != nil)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("server failed", "error", err)
			os.Exit(1)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down gracefully")
	srv.Shutdown(context.Background())
}
