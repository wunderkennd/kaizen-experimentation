// Package main is the entry point for the management service.
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

	"github.com/org/experimentation/gen/go/experimentation/assignment/v1/assignmentv1connect"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/guardrail"
	"github.com/org/experimentation-platform/services/management/internal/handlers"
	"github.com/org/experimentation-platform/services/management/internal/sequential"
	"github.com/org/experimentation-platform/services/management/internal/store"
	"github.com/org/experimentation-platform/services/management/internal/streaming"
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"
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
	pool, err := store.NewPool(ctx)
	if err != nil {
		slog.Error("failed to create database pool", "error", err)
		os.Exit(1)
	}
	defer pool.Close()

	// Stores.
	experimentStore := store.NewExperimentStore(pool)
	auditStore := store.NewAuditStore(pool)
	layerStore := store.NewLayerStore(pool)
	metricStore := store.NewMetricStore(pool)
	targetingStore := store.NewTargetingStore(pool)
	surrogateStore := store.NewSurrogateStore(pool)

	// Notifier for streaming config updates.
	dsn := os.Getenv("DATABASE_URL")
	if dsn == "" {
		dsn = "postgres://experimentation:localdev@localhost:5432/experimentation?sslmode=disable"
	}
	notifier := streaming.NewNotifier(pool, dsn)
	notifier.Start(ctx)
	defer notifier.Stop()

	// Service handlers (created before consumers because sequential consumer uses expSvc as Concluder).
	expSvc := handlers.NewExperimentService(experimentStore, auditStore, layerStore, metricStore, targetingStore, surrogateStore, notifier)

	// Kafka consumers (guardrail auto-pause + sequential auto-conclude).
	if brokers := os.Getenv("KAFKA_BROKERS"); brokers != "" {
		brokerList := strings.Split(brokers, ",")

		// Guardrail alert consumer (Kafka → auto-pause).
		grProcessor := guardrail.NewProcessor(experimentStore, auditStore, notifier)
		grConsumer := guardrail.NewConsumer(brokerList, grProcessor)
		grConsumer.Start(ctx)
		defer grConsumer.Stop()
		slog.Info("guardrail consumer started", "brokers", brokers)

		// Sequential boundary alert consumer (Kafka → auto-conclude).
		seqProcessor := sequential.NewProcessor(experimentStore, auditStore, notifier, expSvc)
		seqConsumer := sequential.NewConsumer(brokerList, seqProcessor)
		seqConsumer.Start(ctx)
		defer seqConsumer.Stop()
		slog.Info("sequential consumer started", "brokers", brokers)
	} else {
		slog.Info("kafka consumers disabled (KAFKA_BROKERS not set)")
	}
	streamSvc := handlers.NewConfigStreamService(experimentStore, notifier)

	// Register ConnectRPC handlers on mux.
	mux := http.NewServeMux()

	// Health check endpoint.
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		fmt.Fprint(w, "ok")
	})

	mgmtPath, mgmtHandler := managementv1connect.NewExperimentManagementServiceHandler(expSvc)
	mux.Handle(mgmtPath, mgmtHandler)

	streamPath, streamHandler := assignmentv1connect.NewAssignmentServiceHandler(streamSvc)
	mux.Handle(streamPath, streamHandler)

	srv := &http.Server{
		Addr:    ":" + port,
		Handler: h2c.NewHandler(mux, &http2.Server{}),
	}

	// Graceful shutdown.
	go func() {
		slog.Info("management service starting", "port", port)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("server failed", "error", err)
			os.Exit(1)
		}
	}()

	<-ctx.Done()
	slog.Info("shutting down gracefully")
	srv.Shutdown(context.Background())
}
