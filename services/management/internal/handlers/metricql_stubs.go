package handlers

import (
	"context"

	"connectrpc.com/connect"

	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
)

// ValidateMetricql + PreviewMetricDefinition are part of the ADR-026 Phase 2
// (#436) MetricQL surface. Per ADR-025, the canonical M5 implementation is the
// Rust crate `experimentation-management`, which owns the full MetricQL
// validator pipeline + the M3 dry-run preview proxy. The Go variant of M5 is
// retained-until-deprecation (see CLAUDE.md "M5 Management" row) and does NOT
// reimplement these endpoints. These stubs exist solely so `*ExperimentService`
// satisfies the regenerated `managementv1connect.ExperimentManagementServiceHandler`
// interface for `go vet` / `go build`; any actual caller hitting the Go service
// for these RPCs receives `Unimplemented` with a clear pointer to the Rust path.

// ValidateMetricql is unimplemented in the Go variant of M5. See ADR-025.
func (s *ExperimentService) ValidateMetricql(
	ctx context.Context,
	req *connect.Request[mgmtv1.ValidateMetricqlRequest],
) (*connect.Response[mgmtv1.ValidateMetricqlResponse], error) {
	return nil, connect.NewError(
		connect.CodeUnimplemented,
		errUnimplementedInGoVariant("ValidateMetricql"),
	)
}

// PreviewMetricDefinition is unimplemented in the Go variant of M5. See ADR-025.
func (s *ExperimentService) PreviewMetricDefinition(
	ctx context.Context,
	req *connect.Request[mgmtv1.PreviewMetricDefinitionRequest],
) (*connect.Response[mgmtv1.PreviewMetricDefinitionResponse], error) {
	return nil, connect.NewError(
		connect.CodeUnimplemented,
		errUnimplementedInGoVariant("PreviewMetricDefinition"),
	)
}

// errUnimplementedInGoVariant constructs the canonical not-here error for the
// MetricQL RPCs. Centralised so a future Go reimplementation (if anyone undoes
// ADR-025's Rust-first decision) flips both call sites by changing this helper.
func errUnimplementedInGoVariant(rpcName string) error {
	return &goVariantUnimplementedError{rpc: rpcName}
}

type goVariantUnimplementedError struct{ rpc string }

func (e *goVariantUnimplementedError) Error() string {
	return e.rpc + " is implemented only in the Rust M5 service (ADR-025); the Go variant is retained for legacy compatibility and does not handle MetricQL RPCs"
}
