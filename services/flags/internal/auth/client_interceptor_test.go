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

	"github.com/org/experimentation-platform/services/flags/internal/auth"
)

// captureHandler records headers received by a mock M5 management service.
type captureHandler struct {
	managementv1connect.UnimplementedExperimentManagementServiceHandler
	lastEmail string
	lastRole  string
}

func (h *captureHandler) CreateExperiment(_ context.Context, req *connect.Request[mgmtv1.CreateExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	h.lastEmail = req.Header().Get(auth.HeaderUserEmail)
	h.lastRole = req.Header().Get(auth.HeaderUserRole)
	return connect.NewResponse(&commonv1.Experiment{ExperimentId: "exp-capture"}), nil
}

func TestAuthForwardInterceptor_IdentityInContext(t *testing.T) {
	handler := &captureHandler{}
	mux := http.NewServeMux()
	path, h := managementv1connect.NewExperimentManagementServiceHandler(handler)
	mux.Handle(path, h)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient,
		server.URL,
		connect.WithInterceptors(auth.NewAuthForwardInterceptor()),
	)

	ctx := auth.WithIdentity(context.Background(), auth.Identity{
		Email: "alice@example.com",
		Role:  auth.RoleAdmin,
	})

	resp, err := client.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: &commonv1.Experiment{Name: "test"},
	}))
	require.NoError(t, err)
	assert.Equal(t, "exp-capture", resp.Msg.GetExperimentId())

	assert.Equal(t, "alice@example.com", handler.lastEmail)
	assert.Equal(t, "admin", handler.lastRole)
}

func TestAuthForwardInterceptor_NoIdentity(t *testing.T) {
	handler := &captureHandler{}
	mux := http.NewServeMux()
	path, h := managementv1connect.NewExperimentManagementServiceHandler(handler)
	mux.Handle(path, h)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	client := managementv1connect.NewExperimentManagementServiceClient(
		http.DefaultClient,
		server.URL,
		connect.WithInterceptors(auth.NewAuthForwardInterceptor()),
	)

	// No identity in context — DISABLE_AUTH scenario.
	resp, err := client.CreateExperiment(context.Background(), connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: &commonv1.Experiment{Name: "test"},
	}))
	require.NoError(t, err)
	assert.Equal(t, "exp-capture", resp.Msg.GetExperimentId())

	// No headers should be forwarded.
	assert.Empty(t, handler.lastEmail)
	assert.Empty(t, handler.lastRole)
}
