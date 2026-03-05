package spark

import (
	"context"
	"time"
)

// SQLResult holds the outcome of executing a SQL query.
type SQLResult struct {
	RowCount int64
	Duration time.Duration
}

// SQLExecutor abstracts SQL execution against a Spark cluster or test backend.
type SQLExecutor interface {
	// ExecuteSQL runs SQL and returns metadata about the result.
	ExecuteSQL(ctx context.Context, sql string) (*SQLResult, error)

	// ExecuteAndWrite runs SQL and writes results to the target Delta table.
	ExecuteAndWrite(ctx context.Context, sql string, targetTable string) (*SQLResult, error)
}
