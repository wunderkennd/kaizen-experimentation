// Package scheduler provides automated periodic execution of metric computation
// jobs. It runs daily StandardJob, hourly GuardrailJob, and weekly log purge.
package scheduler

import (
	"context"
	"log/slog"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
)

// Config controls scheduling intervals.
type Config struct {
	DailyHourUTC    int           // Hour (0-23) to run daily standard jobs. Default: 2.
	GuardrailPeriod time.Duration // Period between guardrail runs. Default: 1h.
	PurgeAge        time.Duration // Delete logs older than this. Default: 90 days.
	PurgePeriod     time.Duration // Period between purge runs. Default: 7 days (weekly).
}

// DefaultConfig returns production-ready scheduling defaults.
func DefaultConfig() Config {
	return Config{
		DailyHourUTC:    2,
		GuardrailPeriod: time.Hour,
		PurgeAge:        90 * 24 * time.Hour,
		PurgePeriod:     7 * 24 * time.Hour,
	}
}

// Scheduler orchestrates periodic metric computation and log maintenance.
type Scheduler struct {
	standardJob  *jobs.StandardJob
	guardrailJob *jobs.GuardrailJob
	cfgStore     *config.ConfigStore
	queryLog     querylog.Writer
	config       Config
	done         chan struct{}
	cancel       context.CancelFunc
}

// New creates a Scheduler with the given dependencies and configuration.
func New(stdJob *jobs.StandardJob, grJob *jobs.GuardrailJob, cfgStore *config.ConfigStore, ql querylog.Writer, cfg Config) *Scheduler {
	return &Scheduler{
		standardJob:  stdJob,
		guardrailJob: grJob,
		cfgStore:     cfgStore,
		queryLog:     ql,
		config:       cfg,
		done:         make(chan struct{}),
	}
}

// Start begins the scheduling goroutine. Call Close to stop.
func (s *Scheduler) Start(ctx context.Context) {
	ctx, s.cancel = context.WithCancel(ctx)
	go s.run(ctx)
}

// Close signals the scheduler to stop and waits for it to finish.
func (s *Scheduler) Close() {
	if s.cancel != nil {
		s.cancel()
	}
	<-s.done
}

func (s *Scheduler) run(ctx context.Context) {
	defer close(s.done)

	guardrailTicker := time.NewTicker(s.config.GuardrailPeriod)
	defer guardrailTicker.Stop()

	purgeTicker := time.NewTicker(s.config.PurgePeriod)
	defer purgeTicker.Stop()

	// Daily ticker: check once per minute whether it's the target hour.
	dailyCheckTicker := time.NewTicker(time.Minute)
	defer dailyCheckTicker.Stop()
	// Use a sentinel with Day() == 2 so the first check on the 1st of any month still triggers.
	// time.Time{} has Day() == 1 which would cause a false "already ran today" on the 1st.
	lastDailyRun := time.Date(1970, 1, 2, 0, 0, 0, 0, time.UTC)

	slog.Info("scheduler: started",
		"daily_hour_utc", s.config.DailyHourUTC,
		"guardrail_period", s.config.GuardrailPeriod,
		"purge_period", s.config.PurgePeriod,
		"purge_age", s.config.PurgeAge)

	for {
		select {
		case <-ctx.Done():
			slog.Info("scheduler: shutting down")
			return

		case now := <-dailyCheckTicker.C:
			if now.UTC().Hour() == s.config.DailyHourUTC && now.UTC().Day() != lastDailyRun.UTC().Day() {
				lastDailyRun = now
				s.runStandardJobs(ctx)
			}

		case <-guardrailTicker.C:
			s.runGuardrailJobs(ctx)

		case <-purgeTicker.C:
			s.runPurge(ctx)
		}
	}
}

func (s *Scheduler) runStandardJobs(ctx context.Context) {
	expIDs := s.cfgStore.RunningExperimentIDs()
	slog.Info("scheduler: starting daily standard jobs", "experiment_count", len(expIDs))
	m3metrics.SchedulerLastRun.WithLabelValues("standard").SetToCurrentTime()

	for _, expID := range expIDs {
		if ctx.Err() != nil {
			return
		}
		_, err := s.standardJob.Run(ctx, expID)
		if err != nil {
			slog.Error("scheduler: standard job failed", "experiment_id", expID, "error", err)
			m3metrics.SchedulerRuns.WithLabelValues("standard", "error").Inc()
			continue
		}
		m3metrics.SchedulerRuns.WithLabelValues("standard", "ok").Inc()
		m3metrics.SchedulerExperimentsProcessed.WithLabelValues("standard").Inc()
	}
	slog.Info("scheduler: daily standard jobs complete", "experiment_count", len(expIDs))
}

func (s *Scheduler) runGuardrailJobs(ctx context.Context) {
	expIDs := s.cfgStore.RunningExperimentIDs()
	m3metrics.SchedulerLastRun.WithLabelValues("guardrail").SetToCurrentTime()

	for _, expID := range expIDs {
		if ctx.Err() != nil {
			return
		}
		_, err := s.guardrailJob.Run(ctx, expID)
		if err != nil {
			slog.Error("scheduler: guardrail job failed", "experiment_id", expID, "error", err)
			m3metrics.SchedulerRuns.WithLabelValues("guardrail", "error").Inc()
			continue
		}
		m3metrics.SchedulerRuns.WithLabelValues("guardrail", "ok").Inc()
		m3metrics.SchedulerExperimentsProcessed.WithLabelValues("guardrail").Inc()
	}
}

func (s *Scheduler) runPurge(ctx context.Context) {
	cutoff := time.Now().Add(-s.config.PurgeAge)
	m3metrics.SchedulerLastRun.WithLabelValues("purge").SetToCurrentTime()

	purged, err := s.queryLog.PurgeOldLogs(ctx, cutoff)
	if err != nil {
		slog.Error("scheduler: purge failed", "error", err)
		m3metrics.SchedulerRuns.WithLabelValues("purge", "error").Inc()
		return
	}
	m3metrics.SchedulerRuns.WithLabelValues("purge", "ok").Inc()
	if purged > 0 {
		slog.Info("scheduler: purged old query logs", "purged_count", purged, "cutoff", cutoff)
	}
}

// RunStandardJobsNow executes standard jobs immediately (for testing).
func (s *Scheduler) RunStandardJobsNow(ctx context.Context) {
	s.runStandardJobs(ctx)
}

// RunGuardrailJobsNow executes guardrail jobs immediately (for testing).
func (s *Scheduler) RunGuardrailJobsNow(ctx context.Context) {
	s.runGuardrailJobs(ctx)
}

// RunPurgeNow executes log purge immediately (for testing).
func (s *Scheduler) RunPurgeNow(ctx context.Context) {
	s.runPurge(ctx)
}
