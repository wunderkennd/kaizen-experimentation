package jobs

import (
	"context"
	"fmt"
	"log/slog"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

type GuardrailValueProvider interface {
	GetVariantValues(ctx context.Context, experimentID, metricID string) (map[string]float64, error)
}

type GuardrailJob struct {
	config        *config.ConfigStore
	renderer      *spark.SQLRenderer
	executor      spark.SQLExecutor
	queryLog      querylog.Writer
	publisher     alerts.Publisher
	tracker       *alerts.BreachTracker
	valueProvider GuardrailValueProvider
}

func NewGuardrailJob(cfg *config.ConfigStore, renderer *spark.SQLRenderer, executor spark.SQLExecutor, ql querylog.Writer, publisher alerts.Publisher, tracker *alerts.BreachTracker, vp GuardrailValueProvider) *GuardrailJob {
	return &GuardrailJob{config: cfg, renderer: renderer, executor: executor, queryLog: ql, publisher: publisher, tracker: tracker, valueProvider: vp}
}

type GuardrailResult struct {
	ExperimentID      string
	GuardrailsChecked int
	AlertsPublished   int
	CompletedAt       time.Time
}

func (j *GuardrailJob) Run(ctx context.Context, experimentID string) (*GuardrailResult, error) {
	exp, err := j.config.GetExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("guardrail: %w", err)
	}
	guardrails, err := j.config.GetGuardrailsForExperiment(experimentID)
	if err != nil {
		return nil, fmt.Errorf("guardrail: %w", err)
	}
	if len(guardrails) == 0 {
		return &GuardrailResult{ExperimentID: experimentID, CompletedAt: time.Now()}, nil
	}
	computationDate := time.Now().Format("2006-01-02")
	guardrailsChecked := 0
	alertsPublished := 0
	for _, gc := range guardrails {
		if err := ctx.Err(); err != nil {
			return nil, fmt.Errorf("guardrail: context cancelled: %w", err)
		}
		metricDef, err := j.config.GetMetric(gc.MetricID)
		if err != nil {
			return nil, fmt.Errorf("guardrail: resolve metric %s: %w", gc.MetricID, err)
		}
		params := spark.TemplateParams{
			ExperimentID: exp.ExperimentID, MetricID: gc.MetricID,
			SourceEventType: metricDef.SourceEventType, ComputationDate: computationDate,
		}
		sql, err := j.renderer.RenderGuardrailMetric(params)
		if err != nil {
			return nil, fmt.Errorf("guardrail: render SQL for %s: %w", gc.MetricID, err)
		}
		start := time.Now()
		result, err := j.executor.ExecuteAndWrite(ctx, sql, "delta.metric_summaries")
		if err != nil {
			return nil, fmt.Errorf("guardrail: execute SQL for %s: %w", gc.MetricID, err)
		}
		m3metrics.SparkQueryDuration.WithLabelValues("hourly_guardrail").Observe(result.Duration.Seconds())
		m3metrics.SparkQueryRows.WithLabelValues("hourly_guardrail").Observe(float64(result.RowCount))
		if err := j.queryLog.Log(ctx, querylog.Entry{
			ExperimentID: experimentID, MetricID: gc.MetricID, SQLText: sql,
			RowCount: result.RowCount, DurationMs: time.Since(start).Milliseconds(), JobType: "hourly_guardrail",
		}); err != nil {
			return nil, fmt.Errorf("guardrail: log query for %s: %w", gc.MetricID, err)
		}
		guardrailsChecked++
		variantValues, err := j.valueProvider.GetVariantValues(ctx, experimentID, gc.MetricID)
		if err != nil {
			return nil, fmt.Errorf("guardrail: read variant values for %s: %w", gc.MetricID, err)
		}
		for _, v := range exp.Variants {
			currentValue, ok := variantValues[v.VariantID]
			if !ok {
				continue
			}
			breached := isBreach(currentValue, gc.Threshold, metricDef.LowerIsBetter)
			count := j.tracker.RecordCheck(experimentID, gc.MetricID, v.VariantID, breached)
			if breached && count >= gc.ConsecutiveBreachesRequired {
				alert := alerts.GuardrailAlert{
					ExperimentID: experimentID, MetricID: gc.MetricID, VariantID: v.VariantID,
					CurrentValue: currentValue, Threshold: gc.Threshold,
					ConsecutiveBreachCount: count, DetectedAt: time.Now(),
				}
				if err := j.publisher.PublishAlert(ctx, alert); err != nil {
					return nil, fmt.Errorf("guardrail: publish alert for %s/%s: %w", gc.MetricID, v.VariantID, err)
				}
				m3metrics.GuardrailBreaches.WithLabelValues(experimentID, gc.MetricID, "alert").Inc()
				alertsPublished++
				slog.Warn("guardrail breach detected", "experiment_id", experimentID,
					"metric_id", gc.MetricID, "variant_id", v.VariantID,
					"current_value", currentValue, "threshold", gc.Threshold, "consecutive_breaches", count)
			}
		}
	}
	return &GuardrailResult{ExperimentID: experimentID, GuardrailsChecked: guardrailsChecked, AlertsPublished: alertsPublished, CompletedAt: time.Now()}, nil
}

func isBreach(currentValue, threshold float64, lowerIsBetter bool) bool {
	if lowerIsBetter {
		return currentValue > threshold
	}
	return currentValue < threshold
}
