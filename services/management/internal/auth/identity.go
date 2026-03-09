// Package auth provides identity extraction, role-based access control,
// and ConnectRPC interceptors for the management service.
package auth

import (
	"context"
	"fmt"

	"connectrpc.com/connect"
)

// Role represents a user's authorization level.
type Role string

const (
	RoleViewer       Role = "viewer"
	RoleAnalyst      Role = "analyst"
	RoleExperimenter Role = "experimenter"
	RoleAdmin        Role = "admin"
)

// roleRank maps each role to a numeric rank for hierarchical comparison.
var roleRank = map[Role]int{
	RoleViewer:       0,
	RoleAnalyst:      1,
	RoleExperimenter: 2,
	RoleAdmin:        3,
}

// ParseRole validates a role string and returns the corresponding Role.
func ParseRole(s string) (Role, error) {
	r := Role(s)
	if _, ok := roleRank[r]; !ok {
		return "", fmt.Errorf("invalid role %q: must be one of viewer, analyst, experimenter, admin", s)
	}
	return r, nil
}

// HasAtLeast returns true if this role meets or exceeds the minimum role.
func (r Role) HasAtLeast(minimum Role) bool {
	return roleRank[r] >= roleRank[minimum]
}

// Identity represents an authenticated user extracted from API gateway headers.
type Identity struct {
	Email string
	Role  Role
}

type contextKey struct{}

// WithIdentity stores an Identity in the context.
func WithIdentity(ctx context.Context, id Identity) context.Context {
	return context.WithValue(ctx, contextKey{}, id)
}

// FromContext retrieves the Identity from the context.
// Returns a connect.CodeUnauthenticated error if no identity is present.
func FromContext(ctx context.Context) (Identity, error) {
	id, ok := ctx.Value(contextKey{}).(Identity)
	if !ok {
		return Identity{}, connect.NewError(connect.CodeUnauthenticated, fmt.Errorf("no identity in context"))
	}
	return id, nil
}
