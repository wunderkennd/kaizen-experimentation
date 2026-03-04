package handler

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/orchestration/internal/querylog"
)

func setup() (*Handler, *querylog.MemWriter) {
	ql := querylog.NewMemWriter()
	h := New(ql, nil) // no postgres in tests
	return h, ql
}

func TestHealthz(t *testing.T) {
	h, _ := setup()
	req := httptest.NewRequest("GET", "/healthz", nil)
	w := httptest.NewRecorder()

	h.Healthz(w, req)

	assert.Equal(t, http.StatusOK, w.Code)
	assert.Equal(t, "ok", w.Body.String())
}

func TestReadyz_NoPg(t *testing.T) {
	h, _ := setup()
	req := httptest.NewRequest("GET", "/readyz", nil)
	w := httptest.NewRecorder()

	h.Readyz(w, req)

	assert.Equal(t, http.StatusOK, w.Code)
	assert.Equal(t, "ready", w.Body.String())
}

func TestPostQueryLog(t *testing.T) {
	h, ql := setup()

	body := `{
		"experiment_id": "exp-001",
		"metric_id": "metric_a",
		"sql_text": "SELECT avg(value) FROM events",
		"row_count": 500,
		"duration_ms": 120,
		"job_type": "daily_metric"
	}`

	req := httptest.NewRequest("POST", "/api/v1/query-log", bytes.NewBufferString(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	h.PostQueryLog(w, req)

	assert.Equal(t, http.StatusCreated, w.Code)

	entries := ql.AllEntries()
	require.Len(t, entries, 1)
	assert.Equal(t, "exp-001", entries[0].ExperimentID)
	assert.Equal(t, "metric_a", entries[0].MetricID)
	assert.Equal(t, int64(500), entries[0].RowCount)
}

func TestPostQueryLog_MissingRequired(t *testing.T) {
	h, _ := setup()

	body := `{"metric_id": "m1"}`
	req := httptest.NewRequest("POST", "/api/v1/query-log", bytes.NewBufferString(body))
	w := httptest.NewRecorder()

	h.PostQueryLog(w, req)

	assert.Equal(t, http.StatusBadRequest, w.Code)
}

func TestPostQueryLog_InvalidJSON(t *testing.T) {
	h, _ := setup()

	req := httptest.NewRequest("POST", "/api/v1/query-log", bytes.NewBufferString("not json"))
	w := httptest.NewRecorder()

	h.PostQueryLog(w, req)

	assert.Equal(t, http.StatusBadRequest, w.Code)
}

func TestGetQueryLog(t *testing.T) {
	h, ql := setup()

	// Seed some entries
	_ = ql.Log(nil, querylog.Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SQL1", JobType: "daily_metric"})
	_ = ql.Log(nil, querylog.Entry{ExperimentID: "exp-001", MetricID: "m2", SQLText: "SQL2", JobType: "hourly_guardrail"})
	_ = ql.Log(nil, querylog.Entry{ExperimentID: "exp-002", MetricID: "m3", SQLText: "SQL3", JobType: "daily_metric"})

	req := httptest.NewRequest("GET", "/api/v1/query-log?experiment_id=exp-001", nil)
	w := httptest.NewRecorder()

	h.GetQueryLog(w, req)

	assert.Equal(t, http.StatusOK, w.Code)
	assert.Equal(t, "application/json", w.Header().Get("Content-Type"))

	var resp queryLogResponse
	err := json.NewDecoder(w.Body).Decode(&resp)
	require.NoError(t, err)
	assert.Len(t, resp.Entries, 2)
}

func TestGetQueryLog_WithMetricFilter(t *testing.T) {
	h, ql := setup()

	_ = ql.Log(nil, querylog.Entry{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SQL1", JobType: "daily_metric"})
	_ = ql.Log(nil, querylog.Entry{ExperimentID: "exp-001", MetricID: "m2", SQLText: "SQL2", JobType: "daily_metric"})

	req := httptest.NewRequest("GET", "/api/v1/query-log?experiment_id=exp-001&metric_id=m1", nil)
	w := httptest.NewRecorder()

	h.GetQueryLog(w, req)

	assert.Equal(t, http.StatusOK, w.Code)

	var resp queryLogResponse
	err := json.NewDecoder(w.Body).Decode(&resp)
	require.NoError(t, err)
	assert.Len(t, resp.Entries, 1)
	assert.Equal(t, "m1", resp.Entries[0].MetricID)
}

func TestGetQueryLog_MissingExperimentID(t *testing.T) {
	h, _ := setup()

	req := httptest.NewRequest("GET", "/api/v1/query-log", nil)
	w := httptest.NewRecorder()

	h.GetQueryLog(w, req)

	assert.Equal(t, http.StatusBadRequest, w.Code)
}

func TestGetQueryLog_NoResults(t *testing.T) {
	h, _ := setup()

	req := httptest.NewRequest("GET", "/api/v1/query-log?experiment_id=nonexistent", nil)
	w := httptest.NewRecorder()

	h.GetQueryLog(w, req)

	assert.Equal(t, http.StatusOK, w.Code)

	var resp queryLogResponse
	err := json.NewDecoder(w.Body).Decode(&resp)
	require.NoError(t, err)
	assert.Empty(t, resp.Entries)
}
