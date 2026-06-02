package handlers

import (
	"context"

	"connectrpc.com/connect"

	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
)

// MigrateMetricDefinition is part of the ADR-026 Phase 3 (#437) convergence
// surface (Lock L7 — two-step apply with shadow-run validation). Per ADR-025,
// the canonical M5 implementation is the Rust crate
// `experimentation-management`, which owns the full migration handler
// including the M3 shadow-run-result validation and the atomic
// `metric_migrations` write. The Go variant of M5 is retained-until-
// deprecation and does NOT reimplement this endpoint; this stub exists
// solely so `*ExperimentService` satisfies the regenerated
// `managementv1connect.ExperimentManagementServiceHandler` interface for
// `go vet` / `go build`. Any caller hitting the Go service for migration
// receives `Unimplemented` with a clear pointer to the Rust path.
//
// Mirrors the pattern in `metricql_stubs.go`; reuses
// `errUnimplementedInGoVariant` from that file.
func (s *ExperimentService) MigrateMetricDefinition(
	ctx context.Context,
	req *connect.Request[mgmtv1.MigrateMetricDefinitionRequest],
) (*connect.Response[mgmtv1.MigrateMetricDefinitionResponse], error) {
	return nil, connect.NewError(
		connect.CodeUnimplemented,
		errUnimplementedInGoVariant("MigrateMetricDefinition"),
	)
}
