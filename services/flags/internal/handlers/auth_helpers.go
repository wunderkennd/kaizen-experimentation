package handlers

import (
	"context"

	"github.com/org/experimentation-platform/services/flags/internal/auth"
)

// actorFromContext extracts the authenticated user's email from the context.
// Returns "system" if no identity is present (e.g., internal paths
// that bypass the RPC interceptor).
func actorFromContext(ctx context.Context) string {
	id, err := auth.FromContext(ctx)
	if err != nil {
		return "system"
	}
	return id.Email
}
