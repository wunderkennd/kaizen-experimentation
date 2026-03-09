package auth_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
	"github.com/org/experimentation/gen/go/experimentation/management/v1/managementv1connect"

	"github.com/org/experimentation-platform/services/management/internal/auth"
)

func TestParseRole(t *testing.T) {
	tests := []struct {
		input   string
		want    auth.Role
		wantErr bool
	}{
		{"viewer", auth.RoleViewer, false},
		{"analyst", auth.RoleAnalyst, false},
		{"experimenter", auth.RoleExperimenter, false},
		{"admin", auth.RoleAdmin, false},
		{"superadmin", "", true},
		{"", "", true},
		{"ADMIN", "", true}, // case-sensitive
	}
	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got, err := auth.ParseRole(tt.input)
			if tt.wantErr {
				assert.Error(t, err)
			} else {
				require.NoError(t, err)
				assert.Equal(t, tt.want, got)
			}
		})
	}
}

func TestRoleHasAtLeast(t *testing.T) {
	tests := []struct {
		role    auth.Role
		min     auth.Role
		allowed bool
	}{
		{auth.RoleAdmin, auth.RoleAdmin, true},
		{auth.RoleAdmin, auth.RoleExperimenter, true},
		{auth.RoleAdmin, auth.RoleAnalyst, true},
		{auth.RoleAdmin, auth.RoleViewer, true},
		{auth.RoleExperimenter, auth.RoleExperimenter, true},
		{auth.RoleExperimenter, auth.RoleAnalyst, true},
		{auth.RoleExperimenter, auth.RoleViewer, true},
		{auth.RoleExperimenter, auth.RoleAdmin, false},
		{auth.RoleAnalyst, auth.RoleAnalyst, true},
		{auth.RoleAnalyst, auth.RoleViewer, true},
		{auth.RoleAnalyst, auth.RoleExperimenter, false},
		{auth.RoleAnalyst, auth.RoleAdmin, false},
		{auth.RoleViewer, auth.RoleViewer, true},
		{auth.RoleViewer, auth.RoleAnalyst, false},
		{auth.RoleViewer, auth.RoleExperimenter, false},
		{auth.RoleViewer, auth.RoleAdmin, false},
	}
	for _, tt := range tests {
		t.Run(string(tt.role)+">="+string(tt.min), func(t *testing.T) {
			assert.Equal(t, tt.allowed, tt.role.HasAtLeast(tt.min))
		})
	}
}

func TestIdentityContext(t *testing.T) {
	id := auth.Identity{Email: "alice@example.com", Role: auth.RoleExperimenter}
	ctx := auth.WithIdentity(context.Background(), id)

	got, err := auth.FromContext(ctx)
	require.NoError(t, err)
	assert.Equal(t, id, got)
}

func TestFromContextMissing(t *testing.T) {
	_, err := auth.FromContext(context.Background())
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}

// stubManagementService implements the management service interface with stubs.
type stubManagementService struct {
	managementv1connect.UnimplementedExperimentManagementServiceHandler
	capturedCtx context.Context
}

func (s *stubManagementService) GetExperiment(ctx context.Context, _ *connect.Request[mgmtv1.GetExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&commonv1.Experiment{}), nil
}

func (s *stubManagementService) CreateExperiment(ctx context.Context, _ *connect.Request[mgmtv1.CreateExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&commonv1.Experiment{}), nil
}

func (s *stubManagementService) ArchiveExperiment(ctx context.Context, _ *connect.Request[mgmtv1.ArchiveExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&commonv1.Experiment{}), nil
}

// withAuthHeaders returns a client option that injects auth headers.
func withAuthHeaders(email, role string) connect.ClientOption {
	return connect.WithInterceptors(connect.UnaryInterceptorFunc(
		func(next connect.UnaryFunc) connect.UnaryFunc {
			return func(ctx context.Context, req connect.AnyRequest) (connect.AnyResponse, error) {
				req.Header().Set(auth.HeaderUserEmail, email)
				req.Header().Set(auth.HeaderUserRole, role)
				return next(ctx, req)
			}
		},
	))
}

func setupStubServer(t *testing.T) (*stubManagementService, string, func()) {
	t.Helper()
	stub := &stubManagementService{}
	mux := http.NewServeMux()
	path, handler := managementv1connect.NewExperimentManagementServiceHandler(stub,
		connect.WithInterceptors(auth.NewAuthInterceptor()),
	)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	return stub, server.URL, server.Close
}

func TestInterceptor_ValidRequest(t *testing.T) {
	stub, url, cleanup := setupStubServer(t)
	defer cleanup()

	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("alice@example.com", "viewer"),
	)

	_, err := client.GetExperiment(context.Background(), connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: "test-id",
	}))
	require.NoError(t, err)

	id, err := auth.FromContext(stub.capturedCtx)
	require.NoError(t, err)
	assert.Equal(t, "alice@example.com", id.Email)
	assert.Equal(t, auth.RoleViewer, id.Role)
}

func TestInterceptor_MissingEmail(t *testing.T) {
	_, url, cleanup := setupStubServer(t)
	defer cleanup()

	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, url,
		connect.WithInterceptors(connect.UnaryInterceptorFunc(
			func(next connect.UnaryFunc) connect.UnaryFunc {
				return func(ctx context.Context, req connect.AnyRequest) (connect.AnyResponse, error) {
					req.Header().Set(auth.HeaderUserRole, "viewer")
					return next(ctx, req)
				}
			},
		)),
	)

	_, err := client.GetExperiment(context.Background(), connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: "test-id",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}

func TestInterceptor_MissingRole(t *testing.T) {
	_, url, cleanup := setupStubServer(t)
	defer cleanup()

	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, url,
		connect.WithInterceptors(connect.UnaryInterceptorFunc(
			func(next connect.UnaryFunc) connect.UnaryFunc {
				return func(ctx context.Context, req connect.AnyRequest) (connect.AnyResponse, error) {
					req.Header().Set(auth.HeaderUserEmail, "alice@example.com")
					return next(ctx, req)
				}
			},
		)),
	)

	_, err := client.GetExperiment(context.Background(), connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: "test-id",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}

func TestInterceptor_InvalidRole(t *testing.T) {
	_, url, cleanup := setupStubServer(t)
	defer cleanup()

	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("alice@example.com", "superadmin"),
	)

	_, err := client.GetExperiment(context.Background(), connect.NewRequest(&mgmtv1.GetExperimentRequest{
		ExperimentId: "test-id",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}

func TestInterceptor_InsufficientRole(t *testing.T) {
	_, url, cleanup := setupStubServer(t)
	defer cleanup()

	// Viewer trying to create (requires experimenter).
	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("viewer@example.com", "viewer"),
	)

	_, err := client.CreateExperiment(context.Background(), connect.NewRequest(&mgmtv1.CreateExperimentRequest{}))
	require.Error(t, err)
	assert.Equal(t, connect.CodePermissionDenied, connect.CodeOf(err))
}

func TestInterceptor_AdminOnlyArchive(t *testing.T) {
	_, url, cleanup := setupStubServer(t)
	defer cleanup()

	// Experimenter trying to archive (requires admin).
	expClient := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("exp@example.com", "experimenter"),
	)
	_, err := expClient.ArchiveExperiment(context.Background(), connect.NewRequest(&mgmtv1.ArchiveExperimentRequest{
		ExperimentId: "test-id",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodePermissionDenied, connect.CodeOf(err))

	// Admin should pass.
	adminClient := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("admin@example.com", "admin"),
	)
	_, err = adminClient.ArchiveExperiment(context.Background(), connect.NewRequest(&mgmtv1.ArchiveExperimentRequest{
		ExperimentId: "test-id",
	}))
	require.NoError(t, err)
}
