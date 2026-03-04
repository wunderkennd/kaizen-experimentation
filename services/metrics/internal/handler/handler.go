// Package handler implements the ConnectRPC MetricComputationServiceHandler.
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

// Ensure MetricsHandler satisfies the generated interface at compile time.
var _ metricsv1connect.MetricComputationServiceHandler = (*MetricsHandler)(nil)

// MetricsHandler implements the MetricComputationService RPC methods.
type MetricsHandler struct {
	job      *jobs.StandardJob
	queryLog querylog.Writer
}

// NewMetricsHandler creates a new handler with the given dependencies.
func NewMetricsHandler(job *jobs.StandardJob, ql querylog.Writer) *MetricsHandler {
	return &MetricsHandler{
		job:      job,
		queryLog: ql,
	}
}

func (h *MetricsHandler) ComputeMetrics(
	ctx context.Context,
	req *connect.Request[metricsv1.ComputeMetricsRequest],
) (*connect.Response[metricsv1.ComputeMetricsResponse], error) {
	experimentID := req.Msg.GetExperimentId()
	if experimentID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}

	result, err := h.job.Run(ctx, experimentID)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	resp := &metricsv1.ComputeMetricsResponse{
		ExperimentId:    result.ExperimentID,
		MetricsComputed: int32(result.MetricsComputed),
		UsersProcessed:  int32(result.UsersProcessed),
		CompletedAt:     timestamppb.New(result.CompletedAt),
	}
	return connect.NewResponse(resp), nil
}

func (h *MetricsHandler) ComputeGuardrailMetrics(
	ctx context.Context,
	req *connect.Request[metricsv1.ComputeGuardrailMetricsRequest],
) (*connect.Response[metricsv1.ComputeMetricsResponse], error) {
	// Stub — Milestone 1.13.
	return nil, connect.NewError(connect.CodeUnimplemented, fmt.Errorf("guardrail metrics not yet implemented"))
}

func (h *MetricsHandler) ExportNotebook(
	ctx context.Context,
	req *connect.Request[metricsv1.ExportNotebookRequest],
) (*connect.Response[metricsv1.ExportNotebookResponse], error) {
	experimentID := req.Msg.GetExperimentId()
	if experimentID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}

	entries, err := h.queryLog.GetLogs(ctx, experimentID, "")
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	if len(entries) == 0 {
		return nil, connect.NewError(connect.CodeNotFound,
			fmt.Errorf("no query logs found for experiment %s", experimentID))
	}

	nbBytes, err := export.GenerateNotebook(experimentID, entries)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	filename := fmt.Sprintf("experiment_%s_%s.ipynb",
		experimentID, time.Now().Format("20060102"))

	resp := &metricsv1.ExportNotebookResponse{
		NotebookContent: nbBytes,
		Filename:        filename,
	}
	return connect.NewResponse(resp), nil
}

func (h *MetricsHandler) GetQueryLog(
	ctx context.Context,
	req *connect.Request[metricsv1.GetQueryLogRequest],
) (*connect.Response[metricsv1.GetQueryLogResponse], error) {
	experimentID := req.Msg.GetExperimentId()
	if experimentID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}

	entries, err := h.queryLog.GetLogs(ctx, experimentID, req.Msg.GetMetricId())
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	protoEntries := make([]*metricsv1.QueryLogEntry, len(entries))
	for i, e := range entries {
		protoEntries[i] = &metricsv1.QueryLogEntry{
			ExperimentId: e.ExperimentID,
			MetricId:     e.MetricID,
			SqlText:      e.SQLText,
			RowCount:     e.RowCount,
			DurationMs:   e.DurationMs,
			ComputedAt:   timestamppb.New(e.ComputedAt),
		}
	}

	resp := &metricsv1.GetQueryLogResponse{Entries: protoEntries}
	return connect.NewResponse(resp), nil
}
