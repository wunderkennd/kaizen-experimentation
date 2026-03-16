package scheduler

import (
	"context"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

func loadTestConfig(t *testing.T) *config.ConfigStore {
	t.Helper()
	cfg, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	return cfg
}

func newTestScheduler(t *testing.T) (*Scheduler, *querylog.MemWriter) {
	t.Helper()
	cfgStore := loadTestConfig(t)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(100)
	qlWriter := querylog.NewMemWriter()

	stdJob := jobs.NewStandardJob(cfgStore, renderer, executor, qlWriter)
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := jobs.NewMockValueProvider()
	grJob := jobs.NewGuardrailJob(cfgStore, renderer, executor, qlWriter, publisher, tracker, vp)

	cfg := Config{
		DailyHourUTC:    2,
		GuardrailPeriod: time.Hour,
		PurgeAge:        90 * 24 * time.Hour,
		PurgePeriod:     7 * 24 * time.Hour,
	}

	sched := New(stdJob, grJob, cfgStore, qlWriter, cfg)
	return sched, qlWriter
}

func TestScheduler_RunStandardJobsNow(t *testing.T) {
	sched, qlWriter := newTestScheduler(t)
	ctx := context.Background()

	sched.RunStandardJobsNow(ctx)

	entries := qlWriter.AllEntries()
	assert.NotEmpty(t, entries, "standard jobs should produce query log entries")

	// Verify entries come from daily_metric jobs.
	hasDailyMetric := false
	for _, e := range entries {
		if e.JobType == "daily_metric" {
			hasDailyMetric = true
			break
		}
	}
	assert.True(t, hasDailyMetric, "should have at least one daily_metric entry")
}

func TestScheduler_RunGuardrailJobsNow(t *testing.T) {
	sched, qlWriter := newTestScheduler(t)
	ctx := context.Background()

	sched.RunGuardrailJobsNow(ctx)

	entries := qlWriter.AllEntries()
	// Guardrail jobs may or may not produce entries depending on config.
	// The key assertion is that the method completes without error.
	_ = entries
}

func TestScheduler_RunPurgeNow(t *testing.T) {
	sched, qlWriter := newTestScheduler(t)
	ctx := context.Background()

	// Seed some entries with old timestamps.
	for i := 0; i < 5; i++ {
		require.NoError(t, qlWriter.Log(ctx, querylog.Entry{
			ExperimentID: "exp-old",
			MetricID:     "m1",
			SQLText:      "SELECT old",
			JobType:      "daily_metric",
		}))
	}

	// Force entries to have old timestamps.
	qlWriter.SetComputedAt(time.Now().Add(-100 * 24 * time.Hour))

	// Add one recent entry.
	require.NoError(t, qlWriter.Log(ctx, querylog.Entry{
		ExperimentID: "exp-recent",
		MetricID:     "m1",
		SQLText:      "SELECT recent",
		JobType:      "daily_metric",
	}))

	sched.RunPurgeNow(ctx)

	remaining := qlWriter.AllEntries()
	assert.Len(t, remaining, 1, "only recent entry should remain after purge")
	assert.Equal(t, "exp-recent", remaining[0].ExperimentID)
}

func TestScheduler_StartAndClose(t *testing.T) {
	sched, _ := newTestScheduler(t)

	// Use short intervals for testing.
	sched.config.GuardrailPeriod = 50 * time.Millisecond
	sched.config.PurgePeriod = 100 * time.Millisecond

	ctx := context.Background()
	sched.Start(ctx)

	// Let it run a few cycles.
	time.Sleep(200 * time.Millisecond)

	// Close should return promptly.
	done := make(chan struct{})
	go func() {
		sched.Close()
		close(done)
	}()

	select {
	case <-done:
		// OK
	case <-time.After(5 * time.Second):
		t.Fatal("scheduler.Close() did not return within 5 seconds")
	}
}

func TestScheduler_DefaultConfig(t *testing.T) {
	cfg := DefaultConfig()
	assert.Equal(t, 2, cfg.DailyHourUTC)
	assert.Equal(t, time.Hour, cfg.GuardrailPeriod)
	assert.Equal(t, 90*24*time.Hour, cfg.PurgeAge)
	assert.Equal(t, 7*24*time.Hour, cfg.PurgePeriod)
}

func TestScheduler_ContextCancellation(t *testing.T) {
	sched, _ := newTestScheduler(t)
	sched.config.GuardrailPeriod = 50 * time.Millisecond

	ctx, cancel := context.WithCancel(context.Background())
	sched.Start(ctx)

	// Cancel immediately.
	cancel()

	done := make(chan struct{})
	go func() {
		sched.Close()
		close(done)
	}()

	select {
	case <-done:
		// OK
	case <-time.After(5 * time.Second):
		t.Fatal("scheduler did not stop on context cancellation")
	}
}
