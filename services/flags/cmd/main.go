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

	"github.com/org/experimentation-platform/services/flags/internal/handlers"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))
	slog.SetDefault(logger)

	port := os.Getenv("PORT")
	if port == "" {
		port = "50055"
	}

	mux := http.NewServeMux()

	// Health check endpoint.
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, "ok")
	})

	// Build FlagService with optional management client.
	// TODO: Add pgxpool.Pool for PostgreSQL store when DATABASE_URL is configured.
	var svc *handlers.FlagService

	mgmtURL := os.Getenv("MANAGEMENT_SERVICE_URL")
	if mgmtURL != "" {
		mgmtClient := managementv1connect.NewExperimentManagementServiceClient(
			http.DefaultClient,
			mgmtURL,
		)
		svc = handlers.NewFlagServiceFull(nil, nil, mgmtClient)
		slog.Info("management client configured", "url", mgmtURL)
	} else {
		svc = handlers.NewFlagService(nil)
		slog.Warn("no MANAGEMENT_SERVICE_URL set — PromoteToExperiment will use mock mode")
	}

	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)

	// Register audit endpoints.
	svc.RegisterAuditRoutes(mux)

	srv := &http.Server{
		Addr:    ":" + port,
		Handler: mux,
	}

	// Graceful shutdown.
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	go func() {
		slog.Info("flags service starting", "port", port)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("server failed", "error", err)
			os.Exit(1)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down gracefully")
	srv.Shutdown(context.Background())
}
