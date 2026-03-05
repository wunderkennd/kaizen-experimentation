package handlers

import (
	"errors"
	"fmt"
	"log/slog"
	"strings"

	"connectrpc.com/connect"
	"github.com/jackc/pgx/v5"
)

func notFoundError(entity, id string) *connect.Error {
	return connect.NewError(connect.CodeNotFound, fmt.Errorf("%s %q not found", entity, id))
}

func preconditionError(msg string) *connect.Error {
	return connect.NewError(connect.CodeFailedPrecondition, fmt.Errorf("%s", msg))
}

func internalError(msg string, err error) *connect.Error {
	slog.Error(msg, "error", err)
	return connect.NewError(connect.CodeInternal, fmt.Errorf("%s", msg))
}

// wrapDBError maps common pgx errors to appropriate gRPC status codes.
func wrapDBError(err error, entity, id string) *connect.Error {
	if err == nil {
		return nil
	}
	if errors.Is(err, pgx.ErrNoRows) {
		return notFoundError(entity, id)
	}
	// FK violation
	if strings.Contains(err.Error(), "violates foreign key constraint") {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("invalid reference: %s", err.Error()))
	}
	// Unique violation
	if strings.Contains(err.Error(), "violates unique constraint") {
		return connect.NewError(connect.CodeAlreadyExists,
			fmt.Errorf("already exists: %s", err.Error()))
	}
	return internalError("database error", err)
}
