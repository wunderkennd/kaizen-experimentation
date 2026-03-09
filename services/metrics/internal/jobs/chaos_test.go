package jobs

import (
	"context"
	"fmt"
	"sync/atomic"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

// --- Failing test doubles ---

// FailingExecutor is a SQLExecutor that fails after N successful calls.
type FailingExecutor struct {
	callCount    atomic.Int64
	failAfterN   int64
	failErr      error
	defaultRows  int64
}

func NewFailingExecutor(failAfterN int64, err error) *FailingExecutor {
	return &FailingExecutor{
		failAfterN:  failAfterN,
		failErr:     err,
		defaultRows: 500,
	}
}

func (e *FailingExecutor) ExecuteSQL(ctx context.Context, sql string) (*spark.SQLResult, error) {
	n := e.callCount.Add(1)
	if n > e.failAfterN {
		return nil, e.failErr
	}
	return &spark.SQLResult{RowCount: e.defaultRows, Duration: 50 * time.Millisecond}, nil
}

func (e *FailingExecutor) ExecuteAndWrite(ctx context.Context, sql string, targetTable string) (*spark.SQLResult, error) {
	n := e.callCount.Add(1)
	if n > e.failAfterN {
		return nil, e.failErr
	}
	return &spark.SQLResult{RowCount: e.defaultRows, Duration: 100 * time.Millisecond}, nil
}

func (e *FailingExecutor) CallCount() int64 {
	return e.callCount.Load()
}

// FailingWriter is a querylog.Writer that fails after N successful writes.
type FailingWriter struct {
	callCount  atomic.Int64
	failAfterN int64
	failErr    error
}

func NewFailingWriter(failAfterN int64, err error) *FailingWriter {
	return &FailingWriter{failAfterN: failAfterN, failErr: err}
}

func (w *FailingWriter) Log(_ context.Context, _ querylog.Entry) error {
	n := w.callCount.Add(1)
	if n > w.failAfterN {
		return w.failErr
	}
	return nil
}

func (w *FailingWriter) GetLogs(_ context.Context, _ string, _ string) ([]querylog.Entry, error) {
	return nil, nil
}

func (w *FailingWriter) CallCount() int64 {
	return w.callCount.Load()
}

// FailingPublisher is an alerts.Publisher that fails after N successful publishes.
type FailingPublisher struct {
	callCount  atomic.Int64
	failAfterN int64
	failErr    error
}

func NewFailingPublisher(failAfterN int64, err error) *FailingPublisher {
	return &FailingPublisher{failAfterN: failAfterN, failErr: err}
}

func (p *FailingPublisher) PublishAlert(_ context.Context, _ alerts.GuardrailAlert) error {
	n := p.callCount.Add(1)
	if n > p.failAfterN {
		return p.failErr
	}
	return nil
}

// FailingValueProvider fails on GetVariantValues.
type FailingValueProvider struct {
	failErr error
}

func (p *FailingValueProvider) GetVariantValues(_ context.Context, _, _ string) (map[string]float64, error) {
	return nil, p.failErr
}

// --- Helper ---

func loadTestConfig(t *testing.T) *config.ConfigStore {
	t.Helper()
	cfg, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	return cfg
}

func newRenderer(t *testing.T) *spark.SQLRenderer {
	t.Helper()
	r, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	return r
}

// =====================
// StandardJob resilience
// =====================

func TestChaos_StandardJob_ExecutorFailsOnFirstCall(t *testing.T) {
	executor := NewFailingExecutor(0, fmt.Errorf("spark cluster unreachable"))
	job := NewStandardJob(loadTestConfig(t), newRenderer(t), executor, querylog.NewMemWriter())

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "spark cluster unreachable")
	assert.Contains(t, err.Error(), "jobs: execute metric")
	// No partial state: executor was called exactly once before failure.
	assert.Equal(t, int64(1), executor.CallCount())
}

func TestChaos_StandardJob_ExecutorFailsMidJob(t *testing.T) {
	// Allow 2 successful calls (first metric value + CUPED covariate), then fail.
	executor := NewFailingExecutor(2, fmt.Errorf("spark OOM"))
	job := NewStandardJob(loadTestConfig(t), newRenderer(t), executor, querylog.NewMemWriter())

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "spark OOM")
	// Should have attempted exactly 3 calls (2 ok + 1 failed).
	assert.Equal(t, int64(3), executor.CallCount())
}

func TestChaos_StandardJob_QueryLogFailsOnFirstWrite(t *testing.T) {
	qlWriter := NewFailingWriter(0, fmt.Errorf("postgres connection refused"))
	executor := spark.NewMockExecutor(500)
	job := NewStandardJob(loadTestConfig(t), newRenderer(t), executor, qlWriter)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "postgres connection refused")
	assert.Contains(t, err.Error(), "jobs: log query")
}

func TestChaos_StandardJob_QueryLogFailsMidJob(t *testing.T) {
	// Allow first metric's query log to succeed, fail on second.
	qlWriter := NewFailingWriter(1, fmt.Errorf("postgres disk full"))
	executor := spark.NewMockExecutor(500)
	job := NewStandardJob(loadTestConfig(t), newRenderer(t), executor, qlWriter)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "postgres disk full")
}

func TestChaos_StandardJob_ContextCancelled(t *testing.T) {
	// Cancel context immediately — the job should still fail since
	// the executor/querylog calls proceed without checking ctx.Err() directly
	// in the StandardJob loop (only GuardrailJob checks). The mock executor
	// will succeed, but we cancel before calling Run.
	ctx, cancel := context.WithCancel(context.Background())
	cancel() // cancel immediately

	executor := spark.NewMockExecutor(500)
	job := NewStandardJob(loadTestConfig(t), newRenderer(t), executor, querylog.NewMemWriter())

	// Run with cancelled context — behavior depends on whether the executor
	// respects context (mock does not). This verifies no panic/hang occurs.
	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	// MockExecutor doesn't check ctx, so Run completes. The important thing
	// is that it doesn't hang or panic.
	if err != nil {
		assert.Contains(t, err.Error(), "context canceled")
	}
}

func TestChaos_StandardJob_ContextTimeout(t *testing.T) {
	// Very short timeout — ensures the job doesn't hang.
	ctx, cancel := context.WithTimeout(context.Background(), 1*time.Millisecond)
	defer cancel()
	time.Sleep(2 * time.Millisecond) // ensure timeout triggers

	executor := spark.NewMockExecutor(500)
	job := NewStandardJob(loadTestConfig(t), newRenderer(t), executor, querylog.NewMemWriter())

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	// Should not hang — mock executor ignores context, so we may succeed.
	// The key invariant is no deadlock or panic.
	_ = err
}

// =====================
// GuardrailJob resilience
// =====================

func TestChaos_GuardrailJob_ExecutorFails(t *testing.T) {
	cfg := loadTestConfig(t)
	executor := NewFailingExecutor(0, fmt.Errorf("spark session expired"))
	qlWriter := querylog.NewMemWriter()
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := NewMockValueProvider()
	job := NewGuardrailJob(cfg, newRenderer(t), executor, qlWriter, publisher, tracker, vp)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "spark session expired")
	// No alerts should be published on executor failure.
	assert.Empty(t, publisher.Alerts())
}

func TestChaos_GuardrailJob_QueryLogFails(t *testing.T) {
	cfg := loadTestConfig(t)
	executor := spark.NewMockExecutor(500)
	qlWriter := NewFailingWriter(0, fmt.Errorf("postgres timeout"))
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := NewMockValueProvider()
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03)
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.008)
	job := NewGuardrailJob(cfg, newRenderer(t), executor, qlWriter, publisher, tracker, vp)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "postgres timeout")
	// No alerts on query log failure — the error occurs before breach detection.
	assert.Empty(t, publisher.Alerts())
}

func TestChaos_GuardrailJob_PublisherFails(t *testing.T) {
	cfg := loadTestConfig(t)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	publisher := NewFailingPublisher(0, fmt.Errorf("kafka broker down"))
	tracker := alerts.NewBreachTracker()
	vp := NewMockValueProvider()
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	// Set values that will breach: error_rate has threshold 0.01, consecutive_breaches=2
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.02) // breach
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03) // no breach
	job := NewGuardrailJob(cfg, newRenderer(t), executor, qlWriter, publisher, tracker, vp)

	// First run: breach count = 1 (below threshold of 2), no publish.
	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)

	// Second run: breach count = 2 → publish attempted → fails.
	_, err = job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "kafka broker down")
	assert.Contains(t, err.Error(), "guardrail: publish alert")
}

func TestChaos_GuardrailJob_ValueProviderFails(t *testing.T) {
	cfg := loadTestConfig(t)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := &FailingValueProvider{failErr: fmt.Errorf("delta lake read timeout")}
	job := NewGuardrailJob(cfg, newRenderer(t), executor, qlWriter, publisher, tracker, vp)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "delta lake read timeout")
	assert.Contains(t, err.Error(), "guardrail: read variant values")
}

func TestChaos_GuardrailJob_ContextCancelled(t *testing.T) {
	cfg := loadTestConfig(t)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := NewMockValueProvider()
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03)
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.008)
	job := NewGuardrailJob(cfg, newRenderer(t), executor, qlWriter, publisher, tracker, vp)

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // cancel before Run

	_, err := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "context cancelled")
	// No alerts should be published.
	assert.Empty(t, publisher.Alerts())
}

// =====================
// InterleavingJob resilience
// =====================

func TestChaos_InterleavingJob_ExecutorFails(t *testing.T) {
	executor := NewFailingExecutor(0, fmt.Errorf("spark driver crash"))
	qlWriter := querylog.NewMemWriter()
	job := NewInterleavingJob(loadTestConfig(t), newRenderer(t), executor, qlWriter)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000003")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "spark driver crash")
}

func TestChaos_InterleavingJob_QueryLogFails(t *testing.T) {
	executor := spark.NewMockExecutor(300)
	qlWriter := NewFailingWriter(0, fmt.Errorf("postgres connection reset"))
	job := NewInterleavingJob(loadTestConfig(t), newRenderer(t), executor, qlWriter)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000003")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "postgres connection reset")
}

// =====================
// ContentConsumptionJob resilience
// =====================

func TestChaos_ContentConsumptionJob_ExecutorFails(t *testing.T) {
	executor := NewFailingExecutor(0, fmt.Errorf("spark shuffle failed"))
	qlWriter := querylog.NewMemWriter()
	job := NewContentConsumptionJob(loadTestConfig(t), newRenderer(t), executor, qlWriter)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "spark shuffle failed")
}

func TestChaos_ContentConsumptionJob_QueryLogFails(t *testing.T) {
	executor := spark.NewMockExecutor(500)
	qlWriter := NewFailingWriter(0, fmt.Errorf("postgres max connections"))
	job := NewContentConsumptionJob(loadTestConfig(t), newRenderer(t), executor, qlWriter)

	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "postgres max connections")
}

// =====================
// Cross-cutting: error propagation
// =====================

func TestChaos_ErrorPropagation_NoSilentSwallowing(t *testing.T) {
	// Verify that all jobs propagate errors — none silently swallow failures.
	sentinelErr := fmt.Errorf("sentinel-error-abc123")
	cfg := loadTestConfig(t)
	renderer := newRenderer(t)

	tests := []struct {
		name string
		run  func() error
	}{
		{
			name: "StandardJob/executor",
			run: func() error {
				j := NewStandardJob(cfg, renderer, NewFailingExecutor(0, sentinelErr), querylog.NewMemWriter())
				_, err := j.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
				return err
			},
		},
		{
			name: "StandardJob/querylog",
			run: func() error {
				j := NewStandardJob(cfg, renderer, spark.NewMockExecutor(500), NewFailingWriter(0, sentinelErr))
				_, err := j.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
				return err
			},
		},
		{
			name: "InterleavingJob/executor",
			run: func() error {
				j := NewInterleavingJob(cfg, renderer, NewFailingExecutor(0, sentinelErr), querylog.NewMemWriter())
				_, err := j.Run(context.Background(), "e0000000-0000-0000-0000-000000000003")
				return err
			},
		},
		{
			name: "InterleavingJob/querylog",
			run: func() error {
				j := NewInterleavingJob(cfg, renderer, spark.NewMockExecutor(300), NewFailingWriter(0, sentinelErr))
				_, err := j.Run(context.Background(), "e0000000-0000-0000-0000-000000000003")
				return err
			},
		},
		{
			name: "ContentConsumptionJob/executor",
			run: func() error {
				j := NewContentConsumptionJob(cfg, renderer, NewFailingExecutor(0, sentinelErr), querylog.NewMemWriter())
				_, err := j.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
				return err
			},
		},
		{
			name: "ContentConsumptionJob/querylog",
			run: func() error {
				j := NewContentConsumptionJob(cfg, renderer, spark.NewMockExecutor(500), NewFailingWriter(0, sentinelErr))
				_, err := j.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
				return err
			},
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.run()
			require.Error(t, err, "error must propagate, not be swallowed")
			assert.ErrorContains(t, err, "sentinel-error-abc123",
				"original error must be preserved in the chain")
		})
	}
}

// =====================
// Idempotency: repeated failures don't corrupt state
// =====================

func TestChaos_RepeatedFailures_NoStateCorruption(t *testing.T) {
	cfg := loadTestConfig(t)
	renderer := newRenderer(t)

	// Run StandardJob with a failing executor 5 times — each should fail cleanly.
	for i := 0; i < 5; i++ {
		executor := NewFailingExecutor(0, fmt.Errorf("transient error attempt %d", i))
		qlWriter := querylog.NewMemWriter()
		job := NewStandardJob(cfg, renderer, executor, qlWriter)

		_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
		require.Error(t, err)
		// No query log entries should exist after failed run.
		assert.Empty(t, qlWriter.AllEntries(), "no partial writes on failure")
	}

	// Now run with a working executor — should succeed as if nothing happened.
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	job := NewStandardJob(cfg, renderer, executor, qlWriter)

	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	assert.Equal(t, 4, result.MetricsComputed)
	assert.NotEmpty(t, qlWriter.AllEntries())
}

// =====================
// Retry recovery tests
// =====================

func TestChaos_StandardJob_RecoversFromTransientSparkFailure(t *testing.T) {
	// The inner executor fails twice, then succeeds. The retry wrapper recovers.
	inner := NewFailingExecutor(0, fmt.Errorf("spark cluster unreachable"))
	// Override: allow up to callCount=2 to fail, then succeed.
	// We use a TransientExecutor that fails first N calls per SQL method.
	transient := &transientChaosExecutor{failFirstN: 2, failErr: fmt.Errorf("spark cluster unreachable"), defaultRows: 500}
	retryExec := spark.NewRetryExecutorForTest(transient, spark.RetryConfig{
		MaxRetries:     3,
		BaseDelay:      1 * time.Second,
		MaxDelay:       30 * time.Second,
		JitterFraction: 0,
	})
	_ = inner // unused, we use transient instead

	job := NewStandardJob(loadTestConfig(t), newRenderer(t), retryExec, querylog.NewMemWriter())
	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	assert.Equal(t, 4, result.MetricsComputed)
}

func TestChaos_StandardJob_PermanentErrorNotRetried(t *testing.T) {
	inner := &permanentChaosExecutor{}
	retryExec := spark.NewRetryExecutorForTest(inner, spark.RetryConfig{
		MaxRetries:     3,
		BaseDelay:      1 * time.Second,
		MaxDelay:       30 * time.Second,
		JitterFraction: 0,
	})

	job := NewStandardJob(loadTestConfig(t), newRenderer(t), retryExec, querylog.NewMemWriter())
	_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "SQL syntax error")
	assert.True(t, spark.IsPermanent(err))
	// Only 1 call — no retries for permanent errors.
	assert.Equal(t, 1, inner.calls())
}

// transientChaosExecutor fails the first N calls per method, then succeeds.
type transientChaosExecutor struct {
	callCount   atomic.Int64
	failFirstN  int64
	failErr     error
	defaultRows int64
}

func (e *transientChaosExecutor) ExecuteSQL(_ context.Context, _ string) (*spark.SQLResult, error) {
	n := e.callCount.Add(1)
	if n <= e.failFirstN {
		return nil, e.failErr
	}
	return &spark.SQLResult{RowCount: e.defaultRows, Duration: 50 * time.Millisecond}, nil
}

func (e *transientChaosExecutor) ExecuteAndWrite(_ context.Context, _ string, _ string) (*spark.SQLResult, error) {
	n := e.callCount.Add(1)
	if n <= e.failFirstN {
		return nil, e.failErr
	}
	return &spark.SQLResult{RowCount: e.defaultRows, Duration: 100 * time.Millisecond}, nil
}

// permanentChaosExecutor always returns a PermanentError.
type permanentChaosExecutor struct {
	callCount atomic.Int64
}

func (e *permanentChaosExecutor) ExecuteSQL(_ context.Context, _ string) (*spark.SQLResult, error) {
	e.callCount.Add(1)
	return nil, &spark.PermanentError{Err: fmt.Errorf("SQL syntax error")}
}

func (e *permanentChaosExecutor) ExecuteAndWrite(_ context.Context, _ string, _ string) (*spark.SQLResult, error) {
	e.callCount.Add(1)
	return nil, &spark.PermanentError{Err: fmt.Errorf("SQL syntax error")}
}

func (e *permanentChaosExecutor) calls() int {
	return int(e.callCount.Load())
}

func TestChaos_GuardrailJob_BreachTrackerSurvivesFailures(t *testing.T) {
	cfg := loadTestConfig(t)
	renderer := newRenderer(t)
	tracker := alerts.NewBreachTracker()
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"

	// Run 1: executor works, values breach. Count → 1.
	{
		executor := spark.NewMockExecutor(500)
		qlWriter := querylog.NewMemWriter()
		publisher := alerts.NewMemPublisher()
		vp := NewMockValueProvider()
		vp.SetVariantValue("error_rate", cv, 0.005)
		vp.SetVariantValue("error_rate", tv, 0.02) // breach
		vp.SetVariantValue("rebuffer_rate", cv, 0.02)
		vp.SetVariantValue("rebuffer_rate", tv, 0.03)
		job := NewGuardrailJob(cfg, renderer, executor, qlWriter, publisher, tracker, vp)
		r, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Equal(t, 0, r.AlertsPublished) // threshold is 2
	}

	// Run 2: executor fails mid-job. Breach tracker state from run 1 persists.
	{
		executor := NewFailingExecutor(0, fmt.Errorf("spark crash"))
		qlWriter := querylog.NewMemWriter()
		publisher := alerts.NewMemPublisher()
		vp := NewMockValueProvider()
		job := NewGuardrailJob(cfg, renderer, executor, qlWriter, publisher, tracker, vp)
		_, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
		require.Error(t, err)
	}

	// Run 3: executor works again, values still breach. Count → 2 → alert fires.
	{
		executor := spark.NewMockExecutor(500)
		qlWriter := querylog.NewMemWriter()
		publisher := alerts.NewMemPublisher()
		vp := NewMockValueProvider()
		vp.SetVariantValue("error_rate", cv, 0.005)
		vp.SetVariantValue("error_rate", tv, 0.02) // still breaching
		vp.SetVariantValue("rebuffer_rate", cv, 0.02)
		vp.SetVariantValue("rebuffer_rate", tv, 0.03)
		job := NewGuardrailJob(cfg, renderer, executor, qlWriter, publisher, tracker, vp)
		r, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
		require.NoError(t, err)
		assert.Equal(t, 1, r.AlertsPublished)
		assert.Equal(t, "error_rate", publisher.Alerts()[0].MetricID)
		assert.Equal(t, 2, publisher.Alerts()[0].ConsecutiveBreachCount)
	}
}
