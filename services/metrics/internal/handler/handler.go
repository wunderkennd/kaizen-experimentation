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
	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
)

var _ metricsv1connect.MetricComputationServiceHandler = (*MetricsHandler)(nil)

type MetricsHandler struct {
	job                 *jobs.StandardJob
	guardrailJob        *jobs.GuardrailJob
	contentConsumption  *jobs.ContentConsumptionJob
	surrogateJob        *jobs.SurrogateJob
	interleavingJob     *jobs.InterleavingJob
	queryLog            querylog.Writer
}

func NewMetricsHandler(job *jobs.StandardJob, gj *jobs.GuardrailJob, ccj *jobs.ContentConsumptionJob, sj *jobs.SurrogateJob, ilj *jobs.InterleavingJob, ql querylog.Writer) *MetricsHandler {
	return &MetricsHandler{job: job, guardrailJob: gj, contentConsumption: ccj, surrogateJob: sj, interleavingJob: ilj, queryLog: ql}
}

func (h *MetricsHandler) ComputeMetrics(ctx context.Context, req *connect.Request[metricsv1.ComputeMetricsRequest]) (*connect.Response[metricsv1.ComputeMetricsResponse], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	result, err := h.job.Run(ctx, id)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	// Also compute content consumption distributions for interference analysis.
	if h.contentConsumption != nil {
		if _, err := h.contentConsumption.Run(ctx, id); err != nil {
			return nil, connect.NewError(connect.CodeInternal, err)
		}
	}
	// Compute surrogate projections if a model is linked.
	if h.surrogateJob != nil {
		if _, err := h.surrogateJob.Run(ctx, id); err != nil {
			return nil, connect.NewError(connect.CodeInternal, err)
		}
	}
	// Compute interleaving scores for INTERLEAVING experiments.
	if h.interleavingJob != nil {
		if _, err := h.interleavingJob.Run(ctx, id); err != nil {
			return nil, connect.NewError(connect.CodeInternal, err)
		}
	}
	return connect.NewResponse(&metricsv1.ComputeMetricsResponse{
		ExperimentId: result.ExperimentID, MetricsComputed: int32(result.MetricsComputed),
		UsersProcessed: int32(result.UsersProcessed), CompletedAt: timestamppb.New(result.CompletedAt),
	}), nil
}

func (h *MetricsHandler) ComputeGuardrailMetrics(ctx context.Context, req *connect.Request[metricsv1.ComputeGuardrailMetricsRequest]) (*connect.Response[metricsv1.ComputeMetricsResponse], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	result, err := h.guardrailJob.Run(ctx, id)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	return connect.NewResponse(&metricsv1.ComputeMetricsResponse{
		ExperimentId: result.ExperimentID, MetricsComputed: int32(result.GuardrailsChecked),
		CompletedAt: timestamppb.New(result.CompletedAt),
	}), nil
}

func (h *MetricsHandler) ExportNotebook(ctx context.Context, req *connect.Request[metricsv1.ExportNotebookRequest]) (*connect.Response[metricsv1.ExportNotebookResponse], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	entries, err := h.queryLog.GetLogs(ctx, id, "")
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	if len(entries) == 0 {
		return nil, connect.NewError(connect.CodeNotFound, fmt.Errorf("no query logs found for experiment %s", id))
	}
	nbBytes, err := export.GenerateNotebook(id, entries)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	return connect.NewResponse(&metricsv1.ExportNotebookResponse{
		NotebookContent: nbBytes, Filename: fmt.Sprintf("experiment_%s_%s.ipynb", id, time.Now().Format("20060102")),
	}), nil
}

func (h *MetricsHandler) GetQueryLog(ctx context.Context, req *connect.Request[metricsv1.GetQueryLogRequest]) (*connect.Response[metricsv1.GetQueryLogResponse], error) {
	id := req.Msg.GetExperimentId()
	if id == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	entries, err := h.queryLog.GetLogs(ctx, id, req.Msg.GetMetricId())
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}
	pe := make([]*metricsv1.QueryLogEntry, len(entries))
	for i, e := range entries {
		pe[i] = &metricsv1.QueryLogEntry{
			ExperimentId: e.ExperimentID, MetricId: e.MetricID, SqlText: e.SQLText,
			RowCount: e.RowCount, DurationMs: e.DurationMs, ComputedAt: timestamppb.New(e.ComputedAt),
		}
	}
	return connect.NewResponse(&metricsv1.GetQueryLogResponse{Entries: pe}), nil
}
