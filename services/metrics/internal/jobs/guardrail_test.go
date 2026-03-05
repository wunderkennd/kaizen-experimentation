package jobs

import (
	"context"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/alerts"
	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

func setupGuardrailTestJob(t *testing.T) (*GuardrailJob, *spark.MockExecutor, *querylog.MemWriter, *alerts.MemPublisher, *alerts.BreachTracker, *MockValueProvider) {
	t.Helper()
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)
	executor := spark.NewMockExecutor(500)
	qlWriter := querylog.NewMemWriter()
	publisher := alerts.NewMemPublisher()
	tracker := alerts.NewBreachTracker()
	vp := NewMockValueProvider()
	job := NewGuardrailJob(cfgStore, renderer, executor, qlWriter, publisher, tracker, vp)
	return job, executor, qlWriter, publisher, tracker, vp
}

func TestGuardrailJob_Run_CorrectSQL(t *testing.T) {
	job, executor, qlWriter, _, _, vp := setupGuardrailTestJob(t)
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03)
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.008)
	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	require.NoError(t, err)
	assert.Equal(t, 2, result.GuardrailsChecked)
	assert.Equal(t, 0, result.AlertsPublished)
	calls := executor.GetCalls()
	assert.Len(t, calls, 2)
	assert.Contains(t, calls[0].SQL, "qoe_rebuffer")
	assert.Contains(t, calls[1].SQL, "playback_error")
	entries := qlWriter.AllEntries()
	assert.Len(t, entries, 2)
	for _, e := range entries {
		assert.Equal(t, "hourly_guardrail", e.JobType)
	}
}

func TestGuardrailJob_Run_BreachDetected(t *testing.T) {
	job, _, _, publisher, _, vp := setupGuardrailTestJob(t)
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("error_rate", cv, 0.005)
	vp.SetVariantValue("error_rate", tv, 0.02) // breach: 0.02 > 0.01
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.03) // no breach: 0.03 < 0.05
	ctx := context.Background()
	r1, _ := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	assert.Equal(t, 0, r1.AlertsPublished)
	r2, _ := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	assert.Equal(t, 1, r2.AlertsPublished)
	a := publisher.Alerts()
	require.Len(t, a, 1)
	assert.Equal(t, "error_rate", a[0].MetricID)
	assert.Equal(t, 2, a[0].ConsecutiveBreachCount)
}

func TestGuardrailJob_Run_ConsecutiveBreachesRequired3(t *testing.T) {
	job, _, _, publisher, _, vp := setupGuardrailTestJob(t)
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.02)
	vp.SetVariantValue("rebuffer_rate", tv, 0.08) // breach: 0.08 > 0.05
	vp.SetVariantValue("error_rate", cv, 0.003)
	vp.SetVariantValue("error_rate", tv, 0.005) // no breach
	ctx := context.Background()
	r, _ := job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	assert.Equal(t, 0, r.AlertsPublished)
	r, _ = job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	assert.Equal(t, 0, r.AlertsPublished)
	r, _ = job.Run(ctx, "e0000000-0000-0000-0000-000000000001")
	assert.Equal(t, 1, r.AlertsPublished)
	assert.Equal(t, "rebuffer_rate", publisher.Alerts()[0].MetricID)
}

func TestGuardrailJob_Run_NoGuardrails(t *testing.T) {
	job, _, _, pub, _, _ := setupGuardrailTestJob(t)
	result, err := job.Run(context.Background(), "e0000000-0000-0000-0000-000000000003")
	require.NoError(t, err)
	assert.Equal(t, 0, result.GuardrailsChecked)
	assert.Len(t, pub.Alerts(), 0)
}

func TestGuardrailJob_Run_NotFound(t *testing.T) {
	job, _, _, _, _, _ := setupGuardrailTestJob(t)
	_, err := job.Run(context.Background(), "nonexistent")
	assert.Error(t, err)
}

func TestGuardrailJob_Run_SQLFields(t *testing.T) {
	job, executor, _, _, _, vp := setupGuardrailTestJob(t)
	cv, tv := "f0000000-0000-0000-0000-000000000001", "f0000000-0000-0000-0000-000000000002"
	vp.SetVariantValue("rebuffer_rate", cv, 0.01)
	vp.SetVariantValue("rebuffer_rate", tv, 0.02)
	vp.SetVariantValue("error_rate", cv, 0.001)
	vp.SetVariantValue("error_rate", tv, 0.002)
	_, _ = job.Run(context.Background(), "e0000000-0000-0000-0000-000000000001")
	for _, call := range executor.GetCalls() {
		assert.True(t, strings.Contains(call.SQL, "GROUP BY eu.variant_id"))
		assert.True(t, strings.Contains(call.SQL, "current_value"))
	}
}
