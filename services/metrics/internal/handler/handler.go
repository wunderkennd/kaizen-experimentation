package handler

import (
	"context"
	"fmt"
	"time"

	"connectrpc.com/connect"
	"google.golang.org/protobuf/types/known/timestamppb"

	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"
	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	"github.com/org/experimentation-platform/services/metrics/internal/catalog"
	"github.com/org/experimentation-platform/services/metrics/internal/export"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
	"github.com/org/experimentation-platform/services/metrics/internal/shadow"
)

var _ metricsv1connect.MetricComputationServiceHandler = (*MetricsHandler)(nil)

type MetricsHandler struct {
	job                *jobs.StandardJob
	guardrailJob       *jobs.GuardrailJob
	contentConsumption *jobs.ContentConsumptionJob
	surrogateJob       *jobs.SurrogateJob
	interleavingJob    *jobs.InterleavingJob
	recalibrationJob   *jobs.RecalibrationJob
	queryLog           querylog.Writer
	// shadowStore is optional (ADR-026 Phase 3 #437). When nil the three
	// shadow-run RPCs return CodeUnavailable so existing callers are unaffected.
	shadowStore shadow.Store
	// catalogReader is optional (Issue #597). When wired, the
	// CompileMetricqlPreview RPC populates KnownMetricIDs from M5's global
	// metric_definitions catalog when the request omits experiment_id, so
	// unknown @metric_refs surface as SEVERITY_ERROR diagnostics. When nil
	// (legacy callers / older tests) the empty-experiment_id path falls back
	// to KnownMetricIDs=nil (existence check skipped) for backward compat.
	catalogReader catalog.CatalogReader
}

// MetricsHandlerOption configures optional MetricsHandler behavior.
// Functional options keep the existing constructor wire-compatible while
// allowing cmd/main.go and new tests to inject production/mock stores.
type MetricsHandlerOption func(*MetricsHandler)

// WithShadowStore wires a shadow.Store for the three shadow-run RPCs.
// When unset (the default for all existing tests), shadow RPCs return
// CodeUnavailable — no production behavior changes.
func WithShadowStore(s shadow.Store) MetricsHandlerOption {
	return func(h *MetricsHandler) { h.shadowStore = s }
}

// WithCatalogReader wires a catalog.CatalogReader so that
// CompileMetricqlPreview can populate KnownMetricIDs from M5's global
// `metric_definitions` table when the request omits experiment_id (Issue
// #597). When unset, the empty-experiment_id path falls back to skipping
// the existence check (KnownMetricIDs=nil) — preserves backward compat for
// legacy callers / tests with no Postgres dependency.
func WithCatalogReader(r catalog.CatalogReader) MetricsHandlerOption {
	return func(h *MetricsHandler) { h.catalogReader = r }
}

func NewMetricsHandler(job *jobs.StandardJob, gj *jobs.GuardrailJob, ccj *jobs.ContentConsumptionJob, sj *jobs.SurrogateJob, ilj *jobs.InterleavingJob, rj *jobs.RecalibrationJob, ql querylog.Writer, opts ...MetricsHandlerOption) *MetricsHandler {
	h := &MetricsHandler{job: job, guardrailJob: gj, contentConsumption: ccj, surrogateJob: sj, interleavingJob: ilj, recalibrationJob: rj, queryLog: ql}
	for _, opt := range opts {
		opt(h)
	}
	return h
}

func (h *MetricsHandler) ComputeMetrics(ctx context.Context, req *connect.Request[metricsv1.ComputeMetricsRequest]) (*connect.Response[metricsv1.ComputeMetricsResponse], error) {
	start := time.Now()
	id := req.Msg.GetExperimentId()
	if id == "" {
		m3metrics.RPCTotal.WithLabelValues("ComputeMetrics", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ComputeMetrics", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	jobStart := time.Now()
	result, err := h.job.Run(ctx, id)
	if err != nil {
		m3metrics.JobTotal.WithLabelValues("standard", "error").Inc()
		m3metrics.RPCTotal.WithLabelValues("ComputeMetrics", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ComputeMetrics", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	m3metrics.JobDuration.WithLabelValues("standard", id).Observe(time.Since(jobStart).Seconds())
	m3metrics.JobTotal.WithLabelValues("standard", "ok").Inc()
	// Also compute content consumption distributions for interference analysis.
	if h.contentConsumption != nil {
		ccStart := time.Now()
		if _, err := h.contentConsumption.Run(ctx, id); err != nil {
			m3metrics.JobTotal.WithLabelValues("content_consumption", "error").Inc()
			m3metrics.RPCTotal.WithLabelValues("ComputeMetrics", "error").Inc()
			m3metrics.RPCDuration.WithLabelValues("ComputeMetrics", "error").Observe(time.Since(start).Seconds())
			return nil, connect.NewError(connect.CodeInternal, err)
		}
		m3metrics.JobDuration.WithLabelValues("content_consumption", id).Observe(time.Since(ccStart).Seconds())
		m3metrics.JobTotal.WithLabelValues("content_consumption", "ok").Inc()
	}
	// Compute surrogate projections if a model is linked.
	if h.surrogateJob != nil {
		surrStart := time.Now()
		if _, err := h.surrogateJob.Run(ctx, id); err != nil {
			m3metrics.JobTotal.WithLabelValues("surrogate", "error").Inc()
			m3metrics.RPCTotal.WithLabelValues("ComputeMetrics", "error").Inc()
			m3metrics.RPCDuration.WithLabelValues("ComputeMetrics", "error").Observe(time.Since(start).Seconds())
			return nil, connect.NewError(connect.CodeInternal, err)
		}
		m3metrics.JobDuration.WithLabelValues("surrogate", id).Observe(time.Since(surrStart).Seconds())
		m3metrics.JobTotal.WithLabelValues("surrogate", "ok").Inc()
	}
	// Compute interleaving scores for INTERLEAVING experiments.
	if h.interleavingJob != nil {
		ilStart := time.Now()
		if _, err := h.interleavingJob.Run(ctx, id); err != nil {
			m3metrics.JobTotal.WithLabelValues("interleaving", "error").Inc()
			m3metrics.RPCTotal.WithLabelValues("ComputeMetrics", "error").Inc()
			m3metrics.RPCDuration.WithLabelValues("ComputeMetrics", "error").Observe(time.Since(start).Seconds())
			return nil, connect.NewError(connect.CodeInternal, err)
		}
		m3metrics.JobDuration.WithLabelValues("interleaving", id).Observe(time.Since(ilStart).Seconds())
		m3metrics.JobTotal.WithLabelValues("interleaving", "ok").Inc()
	}
	m3metrics.RPCTotal.WithLabelValues("ComputeMetrics", "ok").Inc()
	m3metrics.RPCDuration.WithLabelValues("ComputeMetrics", "ok").Observe(time.Since(start).Seconds())
	return connect.NewResponse(&metricsv1.ComputeMetricsResponse{
		ExperimentId: result.ExperimentID, MetricsComputed: int32(result.MetricsComputed),
		UsersProcessed: int32(result.UsersProcessed), CompletedAt: timestamppb.New(result.CompletedAt),
	}), nil
}

func (h *MetricsHandler) ComputeGuardrailMetrics(ctx context.Context, req *connect.Request[metricsv1.ComputeGuardrailMetricsRequest]) (*connect.Response[metricsv1.ComputeMetricsResponse], error) {
	start := time.Now()
	id := req.Msg.GetExperimentId()
	if id == "" {
		m3metrics.RPCTotal.WithLabelValues("ComputeGuardrailMetrics", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ComputeGuardrailMetrics", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	jobStart := time.Now()
	result, err := h.guardrailJob.Run(ctx, id)
	if err != nil {
		m3metrics.JobTotal.WithLabelValues("guardrail", "error").Inc()
		m3metrics.RPCTotal.WithLabelValues("ComputeGuardrailMetrics", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ComputeGuardrailMetrics", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	m3metrics.JobDuration.WithLabelValues("guardrail", id).Observe(time.Since(jobStart).Seconds())
	m3metrics.JobTotal.WithLabelValues("guardrail", "ok").Inc()
	m3metrics.RPCTotal.WithLabelValues("ComputeGuardrailMetrics", "ok").Inc()
	m3metrics.RPCDuration.WithLabelValues("ComputeGuardrailMetrics", "ok").Observe(time.Since(start).Seconds())
	return connect.NewResponse(&metricsv1.ComputeMetricsResponse{
		ExperimentId: result.ExperimentID, MetricsComputed: int32(result.GuardrailsChecked),
		CompletedAt: timestamppb.New(result.CompletedAt),
	}), nil
}

func (h *MetricsHandler) ExportNotebook(ctx context.Context, req *connect.Request[metricsv1.ExportNotebookRequest]) (*connect.Response[metricsv1.ExportNotebookResponse], error) {
	start := time.Now()
	id := req.Msg.GetExperimentId()
	if id == "" {
		m3metrics.RPCTotal.WithLabelValues("ExportNotebook", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ExportNotebook", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	entries, err := h.queryLog.GetLogs(ctx, id, "")
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("ExportNotebook", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ExportNotebook", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	if len(entries) == 0 {
		m3metrics.RPCTotal.WithLabelValues("ExportNotebook", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ExportNotebook", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeNotFound, fmt.Errorf("no query logs found for experiment %s", id))
	}
	format := req.Msg.GetNotebookFormat()
	var nbBytes []byte
	var filename string
	switch format {
	case "databricks":
		nbBytes, err = export.GenerateDatabricksNotebook(id, entries)
		filename = fmt.Sprintf("experiment_%s_%s.py", id, time.Now().Format("20060102"))
	default:
		nbBytes, err = export.GenerateNotebook(id, entries)
		filename = fmt.Sprintf("experiment_%s_%s.ipynb", id, time.Now().Format("20060102"))
	}
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("ExportNotebook", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ExportNotebook", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	m3metrics.RPCTotal.WithLabelValues("ExportNotebook", "ok").Inc()
	m3metrics.RPCDuration.WithLabelValues("ExportNotebook", "ok").Observe(time.Since(start).Seconds())
	return connect.NewResponse(&metricsv1.ExportNotebookResponse{
		NotebookContent: nbBytes, Filename: filename,
	}), nil
}

func (h *MetricsHandler) GetQueryLog(ctx context.Context, req *connect.Request[metricsv1.GetQueryLogRequest]) (*connect.Response[metricsv1.GetQueryLogResponse], error) {
	start := time.Now()
	id := req.Msg.GetExperimentId()
	if id == "" {
		m3metrics.RPCTotal.WithLabelValues("GetQueryLog", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("GetQueryLog", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	filter := querylog.LogFilter{
		ExperimentID: id,
		MetricID:     req.Msg.GetMetricId(),
		JobType:      req.Msg.GetJobType(),
		PageSize:     int(req.Msg.GetPageSize()),
		PageToken:    req.Msg.GetPageToken(),
	}
	if req.Msg.GetAfter() != nil {
		filter.After = req.Msg.GetAfter().AsTime()
	}
	if req.Msg.GetBefore() != nil {
		filter.Before = req.Msg.GetBefore().AsTime()
	}
	entries, nextToken, err := h.queryLog.GetLogsFiltered(ctx, filter)
	if err != nil {
		m3metrics.RPCTotal.WithLabelValues("GetQueryLog", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("GetQueryLog", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	pe := make([]*metricsv1.QueryLogEntry, len(entries))
	for i, e := range entries {
		pe[i] = &metricsv1.QueryLogEntry{
			ExperimentId: e.ExperimentID, MetricId: e.MetricID, SqlText: e.SQLText,
			RowCount: e.RowCount, DurationMs: e.DurationMs, ComputedAt: timestamppb.New(e.ComputedAt),
		}
	}
	m3metrics.RPCTotal.WithLabelValues("GetQueryLog", "ok").Inc()
	m3metrics.RPCDuration.WithLabelValues("GetQueryLog", "ok").Observe(time.Since(start).Seconds())
	return connect.NewResponse(&metricsv1.GetQueryLogResponse{Entries: pe, NextPageToken: nextToken}), nil
}
