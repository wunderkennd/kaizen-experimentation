package auth

import (
	"context"
	"fmt"
	"log/slog"

	"connectrpc.com/connect"

	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
)

const (
	// HeaderUserEmail is the HTTP header set by the API gateway containing the authenticated user's email.
	HeaderUserEmail = "X-User-Email"
	// HeaderUserRole is the HTTP header set by the API gateway containing the user's role.
	HeaderUserRole = "X-User-Role"
)

// procedurePermissions maps each RPC procedure to the minimum role required.
var procedurePermissions = map[string]Role{
	// Read-only / stateless evaluation — viewer
	flagsv1connect.FeatureFlagServiceGetFlagProcedure:       RoleViewer,
	flagsv1connect.FeatureFlagServiceListFlagsProcedure:     RoleViewer,
	flagsv1connect.FeatureFlagServiceEvaluateFlagProcedure:  RoleViewer,
	flagsv1connect.FeatureFlagServiceEvaluateFlagsProcedure: RoleViewer,

	// Write operations affecting production behavior — experimenter
	flagsv1connect.FeatureFlagServiceCreateFlagProcedure: RoleExperimenter,
	flagsv1connect.FeatureFlagServiceUpdateFlagProcedure: RoleExperimenter,

	// Cross-service: creates experiment in M5 — admin
	flagsv1connect.FeatureFlagServicePromoteToExperimentProcedure: RoleAdmin,
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
