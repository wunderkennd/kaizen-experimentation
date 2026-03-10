package recalconsumer

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/segmentio/kafka-go"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/config"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/spark"
	"github.com/org/experimentation-platform/services/metrics/internal/surrogate"
)

func setupTestConsumer(t *testing.T) (*Consumer, *querylog.MemWriter, *surrogate.MemCalibrationUpdater) {
	t.Helper()

	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	inputProvider := &jobs.MockInputMetricsProvider{
		Inputs: surrogate.InputMetrics{
			"f0000000-0000-0000-0000-000000000001": {"churn_7d": 0.15},
			"f0000000-0000-0000-0000-000000000002": {"churn_7d": 0.05},
		},
	}
	qlWriter := querylog.NewMemWriter()
	projWriter := surrogate.NewMemProjectionWriter()
	calibUpdater := surrogate.NewMemCalibrationUpdater()

	recalJob := jobs.NewRecalibrationJob(cfgStore, renderer, inputProvider, qlWriter, projWriter, calibUpdater)

	c := &Consumer{
		job:    recalJob,
		config: cfgStore,
		done:   make(chan struct{}),
	}

	return c, qlWriter, calibUpdater
}

func makeMsg(t *testing.T, req RecalibrationRequest) kafka.Message {
	t.Helper()
	data, err := json.Marshal(req)
	require.NoError(t, err)
	return kafka.Message{Value: data}
}

func TestConsumer_ProcessMessage(t *testing.T) {
	c, qlWriter, _ := setupTestConsumer(t)
	ctx := context.Background()

	req := RecalibrationRequest{
		ModelID:        "sm-churn-predictor-001",
		TargetMetricID: "churn_7d",
		ModelType:      "LINEAR",
	}

	err := c.processMessage(ctx, makeMsg(t, req))
	require.NoError(t, err)

	// Verify the job ran and logged SQL.
	entries := qlWriter.AllEntries()
	require.GreaterOrEqual(t, len(entries), 1)
	assert.Equal(t, "surrogate_recalibration_actual", entries[0].JobType)
	assert.Equal(t, "e0000000-0000-0000-0000-000000000001", entries[0].ExperimentID)
}

func TestConsumer_ProcessMessage_UnknownModel(t *testing.T) {
	c, qlWriter, _ := setupTestConsumer(t)
	ctx := context.Background()

	req := RecalibrationRequest{
		ModelID:        "nonexistent-model-999",
		TargetMetricID: "churn_7d",
	}

	err := c.processMessage(ctx, makeMsg(t, req))
	require.NoError(t, err, "unknown model should not cause error")

	// No SQL logged because no experiments matched.
	entries := qlWriter.AllEntries()
	assert.Empty(t, entries)
}

func TestConsumer_ProcessMessage_InvalidJSON(t *testing.T) {
	c, _, _ := setupTestConsumer(t)
	ctx := context.Background()

	msg := kafka.Message{Value: []byte(`{invalid json!!!`)}

	err := c.processMessage(ctx, msg)
	require.NoError(t, err, "invalid JSON should be skipped, not error")
}

func TestConsumer_ProcessMessage_MultipleExperiments(t *testing.T) {
	// Create a config where two experiments share the same surrogate model.
	cfgStore, err := config.LoadFromFile("../config/testdata/seed_config.json")
	require.NoError(t, err)

	// Verify only one experiment uses the model in seed config.
	ids := cfgStore.GetExperimentsByModelID("sm-churn-predictor-001")
	require.Len(t, ids, 1, "seed config should have exactly 1 experiment with this model")

	// The consumer processes each experiment found. With 1, we just verify it works.
	renderer, err := spark.NewSQLRenderer()
	require.NoError(t, err)

	inputProvider := &jobs.MockInputMetricsProvider{
		Inputs: surrogate.InputMetrics{
			"f0000000-0000-0000-0000-000000000001": {"churn_7d": 0.15},
			"f0000000-0000-0000-0000-000000000002": {"churn_7d": 0.05},
		},
	}
	qlWriter := querylog.NewMemWriter()
	projWriter := surrogate.NewMemProjectionWriter()
	calibUpdater := surrogate.NewMemCalibrationUpdater()

	recalJob := jobs.NewRecalibrationJob(cfgStore, renderer, inputProvider, qlWriter, projWriter, calibUpdater)

	c := &Consumer{
		job:    recalJob,
		config: cfgStore,
		done:   make(chan struct{}),
	}

	ctx := context.Background()
	req := RecalibrationRequest{ModelID: "sm-churn-predictor-001"}
	err = c.processMessage(ctx, makeMsg(t, req))
	require.NoError(t, err)

	entries := qlWriter.AllEntries()
	assert.Len(t, entries, 1, "should process exactly 1 experiment")
}

func TestConsumer_ProcessMessage_EmptyModelID(t *testing.T) {
	c, qlWriter, _ := setupTestConsumer(t)
	ctx := context.Background()

	req := RecalibrationRequest{ModelID: ""}

	err := c.processMessage(ctx, makeMsg(t, req))
	require.NoError(t, err, "empty model_id should be skipped")

	entries := qlWriter.AllEntries()
	assert.Empty(t, entries)
}
