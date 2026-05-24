package handler

import (
	"context"
	"strings"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	metricsv1 "github.com/org/experimentation/gen/go/experimentation/metrics/v1"

	"github.com/org/experimentation-platform/services/metrics/internal/metricql"
)

// CompileMetricqlPreview implements MetricComputationServiceHandler.
//
// It parses + compiles the given MetricQL expression to Spark SQL without
// persisting or executing. Used by the M6 editor's preview pane (ADR-026
// Phase 2 / #436).
//
// KnownMetricIDs is intentionally not derived from the experiment in v1:
// the preview is informational and the M5 Create/Update path is the source
// of truth for existence checks. Passing nil to AnalyzeContext skips the
// @metric_ref existence check (per the analyzer's documented nil-means-skip
// contract).
//
// Honors the incoming gRPC/Connect deadline: ctx.Err() is checked before
// parsing so an already-expired context fails fast with DeadlineExceeded.
func (h *MetricsHandler) CompileMetricqlPreview(
	ctx context.Context,
	req *connect.Request[metricsv1.CompileMetricqlPreviewRequest],
) (*connect.Response[metricsv1.CompileMetricqlPreviewResponse], error) {
	if req.Msg.GetExperimentId() == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, errorf("experiment_id is required"))
	}

	expr := strings.TrimSpace(req.Msg.GetMetricqlExpression())
	if expr == "" {
		return connect.NewResponse(&metricsv1.CompileMetricqlPreviewResponse{
			Diagnostics: []*commonv1.MetricqlDiagnostic{
				errorDiagnostic("empty MetricQL expression", 0, 0, 1, 1),
			},
		}), nil
	}

	// Fail fast if the deadline is already past before we do any work.
	if err := ctx.Err(); err != nil {
		return nil, connect.NewError(connect.CodeDeadlineExceeded, err)
	}

	// Phase 2 v1: skip KnownMetricIDs existence check in the preview path.
	// The analyzer's nil-KnownMetricIDs contract explicitly allows this.
	sql, _, err := metricql.Compile(expr, metricql.CompileContext{
		// ExperimentID and MetricID are injected for template rendering.
		// The preview uses a sentinel value — operators see realistic but
		// non-binding SQL. The actual scheduling path injects real IDs.
		ExperimentID:    req.Msg.GetExperimentId(),
		ComputationDate: "PREVIEW",
		MetricID:        "preview",
		KnownMetricIDs:  nil,
	})
	if err != nil {
		return connect.NewResponse(diagnosticResponseFromError(expr, err)), nil
	}

	return connect.NewResponse(&metricsv1.CompileMetricqlPreviewResponse{
		CompiledSql: sql,
	}), nil
}

// diagnosticResponseFromError converts a single Go error into the proto
// diagnostic shape. Typed errors (ParseError, LexError, AnalyzeError,
// CompileError from the metricql package) carry a Span; all other errors
// default to offset (0, 0) / line 1 col 1.
func diagnosticResponseFromError(source string, err error) *metricsv1.CompileMetricqlPreviewResponse {
	var diag *commonv1.MetricqlDiagnostic

	switch e := err.(type) {
	case *metricql.ParseError:
		line, col := lineColFromOffset(source, e.Span.Start)
		diag = errorDiagnostic(e.Error(), int32(e.Span.Start), int32(e.Span.End), int32(line), int32(col))
	case *metricql.LexError:
		line, col := lineColFromOffset(source, e.Span.Start)
		diag = errorDiagnostic(e.Error(), int32(e.Span.Start), int32(e.Span.End), int32(line), int32(col))
	case *metricql.AnalyzeError:
		line, col := lineColFromOffset(source, e.Span.Start)
		diag = errorDiagnostic(e.Error(), int32(e.Span.Start), int32(e.Span.End), int32(line), int32(col))
	case *metricql.CompileError:
		line, col := lineColFromOffset(source, e.Span.Start)
		diag = errorDiagnostic(e.Error(), int32(e.Span.Start), int32(e.Span.End), int32(line), int32(col))
	default:
		diag = errorDiagnostic(err.Error(), 0, 0, 1, 1)
	}

	return &metricsv1.CompileMetricqlPreviewResponse{
		Diagnostics: []*commonv1.MetricqlDiagnostic{diag},
	}
}

// errorDiagnostic constructs a SEVERITY_ERROR MetricqlDiagnostic.
func errorDiagnostic(message string, startOffset, endOffset, line, column int32) *commonv1.MetricqlDiagnostic {
	return &commonv1.MetricqlDiagnostic{
		Severity: commonv1.MetricqlDiagnostic_SEVERITY_ERROR,
		Message:  message,
		Span: &commonv1.MetricqlDiagnostic_Span{
			StartOffset: startOffset,
			EndOffset:   endOffset,
			Line:        line,
			Column:      column,
		},
	}
}

// lineColFromOffset computes the 1-based (line, column) for a byte offset
// into a UTF-8 source string. Column is ASCII-naive (byte column, not
// grapheme cluster column) -- this matches the proto Span.column comment.
func lineColFromOffset(source string, offset int) (line, col int) {
	line, col = 1, 1
	for i := 0; i < len(source) && i < offset; i++ {
		if source[i] == '\n' {
			line++
			col = 1
		} else {
			col++
		}
	}
	return line, col
}

// errorf returns a plain error for use with connect.NewError. Using a
// package-local helper avoids importing fmt in the hot path of the handler.
func errorf(msg string) error {
	return &plainError{msg}
}

type plainError struct{ msg string }

func (e *plainError) Error() string { return e.msg }
