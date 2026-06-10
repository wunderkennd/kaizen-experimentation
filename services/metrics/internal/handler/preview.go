package handler

import (
	"context"
	"log/slog"
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
// Scope (Issue #597): the preview RPC operates in either experiment scope
// or global scope, mirroring PR #595's `ValidateMetricql` change on the M5
// side:
//
//   - Empty / whitespace-only experiment_id → GLOBAL scope. If a
//     catalog.CatalogReader is wired via WithCatalogReader, KnownMetricIDs
//     is populated from M5's `metric_definitions` table so unknown
//     @metric_refs surface as SEVERITY_ERROR diagnostics. If no reader is
//     wired (legacy callers / tests with no Postgres), KnownMetricIDs falls
//     back to nil (existence check skipped) for backward compat.
//   - Non-empty experiment_id → EXPERIMENT scope. KnownMetricIDs stays nil
//     (current behavior). Experiment-scoped catalog lookup is a future task
//     — Issue #597 only widens the global path.
//
// Honors the incoming gRPC/Connect deadline: ctx.Err() is checked before
// parsing so an already-expired context fails fast with DeadlineExceeded.
func (h *MetricsHandler) CompileMetricqlPreview(
	ctx context.Context,
	req *connect.Request[metricsv1.CompileMetricqlPreviewRequest],
) (*connect.Response[metricsv1.CompileMetricqlPreviewResponse], error) {
	experimentID := req.Msg.GetExperimentId()
	isGlobalScope := strings.TrimSpace(experimentID) == ""

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

	// Build the @metric_ref existence-check context.
	//
	//   - Global scope + catalog reader wired → query the catalog.
	//   - Global scope, no catalog reader → nil (skip check; backward compat).
	//   - Experiment scope → nil (current behavior; experiment-scoped
	//     catalog lookup is intentionally out of scope for Issue #597).
	var knownIDs map[string]bool
	if isGlobalScope {
		if h.catalogReader != nil {
			ids, err := h.catalogReader.ListMetricIDs(ctx)
			if err != nil {
				// Treat catalog failure as Internal — the operator should
				// see this rather than a silent fall-through to "no
				// existence check" (which would hide unknown refs).
				return nil, connect.NewError(connect.CodeInternal, err)
			}
			knownIDs = make(map[string]bool, len(ids))
			for _, id := range ids {
				knownIDs[id] = true
			}
		} else {
			slog.DebugContext(ctx, "CompileMetricqlPreview: empty experiment_id with no catalogReader wired; skipping existence check (legacy backward-compat path)")
		}
	}

	sql, _, err := metricql.Compile(expr, metricql.CompileContext{
		// ExperimentID and MetricID are injected for template rendering.
		// The preview uses a sentinel value — operators see realistic but
		// non-binding SQL. The actual scheduling path injects real IDs.
		// For global scope we pass an empty ExperimentID; the templates
		// tolerate it because the rendered SQL is informational.
		ExperimentID:    experimentID,
		ComputationDate: "PREVIEW",
		MetricID:        "preview",
		KnownMetricIDs:  knownIDs,
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
