package handler

import (
	"context"
	"fmt"
	"time"

	"connectrpc.com/connect"
	"google.golang.org/protobuf/types/known/timestamppb"

	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"
	"github.com/org/experimentation/gen/go/experimentation/metrics/v1/metricsv1connect"

	"github.com/org/experimentation-platform/services/metrics/internal/export"
	"github.com/org/experimentation-platform/services/metrics/internal/jobs"
	m3metrics "github.com/org/experimentation-platform/services/metrics/internal/metrics"
	"github.com/org/experimentation-platform/services/metrics/internal/metricql"
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
)

var _ metricsv1connect.MetricComputationServiceHandler = (*MetricsHandler)(nil)

type MetricsHandler struct {
	job                 *jobs.StandardJob
	guardrailJob        *jobs.GuardrailJob
	contentConsumption  *jobs.ContentConsumptionJob
	surrogateJob        *jobs.SurrogateJob
	interleavingJob     *jobs.InterleavingJob
	recalibrationJob    *jobs.RecalibrationJob
	queryLog            querylog.Writer
}

func NewMetricsHandler(job *jobs.StandardJob, gj *jobs.GuardrailJob, ccj *jobs.ContentConsumptionJob, sj *jobs.SurrogateJob, ilj *jobs.InterleavingJob, rj *jobs.RecalibrationJob, ql querylog.Writer) *MetricsHandler {
	return &MetricsHandler{job: job, guardrailJob: gj, contentConsumption: ccj, surrogateJob: sj, interleavingJob: ilj, recalibrationJob: rj, queryLog: ql}
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

func (h *MetricsHandler) ValidateMetricql(ctx context.Context, req *connect.Request[metricsv1.ValidateMetricqlRequest]) (*connect.Response[metricsv1.ValidateMetricqlResponse], error) {
	start := time.Now()
	expr := req.Msg.GetExpression()
	metricID := req.Msg.GetMetricId()

	if expr == "" {
		m3metrics.RPCTotal.WithLabelValues("ValidateMetricql", "error").Inc()
		m3metrics.RPCDuration.WithLabelValues("ValidateMetricql", "error").Observe(time.Since(start).Seconds())
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("expression is required"))
	}

	var diagnostics []*metricsv1.MetricqlDiagnostic

	// 1. Lex and Parse
	ast, err := metricql.Parse(expr)
	if err != nil {
		var line, col int32
		var msg string
		switch e := err.(type) {
		case *metricql.LexError:
			line, col = offsetToLineCol(expr, e.Span.Start)
			msg = e.Message
		case *metricql.ParseError:
			line, col = offsetToLineCol(expr, e.Span.Start)
			msg = e.Message
		default:
			line, col = 1, 1
			msg = err.Error()
		}
		diagnostics = append(diagnostics, &metricsv1.MetricqlDiagnostic{
			Message:  msg,
			Line:     line,
			Column:   col,
			Severity: 1, // ERROR
		})
		m3metrics.RPCTotal.WithLabelValues("ValidateMetricql", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("ValidateMetricql", "ok").Observe(time.Since(start).Seconds())
		return connect.NewResponse(&metricsv1.ValidateMetricqlResponse{
			IsValid:     false,
			Diagnostics: diagnostics,
		}), nil
	}

	// 2. Semantic Analysis
	cfg := h.job.ConfigStore()
	knownMetricIDs := cfg.MetricIDs()

	analyzeCtx := metricql.AnalyzeContext{
		KnownMetricIDs: knownMetricIDs,
	}

	if err := metricql.Analyze(ast, analyzeCtx); err != nil {
		var line, col int32
		var msg string
		switch e := err.(type) {
		case *metricql.AnalyzeError:
			line, col = offsetToLineCol(expr, e.Span.Start)
			msg = e.Message
		default:
			line, col = 1, 1
			msg = err.Error()
		}
		diagnostics = append(diagnostics, &metricsv1.MetricqlDiagnostic{
			Message:  msg,
			Line:     line,
			Column:   col,
			Severity: 1, // ERROR
		})
		m3metrics.RPCTotal.WithLabelValues("ValidateMetricql", "ok").Inc()
		m3metrics.RPCDuration.WithLabelValues("ValidateMetricql", "ok").Observe(time.Since(start).Seconds())
		return connect.NewResponse(&metricsv1.ValidateMetricqlResponse{
			IsValid:     false,
			Diagnostics: diagnostics,
		}), nil
	}

	// 3. Cycle Detection
	if metricID != "" {
		directOperands := metricql.ExtractMetricRefs(ast)
		lookup := func(mid string) ([]string, bool) {
			if mid == metricID {
				return directOperands, true
			}
			m, err := cfg.GetMetric(mid)
			if err != nil {
				return nil, false
			}
			ops, err := jobs.OperandIDs(m)
			if err != nil {
				return nil, false
			}
			return ops, true
		}

		if err := metricql.CheckNoCycles(metricID, directOperands, lookup); err != nil {
			var msg string
			switch e := err.(type) {
			case *metricql.CycleError:
				msg = e.Message
			default:
				msg = err.Error()
			}
			diagnostics = append(diagnostics, &metricsv1.MetricqlDiagnostic{
				Message:  msg,
				Line:     1,
				Column:   1,
				Severity: 1, // ERROR
			})
			m3metrics.RPCTotal.WithLabelValues("ValidateMetricql", "ok").Inc()
			m3metrics.RPCDuration.WithLabelValues("ValidateMetricql", "ok").Observe(time.Since(start).Seconds())
			return connect.NewResponse(&metricsv1.ValidateMetricqlResponse{
				IsValid:     false,
				Diagnostics: diagnostics,
			}), nil
		}
	}

	m3metrics.RPCTotal.WithLabelValues("ValidateMetricql", "ok").Inc()
	m3metrics.RPCDuration.WithLabelValues("ValidateMetricql", "ok").Observe(time.Since(start).Seconds())
	return connect.NewResponse(&metricsv1.ValidateMetricqlResponse{
		IsValid:     true,
		Diagnostics: nil,
	}), nil
}

func offsetToLineCol(source string, offset int) (int32, int32) {
	if offset < 0 {
		return 1, 1
	}
	if offset > len(source) {
		offset = len(source)
	}
	line := int32(1)
	col := int32(1)
	for i := 0; i < offset; i++ {
		if source[i] == '\n' {
			line++
			col = 1
		} else {
			col++
		}
	}
	return line, col
}

