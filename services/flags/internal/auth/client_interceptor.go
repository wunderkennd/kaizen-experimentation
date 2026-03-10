package auth

import (
	"context"

	"connectrpc.com/connect"
)

// NewAuthForwardInterceptor returns a ConnectRPC client interceptor that reads
// the Identity from the context (placed there by the server-side auth interceptor)
// and forwards the X-User-Email and X-User-Role headers on outgoing requests.
//
// When no identity is present in the context (e.g. DISABLE_AUTH mode), no headers
// are added — the downstream service is expected to also have auth disabled.
func NewAuthForwardInterceptor() connect.UnaryInterceptorFunc {
	return func(next connect.UnaryFunc) connect.UnaryFunc {
		return func(ctx context.Context, req connect.AnyRequest) (connect.AnyResponse, error) {
			id, err := FromContext(ctx)
			if err == nil {
				req.Header().Set(HeaderUserEmail, id.Email)
				req.Header().Set(HeaderUserRole, string(id.Role))
			}
			return next(ctx, req)
		}
	}
}
