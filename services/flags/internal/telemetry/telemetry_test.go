package telemetry

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestInit_ReturnsMetrics(t *testing.T) {
	// No OTLP endpoint set — trace exporter skipped, Prometheus exporter still works.
	t.Setenv("OTEL_EXPORTER_OTLP_ENDPOINT", "")

	m, cleanup, err := Init(context.Background())
	require.NoError(t, err)
	defer cleanup()

	assert.NotNil(t, m)
	assert.NotNil(t, m.FlagEvaluationsTotal)
	assert.NotNil(t, m.FlagPromotionsTotal)
	assert.NotNil(t, m.ReconcilerRunsTotal)
	assert.NotNil(t, m.ReconcilerDuration)
}

func TestPrometheusHandler_ServesMetrics(t *testing.T) {
	t.Setenv("OTEL_EXPORTER_OTLP_ENDPOINT", "")

	_, cleanup, err := Init(context.Background())
	require.NoError(t, err)
	defer cleanup()

	handler := PrometheusHandler()
	req := httptest.NewRequest(http.MethodGet, "/metrics", nil)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	assert.Equal(t, http.StatusOK, rec.Code)

	body, err := io.ReadAll(rec.Body)
	require.NoError(t, err)
	// Prometheus exporter always emits at least process/go runtime metrics.
	assert.Contains(t, string(body), "# HELP")
}
