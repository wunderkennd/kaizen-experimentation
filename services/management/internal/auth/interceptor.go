package auth

import (
	"context"
	"fmt"
	"log/slog"

	"connectrpc.com/connect"

	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"
)

const (
	// HeaderUserEmail is the HTTP header set by the API gateway containing the authenticated user's email.
	HeaderUserEmail = "X-User-Email"
	// HeaderUserRole is the HTTP header set by the API gateway containing the user's role.
	HeaderUserRole = "X-User-Role"
)

// procedurePermissions maps each RPC procedure to the minimum role required.
var procedurePermissions = map[string]Role{
	// Read-only operations — viewer
	managementv1connect.ExperimentManagementServiceGetExperimentProcedure:             RoleViewer,
	managementv1connect.ExperimentManagementServiceListExperimentsProcedure:            RoleViewer,
	managementv1connect.ExperimentManagementServiceGetMetricDefinitionProcedure:        RoleViewer,
	managementv1connect.ExperimentManagementServiceListMetricDefinitionsProcedure:      RoleViewer,
	managementv1connect.ExperimentManagementServiceGetLayerProcedure:                   RoleViewer,
	managementv1connect.ExperimentManagementServiceGetLayerAllocationsProcedure:        RoleViewer,
	managementv1connect.ExperimentManagementServiceListSurrogateModelsProcedure:        RoleViewer,
	managementv1connect.ExperimentManagementServiceGetSurrogateCalibrationProcedure:    RoleViewer,
	managementv1connect.ExperimentManagementServiceGetPortfolioAllocationProcedure:     RoleViewer,

	// Analyst operations
	managementv1connect.ExperimentManagementServiceCreateMetricDefinitionProcedure:        RoleAnalyst,
	managementv1connect.ExperimentManagementServiceCreateTargetingRuleProcedure:            RoleAnalyst,
	managementv1connect.ExperimentManagementServiceCreateSurrogateModelProcedure:           RoleAnalyst,
	managementv1connect.ExperimentManagementServiceTriggerSurrogateRecalibrationProcedure:  RoleAnalyst,

	// Experimenter operations
	managementv1connect.ExperimentManagementServiceCreateExperimentProcedure:   RoleExperimenter,
	managementv1connect.ExperimentManagementServiceUpdateExperimentProcedure:   RoleExperimenter,
	managementv1connect.ExperimentManagementServiceStartExperimentProcedure:    RoleExperimenter,
	managementv1connect.ExperimentManagementServicePauseExperimentProcedure:    RoleExperimenter,
	managementv1connect.ExperimentManagementServiceResumeExperimentProcedure:   RoleExperimenter,
	managementv1connect.ExperimentManagementServiceConcludeExperimentProcedure: RoleExperimenter,

	// Admin operations
	managementv1connect.ExperimentManagementServiceArchiveExperimentProcedure: RoleAdmin,
	managementv1connect.ExperimentManagementServiceCreateLayerProcedure:       RoleAdmin,
}

// NewAuthInterceptor returns a unary ConnectRPC interceptor that extracts identity
// from API gateway headers and enforces role-based permissions.
func NewAuthInterceptor() connect.UnaryInterceptorFunc {
	return func(next connect.UnaryFunc) connect.UnaryFunc {
		return func(ctx context.Context, req connect.AnyRequest) (connect.AnyResponse, error) {
			email := req.Header().Get(HeaderUserEmail)
			if email == "" {
				return nil, connect.NewError(connect.CodeUnauthenticated, fmt.Errorf("missing %s header", HeaderUserEmail))
			}

			roleStr := req.Header().Get(HeaderUserRole)
			if roleStr == "" {
				return nil, connect.NewError(connect.CodeUnauthenticated, fmt.Errorf("missing %s header", HeaderUserRole))
			}

			role, err := ParseRole(roleStr)
			if err != nil {
				return nil, connect.NewError(connect.CodeUnauthenticated, err)
			}

			// Determine required role for this procedure.
			required, ok := procedurePermissions[req.Spec().Procedure]
			if !ok {
				// Unknown procedures default to admin.
				required = RoleAdmin
				slog.Warn("unknown procedure, defaulting to admin", "procedure", req.Spec().Procedure)
			}

			if !role.HasAtLeast(required) {
				return nil, connect.NewError(connect.CodePermissionDenied,
					fmt.Errorf("role %q insufficient for %s (requires %s)", role, req.Spec().Procedure, required))
			}

			ctx = WithIdentity(ctx, Identity{Email: email, Role: role})
			return next(ctx, req)
		}
	}
}
