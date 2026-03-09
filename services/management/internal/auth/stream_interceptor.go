package auth

import (
	"context"
	"log/slog"

	"connectrpc.com/connect"
)

// streamAuthInterceptor implements connect.Interceptor and extracts identity
// from headers when present on streaming RPCs. It is permissive: requests
// without identity headers (e.g., service-to-service calls like M1→M5
// StreamConfigUpdates) are allowed through without an identity in context.
//
// For unary RPCs it is a no-op — use NewAuthInterceptor for unary enforcement.
type streamAuthInterceptor struct{}

// NewStreamAuthInterceptor returns a connect.Interceptor that permissively
// extracts identity from streaming requests.
func NewStreamAuthInterceptor() connect.Interceptor {
	return &streamAuthInterceptor{}
}

func (s *streamAuthInterceptor) WrapUnary(next connect.UnaryFunc) connect.UnaryFunc {
	return next
}

func (s *streamAuthInterceptor) WrapStreamingClient(next connect.StreamingClientFunc) connect.StreamingClientFunc {
	return next
}

func (s *streamAuthInterceptor) WrapStreamingHandler(next connect.StreamingHandlerFunc) connect.StreamingHandlerFunc {
	return func(ctx context.Context, conn connect.StreamingHandlerConn) error {
		email := conn.RequestHeader().Get(HeaderUserEmail)
		roleStr := conn.RequestHeader().Get(HeaderUserRole)

		if email == "" || roleStr == "" {
			slog.Debug("streaming request without identity headers",
				"procedure", conn.Spec().Procedure,
				"has_email", email != "",
				"has_role", roleStr != "",
			)
			return next(ctx, conn)
		}

		role, err := ParseRole(roleStr)
		if err != nil {
			slog.Debug("streaming request with invalid role",
				"procedure", conn.Spec().Procedure,
				"role", roleStr,
			)
			return next(ctx, conn)
		}

		ctx = WithIdentity(ctx, Identity{Email: email, Role: role})
		return next(ctx, conn)
	}
}
