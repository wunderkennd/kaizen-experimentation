// Package handler provides HTTP handlers for the orchestration service.
// Includes health/readiness probes and query log retrieval endpoints.
package handler

import (
	"encoding/json"
	"log/slog"
	"net/http"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/org/experimentation-platform/services/orchestration/internal/querylog"
)

// Handler holds dependencies for HTTP endpoints.
type Handler struct {
	queryLog querylog.Writer
	pgPool   *pgxpool.Pool // nil when running without PostgreSQL
}

// New creates a new Handler with the given dependencies.
func New(ql querylog.Writer, pool *pgxpool.Pool) *Handler {
	return &Handler{
		queryLog: ql,
		pgPool:   pool,
	}
}

// Register registers all routes on the given mux.
func (h *Handler) Register(mux *http.ServeMux) {
	mux.HandleFunc("GET /healthz", h.Healthz)
	mux.HandleFunc("GET /readyz", h.Readyz)
	mux.HandleFunc("GET /api/v1/query-log", h.GetQueryLog)
	mux.HandleFunc("POST /api/v1/query-log", h.PostQueryLog)
}

// Healthz is a liveness probe — always returns 200 if the process is running.
func (h *Handler) Healthz(w http.ResponseWriter, _ *http.Request) {
	w.WriteHeader(http.StatusOK)
	w.Write([]byte("ok"))
}

// Readyz is a readiness probe — returns 200 only if dependencies are healthy.
func (h *Handler) Readyz(w http.ResponseWriter, r *http.Request) {
	if h.pgPool != nil {
		if err := h.pgPool.Ping(r.Context()); err != nil {
			slog.Warn("readiness check failed: postgres unreachable", "error", err)
			http.Error(w, "postgres unreachable", http.StatusServiceUnavailable)
			return
		}
	}
	w.WriteHeader(http.StatusOK)
	w.Write([]byte("ready"))
}

// queryLogResponse is the JSON response for GET /api/v1/query-log.
type queryLogResponse struct {
	Entries []queryLogEntry `json:"entries"`
}

type queryLogEntry struct {
	ExperimentID string `json:"experiment_id"`
	MetricID     string `json:"metric_id"`
	SQLText      string `json:"sql_text"`
	RowCount     int64  `json:"row_count"`
	DurationMs   int64  `json:"duration_ms"`
	JobType      string `json:"job_type"`
	ComputedAt   string `json:"computed_at"`
}

// GetQueryLog returns query log entries filtered by experiment_id and optional metric_id.
// Query params: experiment_id (required), metric_id (optional).
func (h *Handler) GetQueryLog(w http.ResponseWriter, r *http.Request) {
	experimentID := r.URL.Query().Get("experiment_id")
	if experimentID == "" {
		http.Error(w, `{"error":"experiment_id query parameter is required"}`, http.StatusBadRequest)
		return
	}
	metricID := r.URL.Query().Get("metric_id")

	entries, err := h.queryLog.GetLogs(r.Context(), experimentID, metricID)
	if err != nil {
		slog.Error("failed to get query logs", "error", err, "experiment_id", experimentID)
		http.Error(w, `{"error":"internal error"}`, http.StatusInternalServerError)
		return
	}

	resp := queryLogResponse{
		Entries: make([]queryLogEntry, 0, len(entries)),
	}
	for _, e := range entries {
		resp.Entries = append(resp.Entries, queryLogEntry{
			ExperimentID: e.ExperimentID,
			MetricID:     e.MetricID,
			SQLText:      e.SQLText,
			RowCount:     e.RowCount,
			DurationMs:   e.DurationMs,
			JobType:      e.JobType,
			ComputedAt:   e.ComputedAt.Format("2006-01-02T15:04:05Z"),
		})
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}

// postQueryLogRequest is the JSON request for POST /api/v1/query-log.
type postQueryLogRequest struct {
	ExperimentID string `json:"experiment_id"`
	MetricID     string `json:"metric_id"`
	SQLText      string `json:"sql_text"`
	RowCount     int64  `json:"row_count"`
	DurationMs   int64  `json:"duration_ms"`
	JobType      string `json:"job_type"`
}

// PostQueryLog logs a new SQL query entry.
func (h *Handler) PostQueryLog(w http.ResponseWriter, r *http.Request) {
	var req postQueryLogRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, `{"error":"invalid JSON body"}`, http.StatusBadRequest)
		return
	}

	if req.ExperimentID == "" || req.SQLText == "" || req.JobType == "" {
		http.Error(w, `{"error":"experiment_id, sql_text, and job_type are required"}`, http.StatusBadRequest)
		return
	}

	entry := querylog.Entry{
		ExperimentID: req.ExperimentID,
		MetricID:     req.MetricID,
		SQLText:      req.SQLText,
		RowCount:     req.RowCount,
		DurationMs:   req.DurationMs,
		JobType:      req.JobType,
	}

	if err := h.queryLog.Log(r.Context(), entry); err != nil {
		slog.Error("failed to log query", "error", err)
		http.Error(w, `{"error":"internal error"}`, http.StatusInternalServerError)
		return
	}

	w.WriteHeader(http.StatusCreated)
	w.Write([]byte(`{"status":"logged"}`))
}
