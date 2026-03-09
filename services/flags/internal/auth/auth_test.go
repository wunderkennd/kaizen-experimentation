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
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"

	"github.com/org/experimentation-platform/services/flags/internal/auth"
)

// --- Identity unit tests ---

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

// --- Stub flag service for interceptor tests ---

type stubFlagService struct {
	flagsv1connect.UnimplementedFeatureFlagServiceHandler
	capturedCtx context.Context
}

func (s *stubFlagService) GetFlag(ctx context.Context, _ *connect.Request[flagsv1.GetFlagRequest]) (*connect.Response[flagsv1.Flag], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&flagsv1.Flag{FlagId: "test-flag"}), nil
}

func (s *stubFlagService) ListFlags(ctx context.Context, _ *connect.Request[flagsv1.ListFlagsRequest]) (*connect.Response[flagsv1.ListFlagsResponse], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&flagsv1.ListFlagsResponse{}), nil
}

func (s *stubFlagService) EvaluateFlag(ctx context.Context, _ *connect.Request[flagsv1.EvaluateFlagRequest]) (*connect.Response[flagsv1.EvaluateFlagResponse], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&flagsv1.EvaluateFlagResponse{Value: "true"}), nil
}

func (s *stubFlagService) EvaluateFlags(ctx context.Context, _ *connect.Request[flagsv1.EvaluateFlagsRequest]) (*connect.Response[flagsv1.EvaluateFlagsResponse], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&flagsv1.EvaluateFlagsResponse{}), nil
}

func (s *stubFlagService) CreateFlag(ctx context.Context, _ *connect.Request[flagsv1.CreateFlagRequest]) (*connect.Response[flagsv1.Flag], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&flagsv1.Flag{FlagId: "new-flag"}), nil
}

func (s *stubFlagService) UpdateFlag(ctx context.Context, _ *connect.Request[flagsv1.UpdateFlagRequest]) (*connect.Response[flagsv1.Flag], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&flagsv1.Flag{FlagId: "updated-flag"}), nil
}

func (s *stubFlagService) PromoteToExperiment(ctx context.Context, _ *connect.Request[flagsv1.PromoteToExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	s.capturedCtx = ctx
	return connect.NewResponse(&commonv1.Experiment{ExperimentId: "exp-1"}), nil
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

func setupStubServer(t *testing.T) (*stubFlagService, string) {
	t.Helper()
	stub := &stubFlagService{}
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(stub,
		connect.WithInterceptors(auth.NewAuthInterceptor()),
	)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	return stub, server.URL
}

// --- Interceptor tests ---

func TestInterceptor_ValidRequest(t *testing.T) {
	stub, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("alice@example.com", "viewer"),
	)

	_, err := client.GetFlag(context.Background(), connect.NewRequest(&flagsv1.GetFlagRequest{
		FlagId: "test-id",
	}))
	require.NoError(t, err)

	id, err := auth.FromContext(stub.capturedCtx)
	require.NoError(t, err)
	assert.Equal(t, "alice@example.com", id.Email)
	assert.Equal(t, auth.RoleViewer, id.Role)
}

func TestInterceptor_MissingEmail(t *testing.T) {
	_, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
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

	_, err := client.GetFlag(context.Background(), connect.NewRequest(&flagsv1.GetFlagRequest{
		FlagId: "test-id",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}

func TestInterceptor_MissingRole(t *testing.T) {
	_, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
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

	_, err := client.GetFlag(context.Background(), connect.NewRequest(&flagsv1.GetFlagRequest{
		FlagId: "test-id",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}

func TestInterceptor_InvalidRole(t *testing.T) {
	_, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("alice@example.com", "superadmin"),
	)

	_, err := client.GetFlag(context.Background(), connect.NewRequest(&flagsv1.GetFlagRequest{
		FlagId: "test-id",
	}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeUnauthenticated, connect.CodeOf(err))
}

// --- Permission enforcement tests ---

func TestPermission_ViewerCanGetAndEvaluate(t *testing.T) {
	_, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("viewer@example.com", "viewer"),
	)

	_, err := client.GetFlag(context.Background(), connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: "f1"}))
	assert.NoError(t, err)

	_, err = client.ListFlags(context.Background(), connect.NewRequest(&flagsv1.ListFlagsRequest{}))
	assert.NoError(t, err)

	_, err = client.EvaluateFlag(context.Background(), connect.NewRequest(&flagsv1.EvaluateFlagRequest{FlagId: "f1", UserId: "u1"}))
	assert.NoError(t, err)

	_, err = client.EvaluateFlags(context.Background(), connect.NewRequest(&flagsv1.EvaluateFlagsRequest{UserId: "u1"}))
	assert.NoError(t, err)
}

func TestPermission_ViewerCannotCreate(t *testing.T) {
	_, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("viewer@example.com", "viewer"),
	)

	_, err := client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{}))
	require.Error(t, err)
	assert.Equal(t, connect.CodePermissionDenied, connect.CodeOf(err))

	_, err = client.UpdateFlag(context.Background(), connect.NewRequest(&flagsv1.UpdateFlagRequest{}))
	require.Error(t, err)
	assert.Equal(t, connect.CodePermissionDenied, connect.CodeOf(err))

	_, err = client.PromoteToExperiment(context.Background(), connect.NewRequest(&flagsv1.PromoteToExperimentRequest{}))
	require.Error(t, err)
	assert.Equal(t, connect.CodePermissionDenied, connect.CodeOf(err))
}

func TestPermission_ExperimenterCanCreateAndUpdate(t *testing.T) {
	_, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("exp@example.com", "experimenter"),
	)

	_, err := client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{}))
	assert.NoError(t, err)

	_, err = client.UpdateFlag(context.Background(), connect.NewRequest(&flagsv1.UpdateFlagRequest{}))
	assert.NoError(t, err)

	// Experimenter cannot promote (requires admin).
	_, err = client.PromoteToExperiment(context.Background(), connect.NewRequest(&flagsv1.PromoteToExperimentRequest{}))
	require.Error(t, err)
	assert.Equal(t, connect.CodePermissionDenied, connect.CodeOf(err))
}

func TestPermission_AdminCanPromote(t *testing.T) {
	_, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("admin@example.com", "admin"),
	)

	// Admin can do everything.
	_, err := client.GetFlag(context.Background(), connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: "f1"}))
	assert.NoError(t, err)

	_, err = client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{}))
	assert.NoError(t, err)

	_, err = client.UpdateFlag(context.Background(), connect.NewRequest(&flagsv1.UpdateFlagRequest{}))
	assert.NoError(t, err)

	_, err = client.PromoteToExperiment(context.Background(), connect.NewRequest(&flagsv1.PromoteToExperimentRequest{}))
	assert.NoError(t, err)
}

// --- Audit actor integration test ---

func TestInterceptor_ActorExtraction(t *testing.T) {
	stub, url := setupStubServer(t)

	client := flagsv1connect.NewFeatureFlagServiceClient(
		http.DefaultClient, url,
		withAuthHeaders("deploy-bot@corp.com", "admin"),
	)

	_, err := client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{}))
	require.NoError(t, err)

	id, err := auth.FromContext(stub.capturedCtx)
	require.NoError(t, err)
	assert.Equal(t, "deploy-bot@corp.com", id.Email)
	assert.Equal(t, auth.RoleAdmin, id.Role)
}

// --- DISABLE_AUTH test: server without interceptor ---

func TestNoInterceptor_AllRPCsWork(t *testing.T) {
	stub := &stubFlagService{}
	mux := http.NewServeMux()
	// No interceptor — simulates DISABLE_AUTH=true.
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(stub)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	client := flagsv1connect.NewFeatureFlagServiceClient(http.DefaultClient, server.URL)

	_, err := client.GetFlag(context.Background(), connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: "f1"}))
	assert.NoError(t, err)

	_, err = client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{}))
	assert.NoError(t, err)

	_, err = client.PromoteToExperiment(context.Background(), connect.NewRequest(&flagsv1.PromoteToExperimentRequest{}))
	assert.NoError(t, err)
}
