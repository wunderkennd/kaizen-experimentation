package spark

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"math"
	"math/rand"
	"time"
)

// PermanentError marks an error that should not be retried (e.g., SQL syntax
// errors, missing tables). Wrap any non-transient error with this type so the
// RetryExecutor skips retries and returns immediately.
type PermanentError struct {
	Err error
}

func (e *PermanentError) Error() string {
	return e.Err.Error()
}

func (e *PermanentError) Unwrap() error {
	return e.Err
}

// IsPermanent checks whether err (or any error in its chain) is a PermanentError.
func IsPermanent(err error) bool {
	var pe *PermanentError
	return errors.As(err, &pe)
}

// RetryConfig controls the exponential backoff behaviour.
type RetryConfig struct {
	// MaxRetries is the maximum number of retry attempts (0 = no retries).
	MaxRetries int
	// BaseDelay is the delay before the first retry.
	BaseDelay time.Duration
	// MaxDelay caps the computed delay.
	MaxDelay time.Duration
	// JitterFraction adds randomness to the delay (0.0–1.0). The actual
	// jitter added is in [0, delay*JitterFraction).
	JitterFraction float64
}

// DefaultRetryConfig returns production-suitable defaults.
func DefaultRetryConfig() RetryConfig {
	return RetryConfig{
		MaxRetries:     3,
		BaseDelay:      1 * time.Second,
		MaxDelay:       30 * time.Second,
		JitterFraction: 0.2,
	}
}

// sleepFunc abstracts time.Sleep for testing.
type sleepFunc func(context.Context, time.Duration)

// RetryExecutor wraps a SQLExecutor with exponential backoff retry logic.
// It transparently retries transient errors while immediately propagating
// PermanentErrors and context cancellation.
type RetryExecutor struct {
	inner SQLExecutor
	cfg   RetryConfig
	sleep sleepFunc
}

// NewRetryExecutor creates a RetryExecutor for production use.
func NewRetryExecutor(inner SQLExecutor, cfg RetryConfig) *RetryExecutor {
	return &RetryExecutor{
		inner: inner,
		cfg:   cfg,
		sleep: func(ctx context.Context, d time.Duration) {
			t := time.NewTimer(d)
			defer t.Stop()
			select {
			case <-ctx.Done():
			case <-t.C:
			}
		},
	}
}

// NewRetryExecutorForTest creates a RetryExecutor with a no-op sleep for tests.
func NewRetryExecutorForTest(inner SQLExecutor, cfg RetryConfig) *RetryExecutor {
	return &RetryExecutor{
		inner: inner,
		cfg:   cfg,
		sleep: func(_ context.Context, _ time.Duration) {},
	}
}

func (r *RetryExecutor) ExecuteSQL(ctx context.Context, sql string) (*SQLResult, error) {
	return r.withRetry(ctx, "ExecuteSQL", sql, "", func() (*SQLResult, error) {
		return r.inner.ExecuteSQL(ctx, sql)
	})
}

func (r *RetryExecutor) ExecuteAndWrite(ctx context.Context, sql string, targetTable string) (*SQLResult, error) {
	return r.withRetry(ctx, "ExecuteAndWrite", sql, targetTable, func() (*SQLResult, error) {
		return r.inner.ExecuteAndWrite(ctx, sql, targetTable)
	})
}

func (r *RetryExecutor) withRetry(ctx context.Context, method, sql, targetTable string, fn func() (*SQLResult, error)) (*SQLResult, error) {
	sqlPrefix := sql
	if len(sqlPrefix) > 80 {
		sqlPrefix = sqlPrefix[:80] + "..."
	}

	var lastErr error
	for attempt := 0; attempt <= r.cfg.MaxRetries; attempt++ {
		// Check context before each attempt.
		if err := ctx.Err(); err != nil {
			return nil, err
		}

		result, err := fn()
		if err == nil {
			if attempt > 0 {
				slog.Info("retry: recovered after transient failure",
					"method", method,
					"attempt", attempt+1,
					"sql_prefix", sqlPrefix,
					"target_table", targetTable,
				)
			}
			return result, nil
		}

		// Never retry permanent errors or context cancellation.
		if IsPermanent(err) {
			return nil, err
		}
		if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			return nil, err
		}

		lastErr = err

		// If we have retries remaining, sleep with backoff.
		if attempt < r.cfg.MaxRetries {
			delay := r.backoffDelay(attempt)
			slog.Warn("retry: transient failure, will retry",
				"method", method,
				"attempt", attempt+1,
				"max_retries", r.cfg.MaxRetries,
				"delay", delay,
				"error", err,
				"sql_prefix", sqlPrefix,
				"target_table", targetTable,
			)
			r.sleep(ctx, delay)
		}
	}

	return nil, fmt.Errorf("exhausted %d retries for %s: %w", r.cfg.MaxRetries, method, lastErr)
}

// backoffDelay computes base * 2^attempt, capped at MaxDelay, with jitter.
func (r *RetryExecutor) backoffDelay(attempt int) time.Duration {
	delay := float64(r.cfg.BaseDelay) * math.Pow(2, float64(attempt))
	if delay > float64(r.cfg.MaxDelay) {
		delay = float64(r.cfg.MaxDelay)
	}
	if r.cfg.JitterFraction > 0 {
		jitter := delay * r.cfg.JitterFraction * rand.Float64()
		delay += jitter
	}
	return time.Duration(delay)
}
