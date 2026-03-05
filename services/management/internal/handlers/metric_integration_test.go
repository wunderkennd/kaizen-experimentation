//go:build integration

package handlers_test

import (
	"context"
	"testing"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
)

func TestCreateMetricDefinition_Mean(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	resp, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
			Metric: &commonv1.MetricDefinition{
				Name:            "avg_watch_time",
				Description:     "Average watch time per session",
				Type:            commonv1.MetricType_METRIC_TYPE_MEAN,
				SourceEventType: "watch_event",
				LowerIsBetter:   false,
			},
		}))
	require.NoError(t, err)
	m := resp.Msg
	assert.NotEmpty(t, m.GetMetricId(), "metric_id should be auto-generated")
	assert.Equal(t, "avg_watch_time", m.GetName())
	assert.Equal(t, commonv1.MetricType_METRIC_TYPE_MEAN, m.GetType())
	assert.Equal(t, "watch_event", m.GetSourceEventType())
}

func TestCreateMetricDefinition_WithExplicitID(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	resp, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
			Metric: &commonv1.MetricDefinition{
				MetricId:        "custom-metric-001",
				Name:            "explicit_id_metric",
				Type:            commonv1.MetricType_METRIC_TYPE_COUNT,
				SourceEventType: "click",
			},
		}))
	require.NoError(t, err)
	assert.Equal(t, "custom-metric-001", resp.Msg.GetMetricId())
}

func TestCreateMetricDefinition_Ratio(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	resp, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
			Metric: &commonv1.MetricDefinition{
				Name:                 "revenue_per_session",
				Type:                 commonv1.MetricType_METRIC_TYPE_RATIO,
				NumeratorEventType:   "revenue",
				DenominatorEventType: "session",
			},
		}))
	require.NoError(t, err)
	assert.Equal(t, "revenue", resp.Msg.GetNumeratorEventType())
	assert.Equal(t, "session", resp.Msg.GetDenominatorEventType())
}

func TestCreateMetricDefinition_Percentile(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	resp, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
			Metric: &commonv1.MetricDefinition{
				Name:            "p95_ttff",
				Type:            commonv1.MetricType_METRIC_TYPE_PERCENTILE,
				SourceEventType: "ttff_event",
				Percentile:      0.95,
				LowerIsBetter:   true,
				IsQoeMetric:     true,
			},
		}))
	require.NoError(t, err)
	assert.InDelta(t, 0.95, resp.Msg.GetPercentile(), 0.001)
	assert.True(t, resp.Msg.GetLowerIsBetter())
	assert.True(t, resp.Msg.GetIsQoeMetric())
}

func TestCreateMetricDefinition_Custom(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	resp, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
			Metric: &commonv1.MetricDefinition{
				Name:      "custom_engagement",
				Type:      commonv1.MetricType_METRIC_TYPE_CUSTOM,
				CustomSql: "SELECT AVG(score) FROM engagement_events",
			},
		}))
	require.NoError(t, err)
	assert.Equal(t, "SELECT AVG(score) FROM engagement_events", resp.Msg.GetCustomSql())
}

func TestCreateMetricDefinition_ValidationErrors(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	tests := []struct {
		name   string
		metric *commonv1.MetricDefinition
		errMsg string
	}{
		{
			name:   "nil metric",
			metric: nil,
			errMsg: "metric is required",
		},
		{
			name:   "missing name",
			metric: &commonv1.MetricDefinition{Type: commonv1.MetricType_METRIC_TYPE_MEAN, SourceEventType: "e"},
			errMsg: "name is required",
		},
		{
			name:   "missing type",
			metric: &commonv1.MetricDefinition{Name: "m"},
			errMsg: "type is required",
		},
		{
			name:   "mean missing source_event",
			metric: &commonv1.MetricDefinition{Name: "m", Type: commonv1.MetricType_METRIC_TYPE_MEAN},
			errMsg: "source_event_type is required",
		},
		{
			name:   "ratio missing numerator",
			metric: &commonv1.MetricDefinition{Name: "m", Type: commonv1.MetricType_METRIC_TYPE_RATIO},
			errMsg: "numerator_event_type is required",
		},
		{
			name: "percentile out of range",
			metric: &commonv1.MetricDefinition{
				Name: "m", Type: commonv1.MetricType_METRIC_TYPE_PERCENTILE,
				SourceEventType: "e", Percentile: 1.5,
			},
			errMsg: "percentile must be in (0.0, 1.0)",
		},
		{
			name:   "custom missing sql",
			metric: &commonv1.MetricDefinition{Name: "m", Type: commonv1.MetricType_METRIC_TYPE_CUSTOM},
			errMsg: "custom_sql is required",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := env.client.CreateMetricDefinition(context.Background(),
				connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{Metric: tt.metric}))
			require.Error(t, err)
			assert.Contains(t, err.Error(), tt.errMsg)
		})
	}
}

func TestGetMetricDefinition(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	// Create first.
	createResp, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
			Metric: &commonv1.MetricDefinition{
				Name:            "get_test_metric",
				Type:            commonv1.MetricType_METRIC_TYPE_PROPORTION,
				SourceEventType: "conversion",
			},
		}))
	require.NoError(t, err)
	metricID := createResp.Msg.GetMetricId()

	// Get.
	getResp, err := env.client.GetMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.GetMetricDefinitionRequest{MetricId: metricID}))
	require.NoError(t, err)
	assert.Equal(t, metricID, getResp.Msg.GetMetricId())
	assert.Equal(t, "get_test_metric", getResp.Msg.GetName())
}

func TestGetMetricDefinition_NotFound(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	_, err := env.client.GetMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.GetMetricDefinitionRequest{MetricId: "nonexistent"}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeNotFound, connect.CodeOf(err))
}

func TestGetMetricDefinition_EmptyID(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	_, err := env.client.GetMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.GetMetricDefinitionRequest{MetricId: ""}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

func TestListMetricDefinitions(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	// Create 3 metrics.
	for i := 0; i < 3; i++ {
		_, err := env.client.CreateMetricDefinition(context.Background(),
			connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
				Metric: &commonv1.MetricDefinition{
					Name:            fmt.Sprintf("list_test_%d", i),
					Type:            commonv1.MetricType_METRIC_TYPE_COUNT,
					SourceEventType: "click",
				},
			}))
		require.NoError(t, err)
	}

	// List with page_size=2.
	listResp, err := env.client.ListMetricDefinitions(context.Background(),
		connect.NewRequest(&mgmtv1.ListMetricDefinitionsRequest{PageSize: 2}))
	require.NoError(t, err)
	// There may be seed data metrics too, so just check we got at least 2.
	assert.GreaterOrEqual(t, len(listResp.Msg.GetMetrics()), 2)

	// If there's a next_page_token, fetch next page.
	if listResp.Msg.GetNextPageToken() != "" {
		page2, err := env.client.ListMetricDefinitions(context.Background(),
			connect.NewRequest(&mgmtv1.ListMetricDefinitionsRequest{
				PageSize:  2,
				PageToken: listResp.Msg.GetNextPageToken(),
			}))
		require.NoError(t, err)
		assert.NotEmpty(t, page2.Msg.GetMetrics())
	}
}

func TestCreateMetricDefinition_DuplicateID(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	m := &commonv1.MetricDefinition{
		MetricId:        "dup-metric-id",
		Name:            "dup_metric",
		Type:            commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType: "ev",
	}

	_, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{Metric: m}))
	require.NoError(t, err)

	// Second insert with same ID should fail.
	_, err = env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{Metric: m}))
	require.Error(t, err)
	assert.Equal(t, connect.CodeAlreadyExists, connect.CodeOf(err))
}

func TestCreateMetricDefinition_WithCupedCovariate(t *testing.T) {
	env, cleanup := setupTestServer(t)
	defer cleanup()

	resp, err := env.client.CreateMetricDefinition(context.Background(),
		connect.NewRequest(&mgmtv1.CreateMetricDefinitionRequest{
			Metric: &commonv1.MetricDefinition{
				Name:                    "cuped_metric",
				Type:                    commonv1.MetricType_METRIC_TYPE_MEAN,
				SourceEventType:         "view",
				CupedCovariateMetricId:  "some-covariate-id",
				MinimumDetectableEffect: 0.02,
			},
		}))
	require.NoError(t, err)
	assert.Equal(t, "some-covariate-id", resp.Msg.GetCupedCovariateMetricId())
	assert.InDelta(t, 0.02, resp.Msg.GetMinimumDetectableEffect(), 0.001)
}
