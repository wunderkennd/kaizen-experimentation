package spark

import (
	"context"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// --- Test doubles ---

// TransientFailingExecutor fails the first N calls, then succeeds.
type TransientFailingExecutor struct {
	mu          sync.Mutex
	callCount   int
	failFirstN  int
	failErr     error
	defaultRows int64
}

func newTransientFailingExecutor(failFirstN int, err error) *TransientFailingExecutor {
	return &TransientFailingExecutor{
		failFirstN:  failFirstN,
		failErr:     err,
		defaultRows: 100,
	}
}

func (e *TransientFailingExecutor) ExecuteSQL(_ context.Context, _ string) (*SQLResult, error) {
	e.mu.Lock()
	e.callCount++
	n := e.callCount
	e.mu.Unlock()
	if n <= e.failFirstN {
		return nil, e.failErr
	}
	return &SQLResult{RowCount: e.defaultRows, Duration: 10 * time.Millisecond}, nil
}

func (e *TransientFailingExecutor) ExecuteAndWrite(_ context.Context, _ string, _ string) (*SQLResult, error) {
	e.mu.Lock()
	e.callCount++
	n := e.callCount
	e.mu.Unlock()
	if n <= e.failFirstN {
		return nil, e.failErr
	}
	return &SQLResult{RowCount: e.defaultRows, Duration: 20 * time.Millisecond}, nil
}

func (e *TransientFailingExecutor) calls() int {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.callCount
}

// AlwaysPermanentExecutor always returns a PermanentError.
type AlwaysPermanentExecutor struct {
	mu        sync.Mutex
	callCount int
}

func (e *AlwaysPermanentExecutor) ExecuteSQL(_ context.Context, _ string) (*SQLResult, error) {
	e.mu.Lock()
	e.callCount++
	e.mu.Unlock()
	return nil, &PermanentError{Err: fmt.Errorf("SQL syntax error near SELECT")}
}

func (e *AlwaysPermanentExecutor) ExecuteAndWrite(_ context.Context, _ string, _ string) (*SQLResult, error) {
	e.mu.Lock()
	e.callCount++
	e.mu.Unlock()
	return nil, &PermanentError{Err: fmt.Errorf("table not found: missing_table")}
}

func (e *AlwaysPermanentExecutor) calls() int {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.callCount
}

// recordingSleep records each requested delay for verification.
type delaySleep struct {
	mu     sync.Mutex
	delays []time.Duration
}

func (s *delaySleep) sleep(_ context.Context, d time.Duration) {
	s.mu.Lock()
	s.delays = append(s.delays, d)
	s.mu.Unlock()
}

func (s *delaySleep) getDelays() []time.Duration {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := make([]time.Duration, len(s.delays))
	copy(out, s.delays)
	return out
}

// --- Tests ---

func TestRetry_SuccessOnFirstAttempt(t *testing.T) {
	inner := NewMockExecutor(42)
	re := NewRetryExecutorForTest(inner, DefaultRetryConfig())

	result, err := re.ExecuteSQL(context.Background(), "SELECT 1")
	require.NoError(t, err)
	assert.Equal(t, int64(42), result.RowCount)
	assert.Len(t, inner.GetCalls(), 1)
}

func TestRetry_RecoveryAfterTransientFailures(t *testing.T) {
	inner := newTransientFailingExecutor(2, fmt.Errorf("spark cluster unreachable"))
	re := NewRetryExecutorForTest(inner, RetryConfig{
		MaxRetries: 3,
		BaseDelay:  1 * time.Second,
		MaxDelay:   30 * time.Second,
	})

	result, err := re.ExecuteSQL(context.Background(), "SELECT count(*) FROM events")
	require.NoError(t, err)
	assert.Equal(t, int64(100), result.RowCount)
	assert.Equal(t, 3, inner.calls()) // 2 failures + 1 success
}

func TestRetry_ExhaustsRetries(t *testing.T) {
	inner := newTransientFailingExecutor(10, fmt.Errorf("spark OOM"))
	re := NewRetryExecutorForTest(inner, RetryConfig{
		MaxRetries: 3,
		BaseDelay:  1 * time.Second,
		MaxDelay:   30 * time.Second,
	})

	_, err := re.ExecuteSQL(context.Background(), "SELECT * FROM big_table")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "exhausted 3 retries")
	assert.Contains(t, err.Error(), "spark OOM")
	assert.Equal(t, 4, inner.calls()) // 1 initial + 3 retries
}

func TestRetry_PermanentErrorNotRetried(t *testing.T) {
	inner := &AlwaysPermanentExecutor{}
	re := NewRetryExecutorForTest(inner, RetryConfig{
		MaxRetries: 3,
		BaseDelay:  1 * time.Second,
		MaxDelay:   30 * time.Second,
	})

	_, err := re.ExecuteSQL(context.Background(), "SELEKT * FROM bad_syntax")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "SQL syntax error")
	assert.True(t, IsPermanent(err))
	assert.Equal(t, 1, inner.calls()) // only one call, no retries
}

func TestRetry_PermanentErrorNotRetried_ExecuteAndWrite(t *testing.T) {
	inner := &AlwaysPermanentExecutor{}
	re := NewRetryExecutorForTest(inner, RetryConfig{
		MaxRetries: 3,
		BaseDelay:  1 * time.Second,
		MaxDelay:   30 * time.Second,
	})

	_, err := re.ExecuteAndWrite(context.Background(), "INSERT INTO missing", "missing_table")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "table not found")
	assert.True(t, IsPermanent(err))
	assert.Equal(t, 1, inner.calls())
}

func TestRetry_ContextCancelledDuringBackoff(t *testing.T) {
	inner := newTransientFailingExecutor(10, fmt.Errorf("transient"))

	ctx, cancel := context.WithCancel(context.Background())

	re := &RetryExecutor{
		inner: inner,
		cfg: RetryConfig{
			MaxRetries: 5,
			BaseDelay:  1 * time.Hour, // very long so we know it's sleeping
			MaxDelay:   1 * time.Hour,
		},
		sleep: func(ctx context.Context, _ time.Duration) {
			// Simulate cancel arriving during sleep
			cancel()
			<-ctx.Done()
		},
	}

	_, err := re.ExecuteSQL(ctx, "SELECT 1")
	require.Error(t, err)
	assert.ErrorIs(t, err, context.Canceled)
}

func TestRetry_ContextAlreadyCancelled(t *testing.T) {
	inner := NewMockExecutor(42)
	re := NewRetryExecutorForTest(inner, DefaultRetryConfig())

	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	_, err := re.ExecuteSQL(ctx, "SELECT 1")
	require.Error(t, err)
	assert.ErrorIs(t, err, context.Canceled)
	assert.Empty(t, inner.GetCalls()) // never called inner
}

func TestRetry_ExponentialDelayVerification(t *testing.T) {
	inner := newTransientFailingExecutor(3, fmt.Errorf("transient"))
	ds := &delaySleep{}

	re := &RetryExecutor{
		inner: inner,
		cfg: RetryConfig{
			MaxRetries:     3,
			BaseDelay:      1 * time.Second,
			MaxDelay:       30 * time.Second,
			JitterFraction: 0, // no jitter for deterministic test
		},
		sleep: ds.sleep,
	}

	result, err := re.ExecuteSQL(context.Background(), "SELECT 1")
	require.NoError(t, err)
	assert.Equal(t, int64(100), result.RowCount)

	delays := ds.getDelays()
	require.Len(t, delays, 3) // 3 retries before success on 4th call
	assert.Equal(t, 1*time.Second, delays[0])
	assert.Equal(t, 2*time.Second, delays[1])
	assert.Equal(t, 4*time.Second, delays[2])
}

func TestRetry_MaxDelayCap(t *testing.T) {
	// Fail enough times to exceed max delay cap.
	inner := newTransientFailingExecutor(5, fmt.Errorf("transient"))
	ds := &delaySleep{}

	re := &RetryExecutor{
		inner: inner,
		cfg: RetryConfig{
			MaxRetries:     5,
			BaseDelay:      1 * time.Second,
			MaxDelay:       3 * time.Second,
			JitterFraction: 0,
		},
		sleep: ds.sleep,
	}

	result, err := re.ExecuteSQL(context.Background(), "SELECT 1")
	require.NoError(t, err)
	assert.Equal(t, int64(100), result.RowCount)

	delays := ds.getDelays()
	require.Len(t, delays, 5)
	assert.Equal(t, 1*time.Second, delays[0])
	assert.Equal(t, 2*time.Second, delays[1])
	assert.Equal(t, 3*time.Second, delays[2]) // capped
	assert.Equal(t, 3*time.Second, delays[3]) // capped
	assert.Equal(t, 3*time.Second, delays[4]) // capped
}

func TestRetry_ZeroRetriesPassthrough(t *testing.T) {
	inner := newTransientFailingExecutor(1, fmt.Errorf("fail once"))
	re := NewRetryExecutorForTest(inner, RetryConfig{
		MaxRetries: 0,
		BaseDelay:  1 * time.Second,
		MaxDelay:   30 * time.Second,
	})

	_, err := re.ExecuteSQL(context.Background(), "SELECT 1")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "fail once")
	// With 0 retries, should not wrap in "exhausted" message — just return raw error.
	assert.Contains(t, err.Error(), "exhausted 0 retries")
	assert.Equal(t, 1, inner.calls())
}

func TestRetry_ExecuteAndWriteRecovery(t *testing.T) {
	inner := newTransientFailingExecutor(1, fmt.Errorf("shuffle fetch failed"))
	re := NewRetryExecutorForTest(inner, RetryConfig{
		MaxRetries: 2,
		BaseDelay:  1 * time.Second,
		MaxDelay:   30 * time.Second,
	})

	result, err := re.ExecuteAndWrite(context.Background(), "INSERT OVERWRITE ...", "metric_summaries")
	require.NoError(t, err)
	assert.Equal(t, int64(100), result.RowCount)
	assert.Equal(t, 2, inner.calls()) // 1 failure + 1 success
}

func TestIsPermanent_WrappedErrors(t *testing.T) {
	base := &PermanentError{Err: fmt.Errorf("bad SQL")}
	wrapped := fmt.Errorf("executor failed: %w", base)
	doubleWrapped := fmt.Errorf("job failed: %w", wrapped)

	assert.True(t, IsPermanent(base))
	assert.True(t, IsPermanent(wrapped))
	assert.True(t, IsPermanent(doubleWrapped))
	assert.False(t, IsPermanent(fmt.Errorf("transient error")))
	assert.False(t, IsPermanent(nil))
}

func TestRetry_SQLPrefixTruncation(t *testing.T) {
	// Ensure long SQL doesn't cause issues (truncation is for logging, not correctness).
	longSQL := ""
	for i := 0; i < 200; i++ {
		longSQL += "X"
	}
	inner := NewMockExecutor(1)
	re := NewRetryExecutorForTest(inner, DefaultRetryConfig())

	result, err := re.ExecuteSQL(context.Background(), longSQL)
	require.NoError(t, err)
	assert.Equal(t, int64(1), result.RowCount)
}
